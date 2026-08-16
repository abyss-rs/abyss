use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::native;
use crate::storage::{EntryKind, ErrorKind, Location, StorageError, StorageRuntime};
pub(crate) async fn list_all(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<Vec<crate::storage::StorageEntry>, StorageError> {
    match location {
        Location::Local(path) => {
            let mut entries = Vec::new();
            for item in std::fs::read_dir(path).map_err(io_storage_error)? {
                let item = item.map_err(io_storage_error)?;
                let metadata = item.metadata().map_err(io_storage_error)?;
                entries.push(crate::storage::StorageEntry {
                    name: item.file_name().as_encoded_bytes().to_vec(),
                    kind: if metadata.is_dir() {
                        EntryKind::Directory
                    } else if metadata.is_file() {
                        EntryKind::File
                    } else {
                        EntryKind::Other
                    },
                    size: Some(metadata.len()),
                    modified: metadata.modified().ok(),
                    version: None,
                });
            }
            entries.retain(|entry| !entry.name.starts_with(b"._"));
            Ok(entries)
        }
        Location::Remote(remote) => {
            let backend = storage.backend_async(remote).await?;
            let mut continuation = None;
            let mut entries = std::collections::HashMap::new();
            loop {
                let page = backend.list(&remote.path, continuation.as_deref()).await?;
                for entry in page.entries {
                    entries
                        .entry(entry.name.clone())
                        .and_modify(|current: &mut crate::storage::StorageEntry| {
                            if entry.kind == EntryKind::Directory {
                                *current = entry.clone();
                            }
                        })
                        .or_insert(entry);
                }
                continuation = page.continuation;
                if continuation.is_none() {
                    break;
                }
            }
            Ok(entries
                .into_values()
                .filter(|entry| !entry.name.starts_with(b"._"))
                .collect())
        }
    }
}

pub(crate) async fn stat_location(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<crate::storage::StorageEntry, StorageError> {
    match location {
        Location::Local(path) => {
            let metadata = std::fs::metadata(path).map_err(io_storage_error)?;
            Ok(crate::storage::StorageEntry {
                name: path
                    .file_name()
                    .map(|value| value.as_encoded_bytes().to_vec())
                    .unwrap_or_default(),
                kind: if metadata.is_dir() {
                    EntryKind::Directory
                } else if metadata.is_file() {
                    EntryKind::File
                } else {
                    EntryKind::Other
                },
                size: Some(metadata.len()),
                modified: metadata.modified().ok(),
                version: None,
            })
        }
        Location::Remote(remote) => {
            let backend = storage.backend_async(remote).await?;
            match backend.stat(&remote.path).await {
                Ok(entry) => Ok(entry),
                Err(error)
                    if matches!(error.kind, ErrorKind::NotFound | ErrorKind::InvalidInput) =>
                {
                    match backend.list(&remote.path, None).await {
                        Ok(page) if !page.entries.is_empty() => Ok(crate::storage::StorageEntry {
                            name: remote.path.file_name().unwrap_or_default(),
                            kind: EntryKind::Directory,
                            size: None,
                            modified: None,
                            version: None,
                        }),
                        Ok(_) => Err(error),
                        Err(list_error) => Err(list_error),
                    }
                }
                Err(error) => Err(error),
            }
        }
    }
}

pub(crate) async fn location_kind(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<EntryKind, StorageError> {
    match stat_location(storage, location).await {
        Ok(entry) => Ok(entry.kind),
        Err(error)
            if matches!(location, Location::Remote(_))
                && matches!(error.kind, ErrorKind::NotFound | ErrorKind::InvalidInput) =>
        {
            match list_all(storage, location).await {
                Ok(_) => Ok(EntryKind::Directory),
                Err(list_error) if list_error.kind == ErrorKind::NotFound => Ok(EntryKind::File),
                Err(list_error) => Err(list_error),
            }
        }
        Err(error) => Err(error),
    }
}

pub(crate) async fn create_directory(
    storage: &StorageRuntime,
    location: &Location,
) -> Result<(), StorageError> {
    match location {
        Location::Local(path) => std::fs::create_dir_all(path).map_err(io_storage_error),
        Location::Remote(remote) => match stat_location(storage, location).await {
            Ok(entry) if entry.kind == EntryKind::Directory => Ok(()),
            Ok(_) => Err(StorageError::new(
                ErrorKind::Conflict,
                format!("directory conflicts with file: {}", location.display()),
            )),
            Err(error) if error.kind == ErrorKind::NotFound => {
                storage
                    .backend_async(remote)
                    .await?
                    .create_dir(&remote.path)
                    .await
            }
            Err(error) => Err(error),
        },
    }
}

pub(crate) async fn delete_location(
    storage: &StorageRuntime,
    location: &Location,
    recursive: bool,
) -> Result<(), StorageError> {
    match location {
        Location::Local(path) => {
            let metadata = std::fs::symlink_metadata(path).map_err(io_storage_error)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() && recursive {
                native::remove_directory_tree(path).map_err(io_storage_error)
            } else {
                native::remove_path(path, metadata.is_dir()).map_err(io_storage_error)
            }
        }
        Location::Remote(remote) => {
            storage
                .backend_async(remote)
                .await?
                .delete(&remote.path, recursive)
                .await
        }
    }
}

pub(crate) fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), StorageError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(StorageError::new(
            ErrorKind::Cancelled,
            "operation cancelled",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn io_storage_error(error: std::io::Error) -> StorageError {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
        _ => ErrorKind::Other,
    };
    StorageError::new(kind, error.to_string())
}

pub(crate) fn storage_error(error: StorageError) -> Error {
    if error.kind == ErrorKind::Cancelled {
        Error::Cancelled
    } else {
        Error::message(error.to_string())
    }
}
