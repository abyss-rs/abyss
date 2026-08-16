use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::inventory::EntryKind;

pub(crate) fn resolve_target(source: &Path, destination: &Path) -> Result<PathBuf, Error> {
    match fs::metadata(destination) {
        Ok(metadata) if metadata.is_dir() => {
            let source_name = source.file_name().map(ToOwned::to_owned).or_else(|| {
                fs::canonicalize(source)
                    .ok()
                    .and_then(|path| path.file_name().map(ToOwned::to_owned))
            });
            source_name
                .map(|name| destination.join(name))
                .ok_or_else(|| Error::message("cannot derive a name for the source root"))
        }
        Ok(_) => Ok(destination.to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(destination.to_owned()),
        Err(error) => Err(Error::io("inspect destination", destination, error)),
    }
}

pub(crate) fn validate_target(
    source: &Path,
    source_metadata: &fs::Metadata,
    target: &Path,
) -> Result<(), Error> {
    let parent = usable_parent(target);
    fs::metadata(parent).map_err(|error| Error::io("inspect destination parent", parent, error))?;

    if let Ok(target_metadata) = fs::symlink_metadata(target)
        && same_file(source, source_metadata, target, &target_metadata)?
    {
        return Err(Error::message(format!(
            "source and destination are the same filesystem object: {}",
            target.display()
        )));
    }

    if source_metadata.is_dir() {
        let canonical_source =
            fs::canonicalize(source).map_err(|error| Error::io("resolve source", source, error))?;
        let canonical_target = canonicalize_target(target)?;
        if canonical_target.starts_with(&canonical_source) {
            return Err(Error::message(format!(
                "destination cannot be inside the source: {}",
                target.display()
            )));
        }
    }

    Ok(())
}

#[cfg(unix)]
pub(crate) fn same_file(
    _left_path: &Path,
    left: &fs::Metadata,
    _right_path: &Path,
    right: &fs::Metadata,
) -> Result<bool, Error> {
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(windows)]
pub(crate) fn same_file(
    left_path: &Path,
    _left: &fs::Metadata,
    right_path: &Path,
    _right: &fs::Metadata,
) -> Result<bool, Error> {
    let left = crate::native::path_identity(left_path)
        .map_err(|error| Error::io("identify", left_path, error))?;
    let right = crate::native::path_identity(right_path)
        .map_err(|error| Error::io("identify", right_path, error))?;
    Ok(left.volume == right.volume && left.index == right.index)
}

pub(crate) fn canonicalize_target(target: &Path) -> Result<PathBuf, Error> {
    match fs::canonicalize(target) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = usable_parent(target);
            let canonical_parent = fs::canonicalize(parent)
                .map_err(|error| Error::io("resolve destination parent", parent, error))?;
            let name = target.file_name().ok_or_else(|| {
                Error::message(format!(
                    "destination has no file name: {}",
                    target.display()
                ))
            })?;
            Ok(canonical_parent.join(name))
        }
        Err(error) => Err(Error::io("resolve destination", target, error)),
    }
}

pub(crate) fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub(crate) fn ensure_compatible_destination(
    path: &Path,
    source_kind: EntryKind,
) -> Result<(), Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::io("inspect existing destination", path, error)),
    };
    let destination_is_directory = metadata.file_type().is_dir();

    if source_kind == EntryKind::Directory && !destination_is_directory {
        return Err(type_conflict(path, "directory", "non-directory"));
    }
    if source_kind != EntryKind::Directory && destination_is_directory {
        return Err(type_conflict(path, "non-directory", "directory"));
    }
    Ok(())
}

pub(crate) fn type_conflict(path: &Path, source: &str, destination: &str) -> Error {
    Error::message(format!(
        "type conflict at {}: source is a {source}, destination is a {destination}",
        path.display()
    ))
}
