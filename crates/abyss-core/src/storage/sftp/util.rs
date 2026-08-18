use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use russh_sftp::protocol::FileAttributes;

use crate::storage::{EntryKind, ErrorKind, StorageEntry, StorageError};

pub(crate) fn storage_entry(name: Vec<u8>, attrs: &FileAttributes) -> StorageEntry {
    let is_dir = attrs.is_dir();
    let is_file = attrs.is_regular();
    let is_symlink = attrs.is_symlink();

    StorageEntry {
        name,
        kind: if is_dir {
            EntryKind::Directory
        } else if is_file {
            EntryKind::File
        } else if is_symlink {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        },
        size: attrs.size,
        modified: attrs
            .mtime
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds as u64)),
        version: Some(stat_version(attrs)),
    }
}

pub(crate) fn stat_version(attrs: &FileAttributes) -> String {
    format!(
        "{}:{}",
        attrs.size.unwrap_or_default(),
        attrs.mtime.unwrap_or_default()
    )
}

pub(crate) fn temporary_remote_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    destination.with_file_name(format!(".{name}.abyss-{}.part", uuid::Uuid::new_v4()))
}

pub(crate) fn default_known_hosts() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|directories| directories.home_dir().join(".ssh/known_hosts"))
}

pub(crate) fn map_russh_error(error: russh::Error) -> StorageError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let kind = if lower.contains("auth")
        || lower.contains("publickey")
        || lower.contains("denied")
        || lower.contains("server key")
        || lower.contains("key mismatch")
    {
        ErrorKind::Authentication
    } else if lower.contains("permission") {
        ErrorKind::PermissionDenied
    } else if lower.contains("not found") || lower.contains("no such") {
        ErrorKind::NotFound
    } else if lower.contains("exist") {
        ErrorKind::AlreadyExists
    } else if lower.contains("timeout") || lower.contains("timed out") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("SFTP SSH: {message}"))
        .retryable(matches!(kind, ErrorKind::Timeout | ErrorKind::Transport))
}

pub(crate) fn map_sftp_error(error: russh_sftp::client::error::Error) -> StorageError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let kind = if lower.contains("not found") || lower.contains("no such") {
        ErrorKind::NotFound
    } else if lower.contains("permission") || lower.contains("denied") {
        ErrorKind::PermissionDenied
    } else if lower.contains("already exists") || lower.contains("exist") {
        ErrorKind::AlreadyExists
    } else if lower.contains("timeout") || lower.contains("timed out") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("SFTP: {message}"))
        .retryable(matches!(kind, ErrorKind::Timeout | ErrorKind::Transport))
}

pub(crate) fn map_sftp_io(error: std::io::Error) -> StorageError {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
        std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
        _ => ErrorKind::Transport,
    };
    StorageError::new(kind, format!("SFTP I/O: {error}"))
        .retryable(matches!(kind, ErrorKind::Timeout | ErrorKind::Transport))
}
