use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::inventory::{EntryKind, Inventory};
use crate::native;
use crate::progress::{CopyStats, OperationPhase};

pub(crate) fn delete_paths(
    sources: &[PathBuf],
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    stats.reset();
    let mut inventories = Vec::with_capacity(sources.len());
    let mut failures = Vec::new();
    let mut total_objects = 0_u64;
    let mut total_bytes = 0_u64;
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let inventory = match Inventory::scan_with_progress(source, cancelled, Some(stats)) {
            Ok(inventory) => inventory,
            Err(Error::Io {
                path,
                source: error,
                ..
            }) if path == *source && error.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        total_objects = total_objects.saturating_add(inventory.entries.len() as u64);
        total_bytes = total_bytes.saturating_add(inventory.logical_bytes);
        inventories.push((source.clone(), inventory));
    }
    stats.set_totals(total_objects, total_bytes);
    stats.set_phase(OperationPhase::Deleting);

    for (root, inventory) in inventories {
        failures.extend(delete_scanned_root(&root, &inventory, cancelled, stats)?);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::message(format!(
            "{} item(s) could not be deleted:\n{}",
            failures.len(),
            failures.join("\n")
        )))
    }
}

pub(crate) fn trash_paths(
    sources: &[PathBuf],
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    stats.reset();
    stats.set_totals(sources.len() as u64, 0);
    stats.set_phase(OperationPhase::Deleting);
    let mut failures = Vec::new();
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        stats.observe_transfer(source);
        match trash::delete(source) {
            Ok(()) => stats.complete_object(source),
            Err(error) => failures.push(format!("{}: {error}", source.display())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::message(format!(
            "{} item(s) could not be moved to Trash:\n{}",
            failures.len(),
            failures.join("\n")
        )))
    }
}

pub(crate) fn delete_scanned_root(
    root: &Path,
    inventory: &Inventory,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<Vec<String>, Error> {
    let mut failed = Vec::new();
    for entry in inventory.entries.iter().rev() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let path = if entry.relative.as_os_str().is_empty() {
            root.to_owned()
        } else {
            root.join(&entry.relative)
        };
        let removed = match entry.kind {
            EntryKind::Directory => remove_if_present(
                &path,
                |path| native::remove_path(path, true),
                "delete directory",
            ),
            EntryKind::File => remove_if_present(
                &path,
                |path| native::remove_path(path, false),
                "delete file",
            ),
            EntryKind::Symlink => remove_if_present(
                &path,
                |path| native::remove_path(path, false),
                "delete symbolic link",
            ),
            EntryKind::Other => remove_if_present(
                &path,
                |path| native::remove_path(path, false),
                "delete filesystem object",
            ),
        };
        match removed {
            Ok(()) => complete_deleted_entry(stats, entry.kind, entry.len, &path),
            Err(error)
                if entry.kind == EntryKind::Directory
                    && io_error_kind(&error) == Some(std::io::ErrorKind::NotFound) =>
            {
                match native::recover_unremovable_directory(&path) {
                    Ok(()) => complete_deleted_entry(stats, entry.kind, entry.len, &path),
                    Err(recovery_error) => failed.push((
                        entry.kind,
                        entry.len,
                        path.clone(),
                        Error::io("recover malformed directory entry", &path, recovery_error),
                    )),
                }
            }
            Err(error) => failed.push((entry.kind, entry.len, path, error)),
        }
    }

    if failed.is_empty() {
        return Ok(Vec::new());
    }

    // Network, FUSE, and AppleDouble-aware filesystems may materialize a metadata
    // entry after the inventory scan. Keep the exact-entry fast path above, then
    // retry the selected root recursively if it is still a real directory.
    match remove_failed_directory(root)? {
        true => {
            for (kind, len, path, _) in failed {
                complete_deleted_entry(stats, kind, len, &path);
            }
            Ok(Vec::new())
        }
        false => Ok(failed
            .into_iter()
            .map(|(_, _, _, error)| error.to_string())
            .collect()),
    }
}

fn io_error_kind(error: &Error) -> Option<std::io::ErrorKind> {
    match error {
        Error::Io { source, .. } => Some(source.kind()),
        Error::Cancelled | Error::Message(_) => None,
    }
}

fn complete_deleted_entry(stats: &CopyStats, kind: EntryKind, len: u64, path: &Path) {
    match kind {
        EntryKind::File => stats.complete_file(len, 0, false, false),
        EntryKind::Directory | EntryKind::Symlink | EntryKind::Other => stats.complete_object(path),
    }
}

fn remove_failed_directory(path: &Path) -> Result<bool, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(Error::io("inspect failed deletion", path, error)),
    };
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }
    match native::remove_directory_tree(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(Error::io(
            "native recursive directory deletion",
            path,
            error,
        )),
    }
}

fn remove_if_present(
    path: &Path,
    remove: impl FnOnce(&Path) -> std::io::Result<()>,
    action: &'static str,
) -> Result<(), Error> {
    match remove(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(path) {
                Err(inspect_error) if inspect_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(())
                }
                Ok(_) | Err(_) => Err(Error::io(action, path, error)),
            }
        }
        Err(error) => Err(Error::io(action, path, error)),
    }
}
