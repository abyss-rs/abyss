use std::io::Cursor;
use std::time::{Duration, UNIX_EPOCH};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;

use crate::storage::helper_protocol::{HelperEntry, HelperEntryKind, HelperTreeEntry};
use crate::storage::{EntryKind, ErrorKind, StorageEntry, StorageError, TreeEntry, TreeWriteEntry};

pub(crate) fn encode_frame<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, StorageError> {
    let mut body = Vec::new();
    ciborium::into_writer(value, &mut body)
        .map_err(|error| StorageError::new(ErrorKind::Other, error.to_string()))?;
    let length = u32::try_from(body.len())
        .map_err(|_| StorageError::new(ErrorKind::InvalidInput, "helper frame is too large"))?;
    let mut frame = length.to_be_bytes().to_vec();
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub(crate) fn decode_frame<T: serde::de::DeserializeOwned>(
    data: &[u8],
) -> Result<(T, usize), StorageError> {
    if data.len() < 4 {
        return Err(StorageError::new(
            ErrorKind::Transport,
            "truncated helper response",
        ));
    }
    let length = u32::from_be_bytes(data[..4].try_into().expect("four bytes")) as usize;
    if data.len() < 4 + length {
        return Err(StorageError::new(
            ErrorKind::Transport,
            "truncated helper response body",
        ));
    }
    let value = ciborium::from_reader(Cursor::new(&data[4..4 + length]))
        .map_err(|error| StorageError::new(ErrorKind::Transport, error.to_string()))?;
    Ok((value, 4 + length))
}

pub(crate) fn helper_error_kind(kind: &str) -> ErrorKind {
    match kind {
        "entity not found" => ErrorKind::NotFound,
        "permission denied" => ErrorKind::PermissionDenied,
        "entity already exists" => ErrorKind::AlreadyExists,
        "invalid input parameter" => ErrorKind::InvalidInput,
        _ => ErrorKind::Other,
    }
}

pub(crate) fn ensure_exec_succeeded(
    status: Option<Status>,
    stderr: &[u8],
) -> Result<(), StorageError> {
    let Some(status) = status else {
        return Ok(());
    };
    if status.status.as_deref() != Some("Failure") {
        return Ok(());
    }
    let detail = status
        .message
        .or(status.reason)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from_utf8_lossy(stderr).into_owned());
    Err(StorageError::new(
        ErrorKind::Transport,
        format!("Kubernetes helper process failed: {detail}"),
    ))
}

pub(crate) fn map_kube_error(error: impl std::fmt::Display) -> StorageError {
    let message = error.to_string();
    let lowercase = message.to_ascii_lowercase();
    let kind = if lowercase.contains("not found") || lowercase.contains("404") {
        ErrorKind::NotFound
    } else if lowercase.contains("unauthorized") || lowercase.contains("401") {
        ErrorKind::Authentication
    } else if lowercase.contains("forbidden") || lowercase.contains("403") {
        ErrorKind::PermissionDenied
    } else if lowercase.contains("already exists") || lowercase.contains("409") {
        ErrorKind::AlreadyExists
    } else if lowercase.contains("timeout") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("Kubernetes: {message}"))
        .retryable(matches!(kind, ErrorKind::Timeout | ErrorKind::Transport))
}

pub(crate) fn storage_entry(entry: HelperEntry) -> StorageEntry {
    StorageEntry {
        name: entry.name,
        kind: match entry.kind {
            HelperEntryKind::Directory => EntryKind::Directory,
            HelperEntryKind::File => EntryKind::File,
            HelperEntryKind::Symlink => EntryKind::Symlink,
            HelperEntryKind::Other => EntryKind::Other,
        },
        size: entry.size,
        modified: entry
            .modified_seconds
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
        version: None,
    }
}

pub(crate) fn helper_entry_kind(kind: EntryKind) -> HelperEntryKind {
    match kind {
        EntryKind::Directory => HelperEntryKind::Directory,
        EntryKind::File => HelperEntryKind::File,
        EntryKind::Symlink => HelperEntryKind::Symlink,
        EntryKind::Other => HelperEntryKind::Other,
    }
}

pub(crate) fn entry_kind_from_helper(kind: HelperEntryKind) -> EntryKind {
    match kind {
        HelperEntryKind::Directory => EntryKind::Directory,
        HelperEntryKind::File => EntryKind::File,
        HelperEntryKind::Symlink => EntryKind::Symlink,
        HelperEntryKind::Other => EntryKind::Other,
    }
}

pub(crate) fn helper_tree_entry(entry: &TreeEntry) -> HelperTreeEntry {
    HelperTreeEntry {
        path: entry.path.clone(),
        kind: helper_entry_kind(entry.kind),
        size: entry.size,
        overwrite: false,
        clone_from: None,
    }
}

pub(crate) fn helper_tree_write_entry(entry: &TreeWriteEntry) -> HelperTreeEntry {
    let mut value = helper_tree_entry(&entry.entry);
    value.overwrite = entry.overwrite;
    value.clone_from = entry.clone_from.clone();
    value
}

pub(crate) fn tree_entry_from_helper(entry: HelperTreeEntry) -> TreeEntry {
    TreeEntry {
        path: entry.path,
        kind: entry_kind_from_helper(entry.kind),
        size: entry.size,
    }
}
