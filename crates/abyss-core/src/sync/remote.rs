#[cfg(feature = "tokio")]
use std::fs::{self, File};
#[cfg(feature = "tokio")]
use std::io::{self, Read};
use std::sync::Arc;

#[cfg(feature = "tokio")]
use futures_util::StreamExt;
#[cfg(feature = "tokio")]
use sha2::{Digest, Sha256};

use super::local::plan_local;
use super::plan::{SyncComparison, SyncPlan, SyncStrategy};
#[cfg(feature = "tokio")]
use super::plan::{SyncFile, SyncReason};
#[cfg(feature = "tokio")]
use crate::storage::{EntryKind, ErrorKind, StorageEntry, StorageError};
use crate::storage::{Location, StorageRuntime};

pub fn plan_locations(
    storage: Arc<StorageRuntime>,
    source: Location,
    destination: Location,
    comparison: SyncComparison,
    strategy: SyncStrategy,
) -> Result<SyncPlan, String> {
    if let (Location::Local(source), Location::Local(destination)) = (&source, &destination) {
        return plan_local(source, destination, comparison, strategy);
    }
    #[cfg(feature = "tokio")]
    {
        storage
            .block_on(plan_remote(
                Arc::clone(&storage),
                source,
                destination,
                comparison,
                strategy,
            ))
            .map_err(|error| error.to_string())
    }
    #[cfg(not(feature = "tokio"))]
    {
        let _ = (storage, comparison, strategy);
        Err("remote sync requires --features remote".to_owned())
    }
}

#[cfg(feature = "tokio")]
async fn plan_remote(
    storage: Arc<StorageRuntime>,
    source: Location,
    destination: Location,
    comparison: SyncComparison,
    strategy: SyncStrategy,
) -> Result<SyncPlan, StorageError> {
    let mut plan = SyncPlan {
        source: source.clone(),
        destination: destination.clone(),
        comparison,
        strategy,
        directories: Vec::new(),
        files: Vec::new(),
        deletions: Vec::new(),
        unchanged: 0,
        bytes: 0,
    };
    let mut pending = vec![(source, destination, String::new())];
    while let Some((source_directory, destination_directory, prefix)) = pending.pop() {
        for entry in list_all(&storage, &source_directory).await? {
            if entry.name.starts_with(b"._") {
                continue;
            }
            let source_item = source_directory.child(&entry.name)?;
            let destination_item = destination_directory.child(&entry.name)?;
            let name = String::from_utf8_lossy(&entry.name);
            let relative = if prefix.is_empty() {
                name.into_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let destination_entry = stat_optional(&storage, &destination_item).await?;
            if entry.kind == EntryKind::Directory {
                match destination_entry {
                    Some(item) if item.kind == EntryKind::Directory => {}
                    Some(_) => {
                        return Err(StorageError::new(
                            ErrorKind::Conflict,
                            format!(
                                "sync directory conflicts with a non-directory: {}",
                                destination_item.display()
                            ),
                        ));
                    }
                    None => plan.directories.push(destination_item.clone()),
                }
                pending.push((source_item, destination_item, relative));
                continue;
            }
            let reason = compare_remote_item(
                &storage,
                &source_item,
                &destination_item,
                &entry,
                destination_entry.as_ref(),
                comparison,
            )
            .await?;
            if let Some(reason) = reason {
                let size = entry.size.unwrap_or(0);
                plan.bytes = plan.bytes.saturating_add(size);
                plan.files.push(SyncFile {
                    source: source_item,
                    destination: destination_item,
                    relative,
                    reason,
                });
            } else {
                plan.unchanged += 1;
            }
        }
    }
    plan.directories
        .sort_by_key(|location| location.display().matches('/').count());
    plan.files
        .sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(plan)
}

#[cfg(feature = "tokio")]
async fn compare_remote_item(
    storage: &StorageRuntime,
    source: &Location,
    destination: &Location,
    source_entry: &StorageEntry,
    destination_entry: Option<&StorageEntry>,
    comparison: SyncComparison,
) -> Result<Option<SyncReason>, StorageError> {
    let Some(destination_entry) = destination_entry else {
        return Ok(Some(SyncReason::Missing));
    };
    if source_entry.kind != destination_entry.kind {
        return Ok(Some(SyncReason::TypeChanged));
    }
    if source_entry.size != destination_entry.size {
        return Ok(Some(SyncReason::MetadataChanged));
    }
    if source_entry.kind != EntryKind::File {
        return Ok((source_entry.modified != destination_entry.modified)
            .then_some(SyncReason::MetadataChanged));
    }
    match comparison {
        SyncComparison::Metadata => Ok((source_entry.modified != destination_entry.modified)
            .then_some(SyncReason::MetadataChanged)),
        SyncComparison::Checksum => Ok((digest_location(storage, source).await?
            != digest_location(storage, destination).await?)
            .then_some(SyncReason::ChecksumChanged)),
        SyncComparison::DeltaSignature => Ok((digest_location(storage, source).await?
            != digest_location(storage, destination).await?)
            .then_some(SyncReason::DeltaPatchable)),
    }
}

#[cfg(feature = "tokio")]
async fn list_all(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<Vec<StorageEntry>, StorageError> {
    match location {
        Location::Local(path) => {
            let mut entries = Vec::new();
            for item in fs::read_dir(path).map_err(storage_io)? {
                let item = item.map_err(storage_io)?;
                let metadata = fs::symlink_metadata(item.path()).map_err(storage_io)?;
                entries.push(StorageEntry {
                    name: item.file_name().as_encoded_bytes().to_vec(),
                    kind: if metadata.is_dir() {
                        EntryKind::Directory
                    } else if metadata.is_file() {
                        EntryKind::File
                    } else if metadata.file_type().is_symlink() {
                        EntryKind::Symlink
                    } else {
                        EntryKind::Other
                    },
                    size: metadata.is_file().then_some(metadata.len()),
                    modified: metadata.modified().ok(),
                    version: None,
                });
            }
            Ok(entries)
        }
        Location::Remote(remote) => {
            let backend = storage.backend_async(remote).await?;
            let mut entries = Vec::new();
            let mut continuation = None;
            loop {
                let page = backend.list(&remote.path, continuation.as_deref()).await?;
                entries.extend(page.entries);
                continuation = page.continuation;
                if continuation.is_none() {
                    break;
                }
            }
            Ok(entries)
        }
    }
}

#[cfg(feature = "tokio")]
async fn stat_optional(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<Option<StorageEntry>, StorageError> {
    let result = match location {
        Location::Local(path) => fs::symlink_metadata(path)
            .map(|metadata| StorageEntry {
                name: path
                    .file_name()
                    .map(|name| name.as_encoded_bytes().to_vec())
                    .unwrap_or_default(),
                kind: if metadata.is_dir() {
                    EntryKind::Directory
                } else if metadata.is_file() {
                    EntryKind::File
                } else if metadata.file_type().is_symlink() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Other
                },
                size: metadata.is_file().then_some(metadata.len()),
                modified: metadata.modified().ok(),
                version: None,
            })
            .map_err(storage_io),
        Location::Remote(remote) => {
            storage
                .backend_async(remote)
                .await?
                .stat(&remote.path)
                .await
        }
    };
    match result {
        Ok(entry) => Ok(Some(entry)),
        Err(error) if error.kind == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "tokio")]
async fn digest_location(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<[u8; 32], StorageError> {
    let mut digest = Sha256::new();
    match location {
        Location::Local(path) => {
            let mut file = File::open(path).map_err(storage_io)?;
            let mut buffer = vec![0_u8; 1024 * 1024];
            loop {
                let amount = file.read(&mut buffer).map_err(storage_io)?;
                if amount == 0 {
                    break;
                }
                digest.update(&buffer[..amount]);
            }
        }
        Location::Remote(remote) => {
            let mut stream = storage
                .backend_async(remote)
                .await?
                .read(&remote.path, Default::default())
                .await?;
            while let Some(chunk) = stream.next().await {
                digest.update(&chunk?);
            }
        }
    }
    Ok(digest.finalize().into())
}

#[cfg(feature = "tokio")]
fn storage_io(error: io::Error) -> StorageError {
    StorageError::new(ErrorKind::Other, error.to_string())
}
