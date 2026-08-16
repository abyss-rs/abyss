use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::plan::{SyncComparison, SyncFile, SyncPlan, SyncReason, SyncStrategy};
use crate::storage::Location;

pub fn plan_local(
    source: &Path,
    destination: &Path,
    comparison: SyncComparison,
    strategy: SyncStrategy,
) -> Result<SyncPlan, String> {
    if !source.is_dir() {
        return Err(format!(
            "sync source is not a directory: {}",
            source.display()
        ));
    }
    if destination == source || destination.starts_with(source) {
        return Err(format!(
            "sync destination cannot be the source or inside it: {}",
            destination.display()
        ));
    }
    if destination.exists() && !destination.is_dir() {
        return Err(format!(
            "sync destination is not a directory: {}",
            destination.display()
        ));
    }
    let mut plan = SyncPlan {
        source: Location::Local(source.to_owned()),
        destination: Location::Local(destination.to_owned()),
        comparison,
        strategy,
        directories: Vec::new(),
        files: Vec::new(),
        deletions: Vec::new(),
        unchanged: 0,
        bytes: 0,
    };
    walk(
        source,
        destination,
        Path::new(""),
        comparison,
        strategy,
        &mut plan,
    )?;
    if destination.is_dir() {
        match strategy {
            SyncStrategy::Mirror => {
                walk_orphans_mirror(destination, source, Path::new(""), &mut plan)?;
            }
            SyncStrategy::TwoWay => {
                walk_twoway_dest(destination, source, Path::new(""), comparison, &mut plan)?;
            }
            SyncStrategy::UpdateOnly | SyncStrategy::DeltaRsync => {}
        }
    }
    plan.directories
        .sort_by_key(|location| location.display().matches('/').count());
    plan.files
        .sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(plan)
}

fn walk(
    source_root: &Path,
    destination_root: &Path,
    relative: &Path,
    comparison: SyncComparison,
    strategy: SyncStrategy,
    plan: &mut SyncPlan,
) -> Result<(), String> {
    let directory = source_root.join(relative);
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("read sync source {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read sync source {}: {error}", directory.display()))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b"._") {
            continue;
        }
        let relative = relative.join(name);
        let source = source_root.join(&relative);
        let destination = destination_root.join(&relative);
        let source_metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("inspect sync source {}: {error}", source.display()))?;
        if source_metadata.is_dir() {
            match fs::symlink_metadata(&destination) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return Err(format!(
                        "sync directory conflicts with a non-directory: {}",
                        destination.display()
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    plan.directories.push(Location::Local(destination.clone()));
                }
                Err(error) => {
                    return Err(format!(
                        "inspect sync destination {}: {error}",
                        destination.display()
                    ));
                }
            }
            walk(
                source_root,
                destination_root,
                &relative,
                comparison,
                strategy,
                plan,
            )?;
            continue;
        }
        let reason = compare_item(
            &source,
            &destination,
            &source_metadata,
            comparison,
            strategy,
        )?;
        if let Some(reason) = reason {
            let size = source_metadata.len();
            plan.bytes = plan.bytes.saturating_add(size);
            if strategy == SyncStrategy::TwoWay {
                let dest_time = fs::symlink_metadata(&destination)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let src_time = source_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if dest_time > src_time {
                    plan.files.push(SyncFile {
                        source: Location::Local(destination),
                        destination: Location::Local(source),
                        relative: relative.display().to_string(),
                        reason,
                    });
                    continue;
                }
            }
            plan.files.push(SyncFile {
                source: Location::Local(source),
                destination: Location::Local(destination),
                relative: relative.display().to_string(),
                reason,
            });
        } else {
            plan.unchanged += 1;
        }
    }
    Ok(())
}

fn walk_orphans_mirror(
    destination_root: &Path,
    source_root: &Path,
    relative: &Path,
    plan: &mut SyncPlan,
) -> Result<(), String> {
    let directory = destination_root.join(relative);
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("read sync destination {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read sync destination {}: {error}", directory.display()))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b"._") {
            continue;
        }
        let relative = relative.join(name);
        let dest_path = destination_root.join(&relative);
        let source_path = source_root.join(&relative);
        let dest_metadata = fs::symlink_metadata(&dest_path).map_err(|error| {
            format!("inspect sync destination {}: {error}", dest_path.display())
        })?;
        if !source_path.exists() {
            let size = if dest_metadata.is_file() {
                dest_metadata.len()
            } else {
                0
            };
            plan.bytes = plan.bytes.saturating_add(size);
            plan.files.push(SyncFile {
                source: Location::Local(dest_path.clone()),
                destination: Location::Local(dest_path.clone()),
                relative: relative.display().to_string(),
                reason: SyncReason::Orphaned,
            });
            plan.deletions.push(Location::Local(dest_path.clone()));
            if dest_metadata.is_dir() {
                continue;
            }
        } else if dest_metadata.is_dir() {
            walk_orphans_mirror(destination_root, source_root, &relative, plan)?;
        }
    }
    Ok(())
}

fn walk_twoway_dest(
    destination_root: &Path,
    source_root: &Path,
    relative: &Path,
    _comparison: SyncComparison,
    plan: &mut SyncPlan,
) -> Result<(), String> {
    let directory = destination_root.join(relative);
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("read sync destination {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read sync destination {}: {error}", directory.display()))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b"._") {
            continue;
        }
        let relative = relative.join(name);
        let dest_path = destination_root.join(&relative);
        let source_path = source_root.join(&relative);
        let dest_metadata = fs::symlink_metadata(&dest_path).map_err(|error| {
            format!("inspect sync destination {}: {error}", dest_path.display())
        })?;
        if dest_metadata.is_dir() {
            if !source_path.exists() {
                plan.directories.push(Location::Local(source_path.clone()));
            }
            walk_twoway_dest(destination_root, source_root, &relative, _comparison, plan)?;
        } else if !source_path.exists() {
            let size = dest_metadata.len();
            plan.bytes = plan.bytes.saturating_add(size);
            plan.files.push(SyncFile {
                source: Location::Local(dest_path),
                destination: Location::Local(source_path),
                relative: relative.display().to_string(),
                reason: SyncReason::Missing,
            });
        }
    }
    Ok(())
}

fn compare_item(
    source: &Path,
    destination: &Path,
    source_metadata: &fs::Metadata,
    comparison: SyncComparison,
    strategy: SyncStrategy,
) -> Result<Option<SyncReason>, String> {
    let destination_metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Some(SyncReason::Missing));
        }
        Err(error) => {
            return Err(format!(
                "inspect sync destination {}: {error}",
                destination.display()
            ));
        }
    };
    if source_metadata.file_type().is_symlink() != destination_metadata.file_type().is_symlink()
        || source_metadata.is_file() != destination_metadata.is_file()
    {
        return Ok(Some(SyncReason::TypeChanged));
    }
    if source_metadata.file_type().is_symlink() {
        let source_target = fs::read_link(source)
            .map_err(|error| format!("read link {}: {error}", source.display()))?;
        let destination_target = fs::read_link(destination)
            .map_err(|error| format!("read link {}: {error}", destination.display()))?;
        return Ok((source_target != destination_target).then_some(
            if comparison == SyncComparison::Checksum {
                SyncReason::ChecksumChanged
            } else {
                SyncReason::MetadataChanged
            },
        ));
    }
    if !source_metadata.is_file() {
        return Err(format!(
            "unsupported sync source object: {}",
            source.display()
        ));
    }
    if source_metadata.len() != destination_metadata.len() {
        return Ok(Some(
            if strategy == SyncStrategy::DeltaRsync || comparison == SyncComparison::DeltaSignature
            {
                SyncReason::DeltaPatchable
            } else {
                SyncReason::MetadataChanged
            },
        ));
    }
    match comparison {
        SyncComparison::Metadata => {
            let source_time = source_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let destination_time = destination_metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Ok((source_time != destination_time).then_some(
                if strategy == SyncStrategy::DeltaRsync {
                    SyncReason::DeltaPatchable
                } else {
                    SyncReason::MetadataChanged
                },
            ))
        }
        SyncComparison::Checksum => Ok((sha256(source)? != sha256(destination)?).then_some(
            if strategy == SyncStrategy::DeltaRsync {
                SyncReason::DeltaPatchable
            } else {
                SyncReason::ChecksumChanged
            },
        )),
        SyncComparison::DeltaSignature => {
            Ok((sha256(source)? != sha256(destination)?).then_some(SyncReason::DeltaPatchable))
        }
    }
}

fn sha256(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path)
        .map_err(|error| format!("open {} for checksum: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let amount = file
            .read(&mut buffer)
            .map_err(|error| format!("read {} for checksum: {error}", path.display()))?;
        if amount == 0 {
            break;
        }
        digest.update(&buffer[..amount]);
    }
    Ok(digest.finalize().into())
}
