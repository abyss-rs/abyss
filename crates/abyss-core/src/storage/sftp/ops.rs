use std::io::{Read, Seek, SeekFrom, Write};

use async_trait::async_trait;
use futures_util::StreamExt;
use ssh2::{OpenFlags, OpenType};

use super::backend::SftpBackend;
use super::util::{
    delete_sftp_path, join_error, map_sftp_io, map_ssh_error, mkdir_p, sftp_rename, stat_version,
    storage_entry, temporary_remote_path,
};
use crate::storage::{
    BackendCapabilities, ByteStream, ErrorKind, ListPage, ReadOptions, StorageBackend,
    StorageEntry, StorageError, StoragePath, WriteOptions,
};

const SFTP_CHUNK: usize = 256 * 1024;

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
        let backend = self.clone();
        let path = self.remote_path(path)?;
        tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            let entries = sftp
                .readdir(&path)
                .map_err(map_ssh_error)?
                .into_iter()
                .filter_map(|(path, stat)| {
                    let name = path.file_name()?.as_encoded_bytes().to_vec();
                    if matches!(name.as_slice(), b"." | b"..") {
                        None
                    } else {
                        Some(storage_entry(name, &stat))
                    }
                })
                .collect();
            Ok(ListPage {
                entries,
                continuation: None,
            })
        })
        .await
        .map_err(join_error)?
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError> {
        let backend = self.clone();
        let path = self.remote_path(path)?;
        tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            let stat = sftp.stat(&path).map_err(map_ssh_error)?;
            Ok(storage_entry(
                path.file_name()
                    .map(|name| name.as_encoded_bytes().to_vec())
                    .unwrap_or_default(),
                &stat,
            ))
        })
        .await
        .map_err(join_error)?
    }

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        let backend = self.clone();
        let path = self.remote_path(path)?;
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        tokio::task::spawn_blocking(move || {
            let result = (|| {
                let (_session, sftp) = backend.connect()?;
                let mut file = sftp.open(&path).map_err(map_ssh_error)?;
                if let Some(offset) = options.offset {
                    file.seek(SeekFrom::Start(offset)).map_err(map_sftp_io)?;
                }
                let mut remaining = options.length.unwrap_or(u64::MAX);
                let mut buffer = vec![0_u8; SFTP_CHUNK];
                while remaining > 0 {
                    let length = buffer.len().min(remaining as usize);
                    let count = file.read(&mut buffer[..length]).map_err(map_sftp_io)?;
                    if count == 0 {
                        break;
                    }
                    remaining = remaining.saturating_sub(count as u64);
                    if sender
                        .blocking_send(Ok(bytes::Bytes::copy_from_slice(&buffer[..count])))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok::<_, StorageError>(())
            })();
            if let Err(error) = result {
                let _ = sender.blocking_send(Err(error));
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
        let backend = self.clone();
        let destination = self.remote_path(path)?;
        let (sender, mut receiver) =
            tokio::sync::mpsc::channel::<Result<bytes::Bytes, StorageError>>(4);
        let writer = tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            if let Ok(stat) = sftp.stat(&destination) {
                if !options.overwrite {
                    return Err(StorageError::new(
                        ErrorKind::AlreadyExists,
                        "SFTP destination already exists",
                    ));
                }
                if options.expected_version.as_deref() != Some(&stat_version(&stat)) {
                    return Err(StorageError::new(
                        ErrorKind::Conflict,
                        "SFTP destination changed before upload",
                    ));
                }
            }
            let temporary = temporary_remote_path(&destination);
            let result = (|| {
                let mut file = sftp
                    .open_mode(
                        &temporary,
                        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                        0o644,
                        OpenType::File,
                    )
                    .map_err(map_ssh_error)?;
                let mut received = 0_u64;
                while let Some(chunk) = receiver.blocking_recv() {
                    let chunk = chunk?;
                    file.write_all(&chunk).map_err(map_sftp_io)?;
                    received = received.saturating_add(chunk.len() as u64);
                }
                file.flush().map_err(map_sftp_io)?;
                if options.size.is_some_and(|size| size != received) {
                    return Err(StorageError::new(
                        ErrorKind::InvalidInput,
                        "SFTP upload source length changed",
                    ));
                }
                sftp_rename(&sftp, &temporary, &destination, true)
            })();
            if result.is_err() {
                let _ = sftp.unlink(&temporary);
            }
            result
        });
        while let Some(chunk) = source.next().await {
            if sender.send(chunk).await.is_err() {
                break;
            }
        }
        drop(sender);
        writer.await.map_err(join_error)?
    }

    async fn create_dir(&self, path: &StoragePath) -> Result<(), StorageError> {
        let backend = self.clone();
        let path = self.remote_path(path)?;
        tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            mkdir_p(&sftp, &path)
        })
        .await
        .map_err(join_error)?
    }

    async fn delete(&self, path: &StoragePath, recursive: bool) -> Result<(), StorageError> {
        let backend = self.clone();
        let path = self.remote_path(path)?;
        tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            delete_sftp_path(&sftp, &path, recursive)
        })
        .await
        .map_err(join_error)?
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
        let backend = self.clone();
        let source = self.remote_path(source)?;
        let destination = self.remote_path(destination)?;
        tokio::task::spawn_blocking(move || {
            let (_session, sftp) = backend.connect()?;
            sftp_rename(&sftp, &source, &destination, overwrite)
        })
        .await
        .map_err(join_error)?
    }
}
