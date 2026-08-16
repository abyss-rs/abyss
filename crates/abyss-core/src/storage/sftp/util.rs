use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use ssh2::{FileStat, RenameFlags, Sftp};

use crate::storage::{EntryKind, ErrorKind, StorageEntry, StorageError};

pub(crate) fn sftp_rename(
    sftp: &Sftp,
    source: &Path,
    destination: &Path,
    overwrite: bool,
) -> Result<(), StorageError> {
    let mut flags = RenameFlags::ATOMIC | RenameFlags::NATIVE;
    if overwrite {
        flags |= RenameFlags::OVERWRITE;
    }
    if sftp.rename(source, destination, Some(flags)).is_ok() {
        return Ok(());
    }
    if overwrite {
        let _ = sftp.unlink(destination);
    }
    sftp.rename(source, destination, None)
        .map_err(map_ssh_error)
}

pub(crate) fn mkdir_p(sftp: &Sftp, path: &Path) -> Result<(), StorageError> {
    if sftp.stat(path).is_ok() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty() && *parent != Path::new("/"))
    {
        let _ = mkdir_p(sftp, parent);
    }
    match sftp.mkdir(path, 0o755) {
        Ok(_) => Ok(()),
        Err(_) if sftp.stat(path).is_ok() => Ok(()),
        Err(error) => Err(map_ssh_error(error)),
    }
}

pub(crate) fn delete_sftp_path(
    sftp: &Sftp,
    path: &Path,
    recursive: bool,
) -> Result<(), StorageError> {
    let stat = sftp.stat(path).map_err(map_ssh_error)?;
    if stat.is_dir() {
        if recursive {
            for (child, _) in sftp.readdir(path).map_err(map_ssh_error)? {
                let Some(name) = child.file_name() else {
                    continue;
                };
                if matches!(name.as_encoded_bytes(), b"." | b"..") {
                    continue;
                }
                delete_sftp_path(sftp, &child, true)?;
            }
        }
        sftp.rmdir(path).map_err(map_ssh_error)
    } else {
        sftp.unlink(path).map_err(map_ssh_error)
    }
}

pub(crate) fn storage_entry(name: Vec<u8>, stat: &FileStat) -> StorageEntry {
    StorageEntry {
        name,
        kind: if stat.is_dir() {
            EntryKind::Directory
        } else if stat.is_file() {
            EntryKind::File
        } else if stat.file_type().is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        },
        size: stat.size,
        modified: stat
            .mtime
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
        version: Some(stat_version(stat)),
    }
}

pub(crate) fn stat_version(stat: &FileStat) -> String {
    format!(
        "{}:{}",
        stat.size.unwrap_or_default(),
        stat.mtime.unwrap_or_default()
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

pub(crate) fn map_ssh_error(error: ssh2::Error) -> StorageError {
    let message = error.message().to_ascii_lowercase();
    let kind = if message.contains("authentication") || message.contains("publickey") {
        ErrorKind::Authentication
    } else if message.contains("permission") {
        ErrorKind::PermissionDenied
    } else if message.contains("not found") || message.contains("no such") {
        ErrorKind::NotFound
    } else if message.contains("exist") {
        ErrorKind::AlreadyExists
    } else if message.contains("timeout") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("SFTP: {}", error.message()))
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

pub(crate) fn join_error(error: tokio::task::JoinError) -> StorageError {
    StorageError::new(ErrorKind::Other, format!("SFTP worker failed: {error}"))
}
