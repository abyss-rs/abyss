use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::native;
use crate::progress::CopyStats;
use crate::remote_operation::locations::*;
use crate::storage::resume;
use crate::storage::{
    ErrorKind, ReadOptions, RemoteLocation, StorageBackend, StorageError, StorageRuntime,
};
pub(crate) const DOWNLOAD_JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct DownloadJournal {
    version: u32,
    scheme: String,
    connection: String,
    source_path: String,
    destination: Vec<u8>,
    total_size: u64,
    source_version: Option<String>,
}

pub(crate) async fn download_with_resume(
    storage: &StorageRuntime,
    source: &RemoteLocation,
    destination: &Path,
    expected_size: u64,
    overwrite: bool,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
) -> Result<(), StorageError> {
    let backend = storage.backend_async(source).await?;
    download_from_backend(
        backend,
        source,
        destination,
        expected_size,
        overwrite,
        cancelled,
        stats,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn download_from_backend(
    backend: Arc<dyn StorageBackend>,
    source: &RemoteLocation,
    destination: &Path,
    expected_size: u64,
    overwrite: bool,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    resume_root: Option<&Path>,
) -> Result<(), StorageError> {
    use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(io_storage_error)?;
    }
    let source_entry = backend.stat(&source.path).await?;
    let total_size = source_entry.size.unwrap_or(expected_size);
    if total_size != expected_size {
        return Err(StorageError::new(
            ErrorKind::Conflict,
            format!(
                "remote source size changed before download: expected {expected_size}, found {total_size}"
            ),
        ));
    }
    let destination_bytes = destination.as_os_str().as_encoded_bytes().to_vec();
    let source_path = source.path.display();
    let identity = [
        source.scheme.as_str(),
        source.connection.as_str(),
        source_path.as_str(),
        &destination.display().to_string(),
    ];
    let journal_path = match resume_root {
        Some(root) => resume::journal_path_in(root, "downloads", &identity),
        None => resume::journal_path("downloads", &identity),
    }
    .map_err(download_resume_error)?;
    let digest = journal_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| StorageError::new(ErrorKind::Other, "invalid download journal path"))?;
    let partial = destination
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".abyss-download-{digest}.part"));
    let wanted = DownloadJournal {
        version: DOWNLOAD_JOURNAL_VERSION,
        scheme: source.scheme.clone(),
        connection: source.connection.clone(),
        source_path,
        destination: destination_bytes,
        total_size,
        source_version: source_entry.version.clone(),
    };
    let journal_matches = matches!(
        resume::load::<DownloadJournal>(&journal_path),
        Ok(Some(existing))
            if existing.version == wanted.version
                && existing.scheme == wanted.scheme
                && existing.connection == wanted.connection
                && existing.source_path == wanted.source_path
                && existing.destination == wanted.destination
                && existing.total_size == wanted.total_size
                && existing.source_version == wanted.source_version
    );
    let mut offset = if journal_matches && backend.capabilities().range_read {
        tokio::fs::metadata(&partial)
            .await
            .map(|metadata| metadata.len().min(total_size))
            .unwrap_or(0)
    } else {
        let _ = tokio::fs::remove_file(&partial).await;
        0
    };
    resume::save(&journal_path, &wanted).map_err(download_resume_error)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .open(&partial)
        .await
        .map_err(io_storage_error)?;
    if offset > 0 {
        file.set_len(offset).await.map_err(io_storage_error)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(io_storage_error)?;
        stats.current_copied.fetch_add(offset, Ordering::Relaxed);
    }
    let transfer = async {
        if offset < total_size {
            let mut stream = backend
                .read(
                    &source.path,
                    ReadOptions {
                        offset: Some(offset),
                        length: Some(total_size - offset),
                        expected_version: source_entry.version,
                    },
                )
                .await?;
            while let Some(chunk) = stream.next().await {
                ensure_not_cancelled(cancelled)?;
                let chunk = chunk?;
                if offset.saturating_add(chunk.len() as u64) > total_size {
                    return Err(StorageError::new(
                        ErrorKind::Transport,
                        "remote download returned more bytes than requested",
                    ));
                }
                if !stats.wait_for_transfer(cancelled, chunk.len() as u64) {
                    return Err(StorageError::new(
                        ErrorKind::Cancelled,
                        "operation cancelled",
                    ));
                }
                file.write_all(&chunk).await.map_err(io_storage_error)?;
                offset += chunk.len() as u64;
                stats
                    .current_copied
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
        }
        if offset != total_size {
            return Err(StorageError::new(
                ErrorKind::Transport,
                format!(
                    "remote download ended early: expected {total_size} bytes, received {offset}"
                ),
            )
            .retryable(true));
        }
        file.flush().await.map_err(io_storage_error)?;
        file.sync_all().await.map_err(io_storage_error)?;
        Ok(())
    }
    .await;
    if let Err(error) = transfer {
        let _ = file.flush().await;
        let _ = file.sync_data().await;
        return Err(error);
    }
    drop(file);
    publish_download(&partial, destination, overwrite).map_err(io_storage_error)?;
    resume::remove(&journal_path).map_err(download_resume_error)?;
    Ok(())
}

pub(crate) fn publish_download(
    partial: &Path,
    destination: &Path,
    overwrite: bool,
) -> std::io::Result<()> {
    if !overwrite {
        return native::move_path(partial, destination);
    }
    #[cfg(windows)]
    {
        match std::fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    std::fs::rename(partial, destination)
}

pub(crate) fn download_resume_error(error: std::io::Error) -> StorageError {
    StorageError::new(
        ErrorKind::Other,
        format!("persist download resume state: {error}"),
    )
}
