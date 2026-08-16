use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};
use suppaftp::list::{File, ListParser};
use suppaftp::tokio::{AsyncFtpStream, AsyncRustlsConnector, AsyncRustlsFtpStream};
use tokio::io::AsyncReadExt;

use super::entry::{map_ftp_error, map_ftp_io};
use crate::storage::{ErrorKind, StorageError};

const FTP_CHUNK: usize = 256 * 1024;

pub(crate) enum FtpSession {
    Plain(AsyncFtpStream),
    Secure(AsyncRustlsFtpStream),
}

impl FtpSession {
    pub(crate) async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(), StorageError> {
        match self {
            Self::Plain(session) => {
                session
                    .login(username, password)
                    .await
                    .map_err(map_ftp_error)?;
                session
                    .transfer_type(suppaftp::types::FileType::Binary)
                    .await
            }
            Self::Secure(session) => {
                session
                    .login(username, password)
                    .await
                    .map_err(map_ftp_error)?;
                session
                    .transfer_type(suppaftp::types::FileType::Binary)
                    .await
            }
        }
        .map_err(map_ftp_error)
    }

    pub(crate) async fn entries(&mut self, path: &str) -> Result<Vec<File>, StorageError> {
        let mlsd = match self {
            Self::Plain(session) => session.mlsd(Some(path)).await,
            Self::Secure(session) => session.mlsd(Some(path)).await,
        };
        if let Ok(lines) = mlsd {
            return Ok(lines
                .iter()
                .filter_map(|line| ListParser::parse_mlsd(line).ok())
                .filter(|file| !matches!(file.name(), "." | ".."))
                .collect());
        }
        let lines = match self {
            Self::Plain(session) => session.list(Some(path)).await,
            Self::Secure(session) => session.list(Some(path)).await,
        }
        .map_err(map_ftp_error)?;
        Ok(lines
            .iter()
            .filter_map(|line| line.parse::<File>().ok())
            .filter(|file| !matches!(file.name(), "." | ".."))
            .collect())
    }

    pub(crate) async fn entry(&mut self, path: &str) -> Result<File, StorageError> {
        let line = match self {
            Self::Plain(session) => session.mlst(Some(path)).await,
            Self::Secure(session) => session.mlst(Some(path)).await,
        };
        if let Ok(line) = line
            && let Ok(file) = ListParser::parse_mlst(line.trim())
        {
            return Ok(file);
        }
        let std_path = std::path::Path::new(path);
        let target_name = std_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        if let Some(file) = self
            .entries(path)
            .await
            .ok()
            .and_then(|entries| entries.into_iter().find(|f| f.name() == target_name))
        {
            return Ok(file);
        }

        let parent = std_path.parent().and_then(|p| p.to_str()).unwrap_or("");
        let parent_path = if parent.is_empty() { "/" } else { parent };
        let entries = match self.entries(parent_path).await {
            Ok(entries) => entries,
            Err(err) if err.kind == ErrorKind::NotFound => {
                return Err(StorageError::new(
                    ErrorKind::NotFound,
                    format!("FTP entry not found: {path}"),
                ));
            }
            Err(err) => return Err(err),
        };
        entries
            .into_iter()
            .find(|f| f.name() == target_name)
            .ok_or_else(|| {
                StorageError::new(ErrorKind::NotFound, format!("FTP entry not found: {path}"))
            })
    }

    pub(crate) async fn mkdir(&mut self, path: &str) -> Result<(), StorageError> {
        match self {
            Self::Plain(session) => session.mkdir(path).await,
            Self::Secure(session) => session.mkdir(path).await,
        }
        .map_err(map_ftp_error)
    }

    pub(crate) async fn remove_file(&mut self, path: &str) -> Result<(), StorageError> {
        match self {
            Self::Plain(session) => session.rm(path).await,
            Self::Secure(session) => session.rm(path).await,
        }
        .map_err(map_ftp_error)
    }

    pub(crate) async fn remove_dir(&mut self, path: &str) -> Result<(), StorageError> {
        match self {
            Self::Plain(session) => session.rmdir(path).await,
            Self::Secure(session) => session.rmdir(path).await,
        }
        .map_err(map_ftp_error)
    }

    pub(crate) async fn rename(&mut self, from: &str, to: &str) -> Result<(), StorageError> {
        match self {
            Self::Plain(session) => session.rename(from, to).await,
            Self::Secure(session) => session.rename(from, to).await,
        }
        .map_err(map_ftp_error)
    }
}

pub(crate) async fn read_ftp_stream<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    length: Option<u64>,
    sender: &tokio::sync::mpsc::Sender<Result<bytes::Bytes, StorageError>>,
) -> Result<(), StorageError> {
    let mut remaining = length.unwrap_or(u64::MAX);
    let mut buffer = vec![0_u8; FTP_CHUNK];
    while remaining > 0 {
        let length = buffer.len().min(remaining as usize);
        let count = stream
            .read(&mut buffer[..length])
            .await
            .map_err(map_ftp_io)?;
        if count == 0 {
            break;
        }
        remaining = remaining.saturating_sub(count as u64);
        if sender
            .send(Ok(bytes::Bytes::copy_from_slice(&buffer[..count])))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = tokio::io::copy(stream, &mut tokio::io::sink()).await;
    Ok(())
}

pub(crate) async fn mkdir_p_ftp(session: &mut FtpSession, path: &str) -> Result<(), StorageError> {
    let std_path = std::path::Path::new(path);
    if let Some(parent_str) = std_path
        .parent()
        .and_then(|parent| parent.to_str())
        .filter(|parent_str| !parent_str.is_empty() && *parent_str != "/")
    {
        let _ = Box::pin(mkdir_p_ftp(session, parent_str)).await;
    }
    match session.mkdir(path).await {
        Ok(_) => Ok(()),
        Err(_) if session.entry(path).await.is_ok() => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn tls_connector() -> AsyncRustlsConnector {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    AsyncRustlsConnector::from(suppaftp::tokio_rustls::TlsConnector::from(Arc::new(config)))
}
