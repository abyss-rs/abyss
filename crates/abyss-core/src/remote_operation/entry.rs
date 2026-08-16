use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::copy::ConflictResolver;
use crate::progress::{CopyStats, OperationPhase};
use crate::remote_operation::bulk::tree::descend;
use crate::remote_operation::bulk::try_bulk_copy_tree;
use crate::remote_operation::file::copy_file;
use crate::remote_operation::locations::*;
use crate::storage::{EntryKind, ErrorKind, Location, StorageError, StorageRuntime};
pub fn transfer(
    storage: Arc<StorageRuntime>,
    sources: Vec<Location>,
    destination: Location,
    move_sources: bool,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    storage.block_on(async {
        stats.reset();
        stats.set_phase(if move_sources {
            OperationPhase::Moving
        } else {
            OperationPhase::Copying
        });
        let destination_is_directory =
            location_kind(&storage, &destination).await? == EntryKind::Directory;
        let mut totals = (0_u64, 0_u64);
        for source in &sources {
            scan(&storage, source, &cancelled, &stats, &mut totals).await?;
        }
        stats.set_totals(totals.0, totals.1);
        for source in &sources {
            ensure_not_cancelled(&cancelled)?;
            let target = if sources.len() > 1 || destination_is_directory {
                let name = source.file_name().ok_or_else(|| {
                    StorageError::new(ErrorKind::InvalidInput, "source has no file name")
                })?;
                destination.child_transfer(&name)?
            } else {
                destination.clone()
            };
            copy_tree(&storage, source, &target, &cancelled, &stats, conflicts)
            .await?;
        }
        if move_sources {
            if stats.skipped_objects.load(Ordering::Relaxed) > 0 {
                return Err(StorageError::new(
                    ErrorKind::Conflict,
                    "move copied some items but kept every source because conflicts were skipped",
                ));
            }
            stats.set_phase(OperationPhase::Moving);
            for source in &sources {
                ensure_not_cancelled(&cancelled)?;
                delete_location(&storage, source, true).await?;
            }
        }
        Ok::<_, StorageError>(())
    })
    .map_err(storage_error)
}

pub fn delete(
    storage: Arc<StorageRuntime>,
    sources: Vec<Location>,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
) -> Result<(), Error> {
    storage
        .block_on(async {
            stats.reset();
            stats.set_phase(OperationPhase::Deleting);
            stats.set_totals(sources.len() as u64, 0);
            for source in &sources {
                ensure_not_cancelled(&cancelled)?;
                delete_location(&storage, source, true).await?;
                stats.complete_object(Path::new(&source.display()));
            }
            Ok::<_, StorageError>(())
        })
        .map_err(storage_error)
}

pub(crate) async fn scan(
    storage: &StorageRuntime,
    location: &Location,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    totals: &mut (u64, u64),
) -> Result<(), StorageError> {
    ensure_not_cancelled(cancelled)?;
    let root = stat_location(storage, location).await?;
    totals.0 = totals.0.saturating_add(1);
    totals.1 = totals.1.saturating_add(root.size.unwrap_or(0));
    stats.observe_scan(Path::new(&location.display()));
    if root.kind != EntryKind::Directory {
        return Ok(());
    }
    if let Location::Remote(remote) = location {
        let backend = storage.backend_async(remote).await?;
        if backend.capabilities().bulk_tree_read {
            match backend.list_tree(&remote.path).await {
                Ok(entries) => {
                    for entry in entries {
                        ensure_not_cancelled(cancelled)?;
                        totals.0 = totals.0.saturating_add(1);
                        totals.1 = totals.1.saturating_add(entry.size);
                        let display = descend(location, &entry.path)?.display();
                        stats.observe_scan(Path::new(&display));
                    }
                    return Ok(());
                }
                Err(error) if error.kind == ErrorKind::Unsupported => {}
                Err(error) => return Err(error),
            }
        }
    }
    let mut stack = list_all(storage, location)
        .await?
        .into_iter()
        .filter_map(|entry| location.child_transfer(&entry.name).ok())
        .collect::<Vec<_>>();
    while let Some(current) = stack.pop() {
        ensure_not_cancelled(cancelled)?;
        let entry = stat_location(storage, &current).await?;
        totals.0 = totals.0.saturating_add(1);
        totals.1 = totals.1.saturating_add(entry.size.unwrap_or(0));
        stats.observe_scan(Path::new(&current.display()));
        if entry.kind == EntryKind::Directory {
            stack.extend(
                list_all(storage, &current)
                    .await?
                    .into_iter()
                    .filter_map(|entry| current.child_transfer(&entry.name).ok()),
            );
        }
    }
    Ok(())
}

pub(crate) async fn copy_tree(
    storage: &Arc<StorageRuntime>,
    source: &Location,
    destination: &Location,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), StorageError> {
    let source_entry = stat_location(storage, source).await?;
    if source_entry.kind == EntryKind::Directory {
        if try_bulk_copy_tree(
            Arc::clone(storage),
            source,
            destination,
            cancelled,
            stats,
            conflicts,
        )
        .await?
        {
            return Ok(());
        }
        create_directory(storage, destination).await?;
        stats.complete_object(Path::new(&destination.display()));
        let mut stack = vec![(source.clone(), destination.clone())];
        while let Some((source_dir, destination_dir)) = stack.pop() {
            for entry in list_all(storage, &source_dir).await? {
                ensure_not_cancelled(cancelled)?;
                let source_child = source_dir.child_transfer(&entry.name)?;
                let destination_child = destination_dir.child_transfer(&entry.name)?;
                if entry.kind == EntryKind::Directory {
                    create_directory(storage, &destination_child).await?;
                    stats.complete_object(Path::new(&destination_child.display()));
                    stack.push((source_child, destination_child));
                } else {
                    copy_file(
                        storage,
                        &source_child,
                        &destination_child,
                        entry.size.unwrap_or(0),
                        cancelled,
                        stats,
                        conflicts,
                    )
                    .await?;
                }
            }
        }
    } else {
        copy_file(
            storage,
            source,
            destination,
            source_entry.size.unwrap_or(0),
            cancelled,
            stats,
            conflicts,
        )
        .await?;
    }
    Ok(())
}
