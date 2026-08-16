use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::{Buf, Bytes};
use futures_util::{StreamExt, TryStreamExt};
use tokio_util::io::ReaderStream;

use super::{BulkItem, TRANSFER_CHUNK};
use crate::progress::CopyStats;
use crate::remote_operation::locations::*;
use crate::storage::{
    ByteStream, EntryKind, ErrorKind, Location, ReadOptions, StorageBackend, StorageError,
    StorageRuntime, TreeEntry, WriteOptions,
};
pub(crate) async fn collect_tree(
    storage: &StorageRuntime,
    source: &Location,
    destination: &Location,
    source_backend: Option<&Arc<dyn StorageBackend>>,
) -> Result<Vec<BulkItem>, StorageError> {
    if let (Location::Remote(source_remote), Some(backend)) = (source, source_backend)
        && backend.capabilities().bulk_tree_read
    {
        let entries = backend.list_tree(&source_remote.path).await?;
        return entries
            .into_iter()
            .map(|entry| {
                Ok(BulkItem {
                    source: descend(source, &entry.path)?,
                    destination: descend(destination, &entry.path)?,
                    entry,
                    overwrite: false,
                    clone_from: None,
                })
            })
            .collect();
    }

    let mut result = Vec::new();
    let mut stack = vec![(source.clone(), destination.clone(), Vec::<Vec<u8>>::new())];
    while let Some((source_dir, destination_dir, relative)) = stack.pop() {
        for entry in list_all(storage, &source_dir).await? {
            let source_child = source_dir.child_transfer(&entry.name)?;
            let destination_child = destination_dir.child_transfer(&entry.name)?;
            let mut path = relative.clone();
            path.push(entry.name.clone());
            result.push(BulkItem {
                entry: TreeEntry {
                    path: path.clone(),
                    kind: entry.kind,
                    size: entry.size.unwrap_or(0),
                },
                source: source_child.clone(),
                destination: destination_child.clone(),
                overwrite: false,
                clone_from: None,
            });
            if entry.kind == EntryKind::Directory {
                stack.push((source_child, destination_child, path));
            }
        }
    }
    Ok(result)
}

pub(crate) fn descend(root: &Location, path: &[Vec<u8>]) -> Result<Location, StorageError> {
    path.iter().try_fold(root.clone(), |current, component| {
        current.child_transfer(component)
    })
}

pub(crate) struct ConcatenateState {
    storage: Arc<StorageRuntime>,
    entries: VecDeque<BulkItem>,
    current: Option<ByteStream>,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
}

pub(crate) fn concatenate_sources(
    storage: Arc<StorageRuntime>,
    entries: Vec<BulkItem>,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        ConcatenateState {
            storage,
            entries: entries.into(),
            current: None,
            cancelled,
            stats,
        },
        |mut state| async move {
            loop {
                if let Some(stream) = &mut state.current {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            if let Err(error) = ensure_not_cancelled(&state.cancelled) {
                                return Some((Err(error), state));
                            }
                            state
                                .stats
                                .current_copied
                                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                            return Some((Ok(chunk), state));
                        }
                        Some(Err(error)) => return Some((Err(error), state)),
                        None => state.current = None,
                    }
                }
                let item = state.entries.pop_front()?;
                state
                    .stats
                    .observe_transfer(Path::new(&item.destination.display()));
                let opened = match item.source {
                    Location::Local(path) => match tokio::fs::File::open(path).await {
                        Ok(file) => {
                            let stream = ReaderStream::with_capacity(file, TRANSFER_CHUNK)
                                .map_err(io_storage_error)
                                .map_ok(Bytes::from);
                            Ok(Box::pin(stream) as ByteStream)
                        }
                        Err(error) => Err(io_storage_error(error)),
                    },
                    Location::Remote(remote) => match state.storage.backend_async(&remote).await {
                        Ok(backend) => backend.read(&remote.path, ReadOptions::default()).await,
                        Err(error) => Err(error),
                    },
                };
                match opened {
                    Ok(stream) => state.current = Some(stream),
                    Err(error) => return Some((Err(error), state)),
                }
            }
        },
    ))
}

pub(crate) async fn consume_tree_stream(
    storage: &StorageRuntime,
    mut source: ByteStream,
    entries: &[BulkItem],
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
) -> Result<(), StorageError> {
    let mut pending = Bytes::new();
    for item in entries
        .iter()
        .filter(|item| item.entry.kind == EntryKind::File)
    {
        ensure_not_cancelled(cancelled)?;
        stats.observe_transfer(Path::new(&item.destination.display()));
        match &item.destination {
            Location::Local(path) => {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(io_storage_error)?;
                }
                let mut file = tokio::fs::File::create(path)
                    .await
                    .map_err(io_storage_error)?;
                write_exact_from_tree(
                    &mut source,
                    &mut pending,
                    item.entry.size,
                    &mut file,
                    cancelled,
                    stats,
                )
                .await?;
                use tokio::io::AsyncWriteExt as _;
                file.flush().await.map_err(io_storage_error)?;
            }
            Location::Remote(remote) => {
                let backend = storage.backend_async(remote).await?;
                let path = remote.path.clone();
                let size = item.entry.size;
                let overwrite = item.overwrite;
                let (sender, receiver) = tokio::sync::mpsc::channel(4);
                let stream: ByteStream = Box::pin(futures_util::stream::unfold(
                    receiver,
                    |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) },
                ));
                let writer = tokio::spawn(async move {
                    backend
                        .write(
                            &path,
                            stream,
                            WriteOptions {
                                size: Some(size),
                                overwrite,
                                expected_version: None,
                            },
                        )
                        .await
                });
                send_exact_from_tree(&mut source, &mut pending, size, sender, cancelled, stats)
                    .await?;
                writer
                    .await
                    .map_err(|error| StorageError::new(ErrorKind::Other, error.to_string()))??;
            }
        }
    }
    if source.next().await.transpose()?.is_some() || !pending.is_empty() {
        return Err(StorageError::new(
            ErrorKind::Transport,
            "bulk tree stream contained extra file data",
        ));
    }
    Ok(())
}

pub(crate) async fn write_exact_from_tree(
    source: &mut ByteStream,
    pending: &mut Bytes,
    mut remaining: u64,
    output: &mut tokio::fs::File,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
) -> Result<(), StorageError> {
    use tokio::io::AsyncWriteExt as _;
    while remaining > 0 {
        ensure_not_cancelled(cancelled)?;
        if pending.is_empty() {
            *pending = source.next().await.transpose()?.ok_or_else(|| {
                StorageError::new(ErrorKind::Transport, "bulk tree stream ended early")
            })?;
        }
        let length = pending.len().min(remaining as usize);
        output
            .write_all(&pending[..length])
            .await
            .map_err(io_storage_error)?;
        pending.advance(length);
        remaining -= length as u64;
        stats
            .current_copied
            .fetch_add(length as u64, Ordering::Relaxed);
    }
    Ok(())
}

pub(crate) async fn send_exact_from_tree(
    source: &mut ByteStream,
    pending: &mut Bytes,
    mut remaining: u64,
    sender: tokio::sync::mpsc::Sender<Result<Bytes, StorageError>>,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
) -> Result<(), StorageError> {
    while remaining > 0 {
        ensure_not_cancelled(cancelled)?;
        if pending.is_empty() {
            *pending = source.next().await.transpose()?.ok_or_else(|| {
                StorageError::new(ErrorKind::Transport, "bulk tree stream ended early")
            })?;
        }
        let length = pending.len().min(remaining as usize);
        let chunk = pending.split_to(length);
        sender.send(Ok(chunk)).await.map_err(|_| {
            StorageError::new(ErrorKind::Transport, "bulk destination closed early")
        })?;
        remaining -= length as u64;
        stats
            .current_copied
            .fetch_add(length as u64, Ordering::Relaxed);
    }
    drop(sender);
    Ok(())
}
