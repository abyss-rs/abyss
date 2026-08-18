use std::io::SeekFrom;
use std::path::Path;

use async_trait::async_trait;
use futures_util::StreamExt;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use super::backend::SftpBackend;
use super::util::{
    map_sftp_error, map_sftp_io, stat_version, storage_entry, temporary_remote_path,
};
use crate::storage::{
    BackendCapabilities, ByteStream, ErrorKind, ListPage, ReadOptions, StorageBackend,
    StorageEntry, StorageError, StoragePath, WriteOptions,
};

const SFTP_CHUNK: usize = 256 * 1024;

fn path_str(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

async fn sftp_rename(
    sftp: &SftpSession,
    source: &Path,
    destination: &Path,
    overwrite: bool,
) -> Result<(), StorageError> {
    let src_str = path_str(source);
    let dst_str = path_str(destination);
    if sftp.rename(&src_str, &dst_str).await.is_ok() {
        return Ok(());
    }
    if overwrite {
        let _ = sftp.remove_file(&dst_str).await;
        let _ = sftp.remove_dir(&dst_str).await;
    }
    sftp.rename(src_str, dst_str).await.map_err(map_sftp_error)
}

async fn mkdir_p(sftp: &SftpSession, path: &Path) -> Result<(), StorageError> {
    let p_str = path_str(path);
    if sftp.metadata(&p_str).await.is_ok() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty() && *parent != Path::new("/"))
    {
        let _ = Box::pin(mkdir_p(sftp, parent)).await;
    }
    match sftp.create_dir(&p_str).await {
        Ok(_) => Ok(()),
        Err(_) if sftp.metadata(&p_str).await.is_ok() => Ok(()),
        Err(error) => Err(map_sftp_error(error)),
    }
}

async fn delete_sftp_path(
    sftp: &SftpSession,
    path: &Path,
    recursive: bool,
) -> Result<(), StorageError> {
    let p_str = path_str(path);
    let metadata = sftp.metadata(&p_str).await.map_err(map_sftp_error)?;
    if metadata.is_dir() {
        if recursive {
            let entries = sftp.read_dir(&p_str).await.map_err(map_sftp_error)?;
            for child in entries {
                let name = child.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child_path = path.join(name);
                Box::pin(delete_sftp_path(sftp, &child_path, true)).await?;
            }
        }
        sftp.remove_dir(p_str).await.map_err(map_sftp_error)
    } else {
        sftp.remove_file(p_str).await.map_err(map_sftp_error)
    }
}

#[async_trait]
impl StorageBackend for SftpBackend {
    fn connection_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            range_read: true,
            create_dir: true,
            recursive_delete: true,
            atomic_rename: true,
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
        let (_session, sftp) = self.connect().await?;
        let entries = sftp
            .read_dir(path_str(&path))
            .await
            .map_err(map_sftp_error)?
            .filter_map(|entry| {
                let name_str = entry.file_name();
                if name_str == "." || name_str == ".." {
                    None
                } else {
                    Some(storage_entry(
                        name_str.as_bytes().to_vec(),
                        &entry.metadata(),
                    ))
                }
            })
            .collect();
        Ok(ListPage {
            entries,
            continuation: None,
        })
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError> {
        let path = self.remote_path(path)?;
        let (_session, sftp) = self.connect().await?;
        let metadata = sftp
            .metadata(path_str(&path))
            .await
            .map_err(map_sftp_error)?;
        Ok(storage_entry(
            path.file_name()
                .map(|name| name.as_encoded_bytes().to_vec())
                .unwrap_or_default(),
            &metadata,
        ))
    }

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        let path = self.remote_path(path)?;
        let (_session, sftp) = self.connect().await?;
        let mut file = sftp.open(path_str(&path)).await.map_err(map_sftp_error)?;
        if let Some(offset) = options.offset {
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(map_sftp_io)?;
        }
        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> =
            if let Some(length) = options.length {
                Box::new(file.take(length))
            } else {
                Box::new(file)
            };
        let stream = ReaderStream::with_capacity(reader, SFTP_CHUNK);
        let mapped = stream.map(|res| res.map_err(map_sftp_io));
        Ok(Box::pin(mapped))
    }

    async fn write(
        &self,
        path: &StoragePath,
        mut source: ByteStream,
        options: WriteOptions,
    ) -> Result<(), StorageError> {
        let destination = self.remote_path(path)?;
        let dst_str = path_str(&destination);
        let (_session, sftp) = self.connect().await?;
        if let Ok(metadata) = sftp.metadata(&dst_str).await {
            if !options.overwrite {
                return Err(StorageError::new(
                    ErrorKind::AlreadyExists,
                    "SFTP destination already exists",
                ));
            }
            if options.expected_version.as_deref() != Some(&stat_version(&metadata)) {
                return Err(StorageError::new(
                    ErrorKind::Conflict,
                    "SFTP destination changed before upload",
                ));
            }
        }
        let temporary = temporary_remote_path(&destination);
        let temp_str = path_str(&temporary);
        let upload_result: Result<(), StorageError> = async {
            let mut file = sftp
                .open_with_flags(
                    &temp_str,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                )
                .await
                .map_err(map_sftp_error)?;
            let mut received = 0_u64;
            while let Some(chunk_res) = source.next().await {
                let chunk = chunk_res?;
                file.write_all(&chunk).await.map_err(map_sftp_io)?;
                received = received.saturating_add(chunk.len() as u64);
            }
            file.flush().await.map_err(map_sftp_io)?;
            if options.size.is_some_and(|size| size != received) {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "SFTP upload source length changed",
                ));
            }
            sftp_rename(&sftp, &temporary, &destination, true).await
        }
        .await;

        if upload_result.is_err() {
            let _ = sftp.remove_file(&temp_str).await;
        }
        upload_result
    }

    async fn create_dir(&self, path: &StoragePath) -> Result<(), StorageError> {
        let path = self.remote_path(path)?;
        let (_session, sftp) = self.connect().await?;
        mkdir_p(&sftp, &path).await
    }

    async fn delete(&self, path: &StoragePath, recursive: bool) -> Result<(), StorageError> {
        let path = self.remote_path(path)?;
        let (_session, sftp) = self.connect().await?;
        delete_sftp_path(&sftp, &path, recursive).await
    }

    async fn copy(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "SFTP has no portable server-side copy operation",
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
        let (_session, sftp) = self.connect().await?;
        sftp_rename(&sftp, &source, &destination, overwrite).await
    }
}
