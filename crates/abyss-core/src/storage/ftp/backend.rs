use async_trait::async_trait;
use futures_util::StreamExt;
use suppaftp::tokio::{AsyncFtpStream, AsyncRustlsFtpStream};
use tokio::io::AsyncWriteExt;

use super::entry::{delete_ftp_tree, ftp_entry, ftp_version, map_ftp_error, map_ftp_io};
use super::session::{FtpSession, mkdir_p_ftp, read_ftp_stream, tls_connector};
use crate::storage::{
    BackendCapabilities, ByteStream, EntryKind, ErrorKind, FtpConnection, FtpMode, ListPage,
    ReadOptions, StorageBackend, StorageEntry, StorageError, StoragePath, WriteOptions,
};

#[derive(Clone)]
pub struct FtpBackend {
    pub(crate) id: String,
    pub(crate) connection: FtpConnection,
}

impl FtpBackend {
    async fn connect(&self) -> Result<FtpSession, StorageError> {
        let port = self.connection.port.unwrap_or(match self.connection.mode {
            FtpMode::ImplicitTls => 990,
            _ => 21,
        });
        let address = format!("{}:{port}", self.connection.host);
        let password = match &self.connection.password_env {
            Some(variable) => std::env::var(variable).map_err(|_| {
                StorageError::new(
                    ErrorKind::Authentication,
                    format!("FTP password environment variable {variable} is not set"),
                )
            })?,
            None if self.connection.username == "anonymous" => "anonymous@".to_owned(),
            None => String::new(),
        };
        let mut session = match self.connection.mode {
            FtpMode::Plain => FtpSession::Plain(
                AsyncFtpStream::connect(&address)
                    .await
                    .map_err(map_ftp_error)?,
            ),
            FtpMode::ExplicitTls => {
                let stream = AsyncRustlsFtpStream::connect(&address)
                    .await
                    .map_err(map_ftp_error)?;
                FtpSession::Secure(
                    stream
                        .into_secure(tls_connector(), &self.connection.host)
                        .await
                        .map_err(map_ftp_error)?,
                )
            }
            FtpMode::ImplicitTls => FtpSession::Secure(
                AsyncRustlsFtpStream::connect_secure_implicit(
                    &address,
                    tls_connector(),
                    &self.connection.host,
                )
                .await
                .map_err(map_ftp_error)?,
            ),
        };
        session.login(&self.connection.username, &password).await?;
        Ok(session)
    }

    fn remote_path(&self, path: &StoragePath) -> Result<String, StorageError> {
        let StoragePath::Remote(path) = path else {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "FTP paths must be remote paths",
            ));
        };
        let root = self.connection.root.trim_end_matches('/');
        let relative = path.trim_matches('/');
        let result = match (root.is_empty(), relative.is_empty()) {
            (true, true) => "/".to_owned(),
            (true, false) => format!("/{relative}"),
            (false, true) => root.to_owned(),
            (false, false) => format!("{root}/{relative}"),
        };
        Ok(result)
    }
}

#[async_trait]
impl StorageBackend for FtpBackend {
    fn connection_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            range_read: true,
            create_dir: true,
            recursive_delete: true,
            atomic_rename: false,
            conditional_write: true,
            real_directories: true,
            ..Default::default()
        }
    }

    async fn list(
        &self,
        path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        let path = self.remote_path(path)?;
        let mut session = self.connect().await?;
        Ok(ListPage {
            entries: session
                .entries(&path)
                .await?
                .into_iter()
                .map(ftp_entry)
                .collect(),
            continuation: None,
        })
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError> {
        let path = self.remote_path(path)?;
        if path == "/" || path.is_empty() {
            return Ok(StorageEntry {
                name: Vec::new(),
                kind: EntryKind::Directory,
                size: None,
                modified: None,
                version: None,
            });
        }
        let mut session = self.connect().await?;
        session.entry(&path).await.map(ftp_entry)
    }

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        let path = self.remote_path(path)?;
        let mut session = self.connect().await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let result = async {
                match &mut session {
                    FtpSession::Plain(ftp) => {
                        if let Some(offset) = options.offset {
                            ftp.resume_transfer(offset as usize)
                                .await
                                .map_err(map_ftp_error)?;
                        }
                        let mut stream = ftp.retr_as_stream(&path).await.map_err(map_ftp_error)?;
                        read_ftp_stream(&mut stream, options.length, &sender).await?;
                        let _ = ftp.finalize_retr_stream(stream).await;
                    }
                    FtpSession::Secure(ftp) => {
                        if let Some(offset) = options.offset {
                            ftp.resume_transfer(offset as usize)
                                .await
                                .map_err(map_ftp_error)?;
                        }
                        let mut stream = ftp.retr_as_stream(&path).await.map_err(map_ftp_error)?;
                        read_ftp_stream(&mut stream, options.length, &sender).await?;
                        let _ = ftp.finalize_retr_stream(stream).await;
                    }
                }
                Ok::<_, StorageError>(())
            }
            .await;
            if let Err(error) = result {
                let _ = sender.send(Err(error)).await;
            }
        });
        Ok(Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) },
        )))
    }

    async fn write(
        &self,
        path: &StoragePath,
        mut source: ByteStream,
        options: WriteOptions,
    ) -> Result<(), StorageError> {
        let destination = self.remote_path(path)?;
        let temporary = format!("{destination}.abyss-{}.part", uuid::Uuid::new_v4());
        let mut session = self.connect().await?;
        if let Ok(entry) = session.entry(&destination).await {
            if !options.overwrite {
                return Err(StorageError::new(
                    ErrorKind::AlreadyExists,
                    "FTP destination already exists",
                ));
            }
            if let Some(expected) = &options.expected_version
                && expected != &ftp_version(&entry)
            {
                return Err(StorageError::new(
                    ErrorKind::Conflict,
                    "FTP destination changed before upload",
                ));
            }
        }
        if let Some(parent) = std::path::Path::new(&temporary)
            .parent()
            .and_then(|p| p.to_str())
            .filter(|p| !p.is_empty() && *p != "/")
        {
            let _ = mkdir_p_ftp(&mut session, parent).await;
        }
        let result = async {
            let mut received = 0_u64;
            match &mut session {
                FtpSession::Plain(ftp) => {
                    let mut stream = ftp
                        .put_with_stream(&temporary)
                        .await
                        .map_err(map_ftp_error)?;
                    while let Some(chunk) = source.next().await {
                        let chunk = chunk?;
                        stream.write_all(&chunk).await.map_err(map_ftp_io)?;
                        received = received.saturating_add(chunk.len() as u64);
                    }
                    ftp.finalize_put_stream(stream)
                        .await
                        .map_err(map_ftp_error)?;
                }
                FtpSession::Secure(ftp) => {
                    let mut stream = ftp
                        .put_with_stream(&temporary)
                        .await
                        .map_err(map_ftp_error)?;
                    while let Some(chunk) = source.next().await {
                        let chunk = chunk?;
                        stream.write_all(&chunk).await.map_err(map_ftp_io)?;
                        received = received.saturating_add(chunk.len() as u64);
                    }
                    ftp.finalize_put_stream(stream)
                        .await
                        .map_err(map_ftp_error)?;
                }
            }
            if options.size.is_some_and(|size| size != received) {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "FTP upload source length changed",
                ));
            }
            if options.overwrite {
                let _ = session.remove_file(&destination).await;
            }
            session.rename(&temporary, &destination).await
        }
        .await;
        if result.is_err() {
            let _ = session.remove_file(&temporary).await;
        }
        result
    }

    async fn create_dir(&self, path: &StoragePath) -> Result<(), StorageError> {
        let path = self.remote_path(path)?;
        if path == "/" || path.is_empty() {
            return Ok(());
        }
        let mut session = self.connect().await?;
        if session.entry(&path).await.is_ok() {
            return Ok(());
        }
        mkdir_p_ftp(&mut session, &path).await
    }

    async fn delete(&self, path: &StoragePath, recursive: bool) -> Result<(), StorageError> {
        let path = self.remote_path(path)?;
        let mut session = self.connect().await?;
        if recursive {
            delete_ftp_tree(&mut session, path).await
        } else {
            match session.entry(&path).await {
                Ok(entry) if entry.is_directory() => session.remove_dir(&path).await,
                _ => session.remove_file(&path).await,
            }
        }
    }

    async fn copy(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "FTP has no server-side copy operation",
        ))
    }

    async fn rename(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        overwrite: bool,
    ) -> Result<(), StorageError> {
        let source = self.remote_path(source)?;
        let destination = self.remote_path(destination)?;
        let mut session = self.connect().await?;
        if !overwrite && session.entry(&destination).await.is_ok() {
            return Err(StorageError::new(
                ErrorKind::AlreadyExists,
                "FTP rename destination already exists",
            ));
        }
        if overwrite {
            let _ = session.remove_file(&destination).await;
            let _ = session.remove_dir(&destination).await;
        }
        session.rename(&source, &destination).await
    }
}
