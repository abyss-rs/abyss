use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::copy::target::{ensure_compatible_destination, resolve_target, validate_target};
use crate::copy::types::{ConflictDecision, ConflictResolver, OverwriteAll};
use crate::inventory::{EntryKind, Inventory};
use crate::native::{
    CloneCapabilities, apply_path_metadata, copy_regular_file, copy_symlink, try_hard_link,
};
use crate::progress::{CopyStats, OperationPhase};

pub fn run(source: &Path, destination: &Path, cancelled: Arc<AtomicBool>) -> Result<(), Error> {
    let stats = Arc::new(CopyStats::default());
    run_with_stats(source, destination, cancelled, stats, &OverwriteAll)
}

pub fn run_with_stats(
    source: &Path,
    destination: &Path,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    stats.reset();
    let prepared = prepare(source, destination, &cancelled, &stats)?;
    stats.set_totals(
        prepared.inventory.entries.len() as u64,
        prepared.inventory.logical_bytes,
    );
    stats.set_phase(OperationPhase::Copying);
    let mut clone_capabilities = CloneCapabilities::default();
    let mut hard_links = HashMap::new();
    execute(
        &prepared.source,
        &prepared.target,
        &prepared.inventory,
        &cancelled,
        &stats,
        conflicts,
        &mut clone_capabilities,
        &mut hard_links,
    )
}

pub fn run_batch(
    sources: &[PathBuf],
    destination_directory: &Path,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    let destination_metadata = fs::metadata(destination_directory).map_err(|error| {
        Error::io(
            "inspect destination directory",
            destination_directory,
            error,
        )
    })?;
    if !destination_metadata.is_dir() {
        return Err(Error::message(format!(
            "multiple items require a destination directory: {}",
            destination_directory.display()
        )));
    }

    stats.reset();
    let mut prepared = Vec::with_capacity(sources.len());
    let mut total_objects = 0_u64;
    let mut total_bytes = 0_u64;
    for source in sources {
        let item = prepare(source, destination_directory, &cancelled, &stats)?;
        total_objects = total_objects.saturating_add(item.inventory.entries.len() as u64);
        total_bytes = total_bytes.saturating_add(item.inventory.logical_bytes);
        prepared.push(item);
    }
    stats.set_totals(total_objects, total_bytes);
    stats.set_phase(OperationPhase::Copying);

    let mut clone_capabilities = CloneCapabilities::default();
    let mut hard_links = HashMap::new();
    (|| {
        for item in prepared {
            execute(
                &item.source,
                &item.target,
                &item.inventory,
                &cancelled,
                &stats,
                conflicts,
                &mut clone_capabilities,
                &mut hard_links,
            )?;
        }
        Ok(())
    })()
}

pub(crate) struct PreparedCopy {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
    pub(crate) inventory: Inventory,
}

pub(crate) fn prepare(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<PreparedCopy, Error> {
    let source_metadata =
        fs::symlink_metadata(source).map_err(|error| Error::io("inspect source", source, error))?;
    let target = resolve_target(source, destination)?;
    validate_target(source, &source_metadata, &target)?;
    let inventory = Inventory::scan_for_copy_with_progress(source, cancelled, Some(stats))?;
    Ok(PreparedCopy {
        source: source.to_owned(),
        target,
        inventory,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    source_root: &Path,
    target_root: &Path,
    inventory: &Inventory,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
    clone_capabilities: &mut CloneCapabilities,
    hard_links: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<(), Error> {
    for entry in &inventory.entries {
        check_cancelled(cancelled)?;
        if !stats.wait_for_transfer(cancelled, 0) {
            return Err(Error::Cancelled);
        }
        let source = path_for_entry(source_root, &entry.relative);
        let destination = path_for_entry(target_root, &entry.relative);
        ensure_compatible_destination(&destination, entry.kind)?;

        match entry.kind {
            EntryKind::Directory => {
                if !destination.exists() {
                    fs::create_dir(&destination)
                        .map_err(|error| Error::io("create directory", &destination, error))?;
                }
            }
            EntryKind::Symlink => {
                if destination_exists(&destination)? {
                    match conflicts.resolve(&destination)? {
                        ConflictDecision::Skip => {
                            stats.skip_object(&destination, 0);
                            continue;
                        }
                        ConflictDecision::Cancel => return Err(Error::Cancelled),
                        ConflictDecision::Overwrite => {}
                    }
                }
                copy_symlink(&source, &destination, entry)?;
                stats.complete_object(&destination);
            }
            EntryKind::File => {
                if destination_exists(&destination)? {
                    match conflicts.resolve(&destination)? {
                        ConflictDecision::Overwrite => {}
                        ConflictDecision::Skip => {
                            stats.skip_object(&destination, entry.len);
                            continue;
                        }
                        ConflictDecision::Cancel => return Err(Error::Cancelled),
                    }
                }
                stats.begin_file(&destination, entry.len);
                let key = (entry.device, entry.inode);
                let linked_outcome = if entry.links > 1 {
                    hard_links
                        .get(&key)
                        .map(|existing| try_hard_link(existing, &destination, entry))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };

                let outcome = match linked_outcome {
                    Some(outcome) => outcome,
                    None => copy_regular_file(
                        &source,
                        &destination,
                        entry,
                        cancelled,
                        stats,
                        clone_capabilities,
                    )?,
                };
                if entry.links > 1 {
                    hard_links.entry(key).or_insert(destination.clone());
                }
                stats.complete_file(
                    entry.len,
                    outcome.physical_bytes,
                    outcome.cloned,
                    outcome.linked,
                );
            }
            EntryKind::Other => {
                return Err(Error::message(format!(
                    "unsupported filesystem object: {}",
                    source.display()
                )));
            }
        }
    }

    for entry in inventory.entries.iter().rev() {
        if entry.kind != EntryKind::Directory {
            continue;
        }
        check_cancelled(cancelled)?;
        let destination = path_for_entry(target_root, &entry.relative);
        apply_path_metadata(&destination, entry)?;
        stats.complete_object(&destination);
    }

    Ok(())
}

pub(crate) fn path_for_entry(root: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        root.to_owned()
    } else {
        root.join(relative)
    }
}

pub(crate) fn destination_exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::io("inspect destination", path, error)),
    }
}

pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), Error> {
    if cancelled.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
