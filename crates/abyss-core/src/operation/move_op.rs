use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::Error;
use crate::copy::ConflictResolver;
use crate::native;
use crate::operation::copy::copy_paths;
use crate::progress::{CopyStats, OperationPhase};

pub(crate) fn move_paths(
    sources: &[PathBuf],
    destination: &Path,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    if let Some(targets) = rename_targets(sources, destination)? {
        stats.reset();
        stats.set_phase(OperationPhase::Moving);
        stats.set_totals(sources.len() as u64, 0);
        for (source, target) in sources.iter().zip(targets) {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            native::move_path(source, &target)
                .map_err(|error| Error::io("move to", &target, error))?;
            stats.complete_object(&target);
        }
        return Ok(());
    }

    copy_paths(
        sources,
        destination,
        Arc::clone(&cancelled),
        Arc::clone(&stats),
        conflicts,
    )?;
    if stats.skipped_objects.load(Ordering::Relaxed) > 0 {
        return Err(Error::message(
            "move copied some items but kept every source because conflicts were skipped",
        ));
    }

    stats.set_phase(OperationPhase::Moving);
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        remove_source(source)?;
    }
    Ok(())
}

fn rename_targets(sources: &[PathBuf], destination: &Path) -> Result<Option<Vec<PathBuf>>, Error> {
    let destination_is_directory = match fs::metadata(destination) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(Error::io("inspect move destination", destination, error)),
    };
    if sources.len() > 1 && !destination_is_directory {
        return Ok(None);
    }

    let mut targets = Vec::with_capacity(sources.len());
    for source in sources {
        let source_metadata =
            fs::symlink_metadata(source).map_err(|error| Error::io("inspect", source, error))?;
        let Some(name) = source.file_name() else {
            return Ok(None);
        };
        let target = if destination_is_directory {
            destination.join(name)
        } else {
            destination.to_owned()
        };
        match fs::symlink_metadata(&target) {
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io("inspect move target", &target, error)),
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::message("move destination has no parent directory"))?;
        let parent_metadata = match fs::metadata(parent) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Error::io(
                    "inspect move destination directory",
                    parent,
                    error,
                ));
            }
        };
        if !parent_metadata.is_dir()
            || !same_volume(source, &source_metadata, parent, &parent_metadata)?
        {
            return Ok(None);
        }
        targets.push(target);
    }
    Ok(Some(targets))
}

#[cfg(unix)]
fn same_volume(
    _left_path: &Path,
    left: &fs::Metadata,
    _right_path: &Path,
    right: &fs::Metadata,
) -> Result<bool, Error> {
    Ok(left.dev() == right.dev())
}

#[cfg(windows)]
fn same_volume(
    left_path: &Path,
    _left: &fs::Metadata,
    right_path: &Path,
    _right: &fs::Metadata,
) -> Result<bool, Error> {
    let left = native::path_identity(left_path)
        .map_err(|error| Error::io("identify", left_path, error))?;
    let right = native::path_identity(right_path)
        .map_err(|error| Error::io("identify", right_path, error))?;
    Ok(left.volume == right.volume)
}

fn remove_source(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Error::io("inspect moved source", path, error))?;
    if metadata.is_dir() {
        native::remove_directory_tree(path)
            .map_err(|error| Error::io("remove moved directory", path, error))
    } else {
        native::remove_path(path, false)
            .map_err(|error| Error::io("remove moved item", path, error))
    }
}
