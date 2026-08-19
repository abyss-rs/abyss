use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use tokio_util::io::ReaderStream;

use crate::copy::{ConflictDecision, ConflictResolver};
use crate::progress::CopyStats;
use crate::remote_operation::bulk::TRANSFER_CHUNK;
use crate::remote_operation::download::download_with_resume;
use crate::remote_operation::locations::*;
use crate::storage::{
    ByteStream, EntryKind, ErrorKind, Location, ReadOptions, RemoteLocation, StorageBackend,
    StorageError, StoragePath, StorageRuntime, WriteOptions,
};
pub async fn copy_file(
    storage: &StorageRuntime,
    source: &Location,
    destination: &Location,
    size: u64,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), StorageError> {
    ensure_not_cancelled(cancelled)?;
    stats.begin_file(Path::new(&destination.display()), size);
    let Some(write) =
        prepare_file_destination(storage, destination, conflicts, size, stats).await?
    else {
        return Ok(());
    };
    match (source, destination) {
        (Location::Remote(source), Location::Remote(destination))
            if source.scheme == destination.scheme
                && source.connection == destination.connection =>
        {
            let backend = storage.backend_async(source).await?;
            if backend.capabilities().server_side_copy && !write.overwrite {
                match backend.copy(&source.path, &destination.path, false).await {
                    Ok(()) => {
                        stats.complete_file(size, 0, true, false);
                        return Ok(());
                    }
                    Err(error)
                        if error.retryable
                            || matches!(
                                error.kind,
                                ErrorKind::Unsupported | ErrorKind::InvalidInput
                            ) => {}
                    Err(error) => return Err(error),
                }
            }
            stream_between_remote(
                backend,
                source,
                destination,
                size,
                Arc::clone(cancelled),
                Arc::clone(stats),
                write,
            )
            .await?;
        }
        (Location::Remote(source), Location::Remote(destination)) => {
            let reader = storage.backend_async(source).await?;
            let writer = storage.backend_async(destination).await?;
            stream_between_backends(
                reader,
                writer,
                &source.path,
                &destination.path,
                size,
                Arc::clone(cancelled),
                Arc::clone(stats),
                write,
            )
            .await?;
        }
        (Location::Local(source), Location::Remote(destination)) => {
            let file = tokio::fs::File::open(source)
                .await
                .map_err(io_storage_error)?;
            let stream: ByteStream = Box::pin(
                ReaderStream::with_capacity(file, TRANSFER_CHUNK)
                    .map_err(io_storage_error)
                    .map_ok(Bytes::from),
            );
            let stream = progress_stream(stream, Arc::clone(cancelled), Arc::clone(stats));
            storage
                .backend_async(destination)
                .await?
                .write(
                    &destination.path,
                    stream,
                    WriteOptions {
                        size: Some(size),
                        overwrite: write.overwrite,
                        expected_version: write.expected_version,
                    },
                )
                .await?;
        }
        (Location::Remote(source), Location::Local(destination)) => {
            download_with_resume(
                storage,
                source,
                destination,
                size,
                write.overwrite,
                cancelled,
                stats,
            )
            .await?;
        }
        (Location::Local(_), Location::Local(_)) => {
            return Err(StorageError::new(
                ErrorKind::Other,
                "local transfers must use the native platform copy engine",
            ));
        }
    }
    stats.complete_file(size, size, false, false);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn stream_between_backends(
    reader: Arc<dyn StorageBackend>,
    writer: Arc<dyn StorageBackend>,
    source: &StoragePath,
    destination: &StoragePath,
    size: u64,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    write: FileWrite,
) -> Result<(), StorageError> {
    let stream = progress_stream(
        reader.read(source, ReadOptions::default()).await?,
        cancelled,
        stats,
    );
    writer
        .write(
            destination,
            stream,
            WriteOptions {
                size: Some(size),
                overwrite: write.overwrite,
                expected_version: write.expected_version,
            },
        )
        .await
}

pub(crate) async fn stream_between_remote(
    backend: Arc<dyn StorageBackend>,
    source: &RemoteLocation,
    destination: &RemoteLocation,
    size: u64,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    write: FileWrite,
) -> Result<(), StorageError> {
    let stream = backend.read(&source.path, ReadOptions::default()).await?;
    let stream = progress_stream(stream, cancelled, stats);
    backend
        .write(
            &destination.path,
            stream,
            WriteOptions {
                size: Some(size),
                overwrite: write.overwrite,
                expected_version: write.expected_version,
            },
        )
        .await
}

pub(crate) struct FileWrite {
    pub(crate) overwrite: bool,
    pub(crate) expected_version: Option<String>,
}

pub(crate) async fn prepare_file_destination(
    storage: &StorageRuntime,
    destination: &Location,
    conflicts: &dyn ConflictResolver,
    size: u64,
    stats: &CopyStats,
) -> Result<Option<FileWrite>, StorageError> {
    match stat_location(storage, destination).await {
        Ok(entry) if entry.kind == EntryKind::Directory => Err(StorageError::new(
            ErrorKind::Conflict,
            format!(
                "cannot overwrite directory with file: {}",
                destination.display()
            ),
        )),
        Ok(entry) => {
            let display = std::path::PathBuf::from(destination.display());
            match conflicts
                .resolve(&display)
                .map_err(|error| StorageError::new(ErrorKind::Other, error.to_string()))?
            {
                ConflictDecision::Overwrite => Ok(Some(FileWrite {
                    overwrite: true,
                    expected_version: entry.version,
                })),
                ConflictDecision::Skip => {
                    stats.skip_object(&display, size);
                    Ok(None)
                }
                ConflictDecision::Cancel => Err(StorageError::new(
                    ErrorKind::Cancelled,
                    "operation cancelled",
                )),
            }
        }
        Err(error) if error.kind == ErrorKind::NotFound => Ok(Some(FileWrite {
            overwrite: false,
            expected_version: None,
        })),
        Err(error) => Err(error),
    }
}

pub(crate) fn progress_stream(
    stream: ByteStream,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
) -> ByteStream {
    Box::pin(stream.map(move |result| {
        ensure_not_cancelled(&cancelled)?;
        let chunk = result?;
        if !stats.wait_for_transfer(&cancelled, chunk.len() as u64) {
            return Err(StorageError::new(
                ErrorKind::Cancelled,
                "operation cancelled",
            ));
        }
        stats
            .current_copied
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
        Ok(chunk)
    }))
}
