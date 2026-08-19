use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::copy::ConflictResolver;
use crate::progress::{CopyStats, OperationPhase};
use crate::storage::{Location, StorageRuntime};
use crate::sync::plan::{SyncPlan, SyncReason, SyncStrategy};

/// Executes a complete `SyncPlan` with progress tracking and cancellation support.
pub fn execute_sync(
    storage: Arc<StorageRuntime>,
    plan: SyncPlan,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    resolver: &dyn ConflictResolver,
) -> Result<(), Error> {
    stats.reset();
    let total_items = (plan.directories.len() + plan.files.len() + plan.deletions.len()) as u64;
    stats.set_totals(total_items, plan.bytes);
    stats.set_phase(OperationPhase::Copying);

    // 1. Create target directories
    for directory in &plan.directories {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match directory {
            Location::Local(path) => {
                fs::create_dir_all(path)
                    .map_err(|e| Error::io("create sync directory", path, e))?;
            }
            #[cfg(feature = "tokio")]
            Location::Remote(remote) => {
                storage.block_on(async {
                    let backend = storage.backend_async(remote).await?;
                    if backend.capabilities().create_dir {
                        backend.create_dir(&remote.path).await?;
                    }
                    Ok::<(), Error>(())
                })?;
            }
            #[cfg(not(feature = "tokio"))]
            Location::Remote(_) => {
                return Err(Error::message("remote storage is disabled in this build"));
            }
        }
    }

    // 2. Perform deletions (e.g. for Mirror mode)
    if !plan.deletions.is_empty() {
        stats.set_phase(OperationPhase::Deleting);
        for deletion in &plan.deletions {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            match deletion {
                Location::Local(path) => {
                    if path.is_dir() {
                        let _ = fs::remove_dir_all(path);
                    } else {
                        let _ = fs::remove_file(path);
                    }
                }
                #[cfg(feature = "tokio")]
                Location::Remote(remote) => {
                    let _ = storage.block_on(async {
                        let backend = storage.backend_async(remote).await?;
                        backend.delete(&remote.path, true).await?;
                        Ok::<(), Error>(())
                    });
                }
                #[cfg(not(feature = "tokio"))]
                Location::Remote(_) => {}
            }
            stats.complete_file(0, 0, false, false);
        }
    }

    // 3. Sync files
    stats.set_phase(OperationPhase::Copying);
    for file in &plan.files {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }

        match (&file.source, &file.destination) {
            (Location::Local(src), Location::Local(dst)) => {
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let Ok(src_metadata) = fs::symlink_metadata(src) else {
                    continue;
                };

                let file_size = if src_metadata.is_file() {
                    src_metadata.len()
                } else {
                    0
                };
                stats.begin_file(dst, file_size);

                // Replicate symlink if source is a symlink
                if src_metadata.file_type().is_symlink()
                    && let Ok(target) = fs::read_link(src)
                {
                    let _ = fs::remove_file(dst);
                    let _ = fs::remove_dir_all(dst);
                    #[cfg(unix)]
                    let res = std::os::unix::fs::symlink(&target, dst);
                    #[cfg(windows)]
                    let res = if target.is_dir() {
                        std::os::windows::fs::symlink_dir(&target, dst)
                    } else {
                        std::os::windows::fs::symlink_file(&target, dst)
                    };
                    if res.is_ok() {
                        stats.complete_file(0, 0, false, true);
                        continue;
                    }
                }

                // If destination exists as conflicting type (symlink or dir), clear it first
                if dst.is_dir() {
                    let _ = fs::remove_dir_all(dst);
                } else if dst.is_symlink() {
                    let _ = fs::remove_file(dst);
                }

                // Delta sync optimization if requested
                let mut delta_done = false;
                if plan.strategy == SyncStrategy::DeltaRsync
                    && file.reason == SyncReason::DeltaPatchable
                    && dst.exists()
                    && let (Ok(base_data), Ok(target_data)) = (fs::read(dst), fs::read(src))
                {
                    let signature = crate::sync::delta::compute_signature(
                        &base_data,
                        crate::sync::delta::DEFAULT_BLOCK_SIZE,
                    );
                    let delta_bytes = crate::sync::delta::compute_delta(&signature, &target_data);
                    if let Ok(reconstructed) =
                        crate::sync::delta::apply_delta(&base_data, &delta_bytes)
                        && let Ok(()) = fs::write(dst, &reconstructed)
                    {
                        stats.complete_file(file_size, delta_bytes.len() as u64, false, false);
                        delta_done = true;
                    }
                }

                if !delta_done {
                    match fs::copy(src, dst) {
                        Ok(bytes_copied) => {
                            stats.complete_file(file_size, bytes_copied, false, false);
                        }
                        Err(_) => {
                            let _ = fs::remove_file(dst);
                            if let Ok(bytes_copied) = fs::copy(src, dst) {
                                stats.complete_file(file_size, bytes_copied, false, false);
                            } else {
                                stats.complete_file(file_size, 0, false, false);
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "tokio")]
            _ => {
                let size = match &file.source {
                    Location::Local(p) => fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                    Location::Remote(_) => 0,
                };
                storage.block_on(async {
                    crate::remote_operation::copy_file(
                        &storage,
                        &file.source,
                        &file.destination,
                        size,
                        &cancelled,
                        &stats,
                        resolver,
                    )
                    .await
                    .map_err(Error::from)
                })?;
            }
            #[cfg(not(feature = "tokio"))]
            _ => {
                return Err(Error::message("remote storage is disabled in this build"));
            }
        }
    }

    Ok(())
}
