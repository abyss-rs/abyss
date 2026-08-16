use std::future::Future;
use std::pin::Pin;
use std::time::UNIX_EPOCH;

use suppaftp::list::File;

use super::session::FtpSession;
use crate::storage::{EntryKind, ErrorKind, StorageEntry, StorageError};

pub(crate) fn delete_ftp_tree<'a>(
    session: &'a mut FtpSession,
    path: String,
) -> Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>> {
    Box::pin(async move {
        match session.entry(&path).await {
            Ok(entry) if entry.is_directory() => {
                for child in session.entries(&path).await? {
                    let child_path = if path == "/" || path.is_empty() {
                        format!("/{}", child.name())
                    } else {
                        format!("{path}/{}", child.name())
                    };
                    delete_ftp_tree(session, child_path).await?;
                }
                if path != "/" && !path.is_empty() {
                    session.remove_dir(&path).await?;
                }
                Ok(())
            }
            _ => session.remove_file(&path).await,
        }
    })
}

pub(crate) fn ftp_entry(file: File) -> StorageEntry {
    StorageEntry {
        name: file.name().as_bytes().to_vec(),
        kind: if file.is_directory() {
            EntryKind::Directory
        } else if file.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::File
        },
        size: (!file.is_directory()).then_some(file.size() as u64),
        modified: Some(file.modified()),
        version: Some(ftp_version(&file)),
    }
}

pub(crate) fn ftp_version(file: &File) -> String {
    format!(
        "{}:{}",
        file.size(),
        file.modified()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default()
    )
}

pub(crate) fn map_ftp_error(error: suppaftp::FtpError) -> StorageError {
    let message = error.to_string();
    let lowercase = message.to_ascii_lowercase();
    let kind = if lowercase.contains("login")
        || lowercase.contains("authentication")
        || lowercase.contains("530")
    {
        ErrorKind::Authentication
    } else if lowercase.contains("not found")
        || lowercase.contains("no such file")
        || lowercase.contains("does not exist")
        || lowercase.contains("cannot find")
    {
        ErrorKind::NotFound
    } else if lowercase.contains("permission") || lowercase.contains("denied") {
        ErrorKind::PermissionDenied
    } else if lowercase.contains("exist") {
        ErrorKind::AlreadyExists
    } else if lowercase.contains("timeout") {
        ErrorKind::Timeout
    } else if lowercase.contains("550") {
        ErrorKind::NotFound
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("FTP: {message}"))
        .retryable(matches!(kind, ErrorKind::Timeout | ErrorKind::Transport))
}

pub(crate) fn map_ftp_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::TimedOut {
        ErrorKind::Timeout
    } else {
        ErrorKind::Transport
    };
    StorageError::new(kind, format!("FTP data stream: {error}")).retryable(true)
}
