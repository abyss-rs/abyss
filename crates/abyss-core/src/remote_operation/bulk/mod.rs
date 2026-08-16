pub(crate) mod batch;
pub(crate) mod dedup;
pub(crate) mod tree;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use futures_util::StreamExt;

use self::batch::*;
use self::dedup::*;
use self::tree::*;
use crate::copy::{ConflictDecision, ConflictResolver};
use crate::progress::CopyStats;
use crate::remote_operation::file::prepare_file_destination;
use crate::remote_operation::locations::*;
use crate::storage::{
    EntryKind, ErrorKind, Location, StorageError, StoragePath, StorageRuntime, TreeEntry,
    TreeWriteEntry,
};
#[derive(Clone)]
pub(crate) struct BulkItem {
    pub(crate) entry: TreeEntry,
    pub(crate) source: Location,
    pub(crate) destination: Location,
    pub(crate) overwrite: bool,
    pub(crate) clone_from: Option<Vec<Vec<u8>>>,
}

pub(crate) const BULK_MAX_ENTRIES: usize = 4_096;
pub(crate) const BULK_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const BULK_COMPRESS_FILE_MAX: u64 = 1024 * 1024;
pub(crate) const TRANSFER_CHUNK: usize = 1024 * 1024;
pub(crate) const BULK_CONCURRENCY: usize = 16;
pub(crate) const DEDUP_MIN_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) async fn try_bulk_copy_tree(
    storage: Arc<StorageRuntime>,
    source: &Location,
    destination: &Location,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<bool, StorageError> {
    let trace_started = std::time::Instant::now();
    let trace = std::env::var_os("ABYSS_BULK_TRACE").is_some();
    let source_backend = match source {
        Location::Remote(remote) => Some(storage.backend_async(remote).await?),
        Location::Local(_) => None,
    };
    let destination_backend = match destination {
        Location::Remote(remote) => Some(storage.backend_async(remote).await?),
        Location::Local(_) => None,
    };
    let source_bulk = source_backend
        .as_ref()
        .is_some_and(|backend| backend.capabilities().bulk_tree_read);
    let destination_bulk = destination_backend
        .as_ref()
        .is_some_and(|backend| backend.capabilities().bulk_tree_write);
    if !source_bulk && !destination_bulk {
        return Ok(false);
    }

    let entries = match collect_tree(&storage, source, destination, source_backend.as_ref()).await {
        Ok(entries) => entries,
        Err(error) if error.kind == ErrorKind::Unsupported => return Ok(false),
        Err(error) => return Err(error),
    };
    if trace {
        eprintln!(
            "bulk trace: collected tree in {:.2?}",
            trace_started.elapsed()
        );
    }
    let file_count = entries
        .iter()
        .filter(|entry| entry.entry.kind == EntryKind::File)
        .count();
    if file_count < 8 {
        return Ok(false);
    }
    ensure_not_cancelled(cancelled)?;

    let mut planned = if destination_bulk {
        match stat_location(&storage, destination).await {
            Ok(entry) if entry.kind == EntryKind::Directory => {}
            Ok(_) => {
                return Err(StorageError::new(
                    ErrorKind::Conflict,
                    format!("directory conflicts with file: {}", destination.display()),
                ));
            }
            Err(error) if error.kind == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let Location::Remote(remote) = destination else {
            unreachable!("bulk destination has a remote backend")
        };
        let mut states = Vec::with_capacity(entries.len());
        for chunk in entries.chunks(BULK_MAX_ENTRIES) {
            let inspected = match destination_backend
                .as_ref()
                .expect("bulk destination backend")
                .inspect_tree(
                    &remote.path,
                    &chunk
                        .iter()
                        .map(|item| item.entry.clone())
                        .collect::<Vec<_>>(),
                )
                .await
            {
                Ok(states) => states,
                Err(error) if error.kind == ErrorKind::Unsupported => return Ok(false),
                Err(error) => return Err(error),
            };
            states.extend(inspected);
        }
        if states.len() != entries.len() {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "Kubernetes helper returned an incomplete bulk inspection",
            ));
        }
        let mut accepted = Vec::with_capacity(entries.len());
        for (mut item, state) in entries.into_iter().zip(states) {
            match (item.entry.kind, state.map(|value| value.kind)) {
                (EntryKind::Directory, None | Some(EntryKind::Directory)) => accepted.push(item),
                (EntryKind::Directory, Some(_)) => {
                    return Err(StorageError::new(
                        ErrorKind::Conflict,
                        format!(
                            "directory conflicts with file: {}",
                            item.destination.display()
                        ),
                    ));
                }
                (EntryKind::File, None) => accepted.push(item),
                (EntryKind::File, Some(EntryKind::File)) => {
                    let display = PathBuf::from(item.destination.display());
                    match conflicts
                        .resolve(&display)
                        .map_err(|error| StorageError::new(ErrorKind::Other, error.to_string()))?
                    {
                        ConflictDecision::Overwrite => {
                            item.overwrite = true;
                            accepted.push(item);
                        }
                        ConflictDecision::Skip => {
                            stats.skip_object(&display, item.entry.size);
                        }
                        ConflictDecision::Cancel => {
                            return Err(StorageError::new(
                                ErrorKind::Cancelled,
                                "operation cancelled",
                            ));
                        }
                    }
                }
                (EntryKind::File, Some(_)) => {
                    return Err(StorageError::new(
                        ErrorKind::Conflict,
                        format!(
                            "cannot overwrite directory with file: {}",
                            item.destination.display()
                        ),
                    ));
                }
                _ => {
                    return Err(StorageError::new(
                        ErrorKind::Unsupported,
                        "bulk transfers support only regular files and directories",
                    ));
                }
            }
        }
        accepted
    } else {
        let mut accepted = Vec::with_capacity(entries.len());
        for mut item in entries {
            ensure_not_cancelled(cancelled)?;
            if item.entry.kind == EntryKind::Directory {
                create_directory(&storage, &item.destination).await?;
                accepted.push(item);
            } else if item.entry.kind == EntryKind::File {
                if let Some(write) = prepare_file_destination(
                    &storage,
                    &item.destination,
                    conflicts,
                    item.entry.size,
                    stats,
                )
                .await?
                {
                    item.overwrite = write.overwrite;
                    accepted.push(item);
                }
            } else {
                return Err(StorageError::new(
                    ErrorKind::Unsupported,
                    "bulk transfers support only regular files and directories",
                ));
            }
        }
        accepted
    };

    if destination_bulk && matches!(source, Location::Local(_)) {
        discover_local_duplicates(&mut planned).await?;
    }
    if trace {
        let clones = planned
            .iter()
            .filter(|entry| entry.clone_from.is_some())
            .count();
        eprintln!(
            "bulk trace: planned tree with {clones} verified duplicates in {:.2?}",
            trace_started.elapsed()
        );
    }

    let same_pvc = {
        #[cfg(feature = "kubernetes")]
        {
            match (source, destination) {
                (Location::Remote(left), Location::Remote(right))
                    if left.scheme == "kube"
                        && right.scheme == "kube"
                        && left.connection == right.connection =>
                {
                    match (&left.path, &right.path) {
                        (StoragePath::Kubernetes(left), StoragePath::Kubernetes(right)) => {
                            left.get(..2) == right.get(..2)
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        #[cfg(not(feature = "kubernetes"))]
        {
            false
        }
    };
    if same_pvc {
        let (Location::Remote(source), Location::Remote(destination)) = (source, destination)
        else {
            unreachable!()
        };
        stats.begin_bulk_transfer(bulk_logical_bytes(&planned));
        for batch in bulk_batches(&planned) {
            let writes = batch
                .iter()
                .map(|item| TreeWriteEntry {
                    entry: item.entry.clone(),
                    overwrite: item.overwrite,
                    clone_from: None,
                })
                .collect();
            match source_backend
                .as_ref()
                .expect("same-PVC backend")
                .copy_tree(&source.path, &destination.path, writes)
                .await
            {
                Ok(()) => complete_bulk_entries(stats, &batch, 0, true, false),
                Err(error) if error.kind == ErrorKind::Unsupported => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        stats.complete_object(Path::new(&Location::Remote(destination.clone()).display()));
        return Ok(true);
    }

    let (payload_items, cloned_items): (Vec<_>, Vec<_>) = planned
        .iter()
        .cloned()
        .partition(|item| item.clone_from.is_none());
    let batches = bulk_batches(&payload_items);
    stats.begin_bulk_transfer(bulk_logical_bytes(&planned));
    let source = source.clone();
    let destination = destination.clone();
    let mut transfers = futures_util::stream::iter(batches.into_iter().map(|batch| {
        let storage = Arc::clone(&storage);
        let source = source.clone();
        let destination = destination.clone();
        let source_backend = source_backend.clone();
        let destination_backend = destination_backend.clone();
        let cancelled = Arc::clone(cancelled);
        let stats = Arc::clone(stats);
        async move {
            let physical = transfer_bulk_batch(
                storage,
                source,
                destination,
                source_backend,
                destination_backend,
                source_bulk,
                destination_bulk,
                &batch,
                cancelled,
                Arc::clone(&stats),
            )
            .await?;
            Ok::<_, StorageError>((batch, physical))
        }
    }))
    .buffer_unordered(BULK_CONCURRENCY);
    while let Some(result) = transfers.next().await {
        let (batch, physical) = result?;
        complete_bulk_entries(stats, &batch, physical, false, true);
    }
    if trace {
        eprintln!(
            "bulk trace: transferred unique payloads in {:.2?}",
            trace_started.elapsed()
        );
    }
    let mut clones =
        futures_util::stream::iter(bulk_batches(&cloned_items).into_iter().map(|batch| {
            let storage = Arc::clone(&storage);
            let source = source.clone();
            let destination = destination.clone();
            let source_backend = source_backend.clone();
            let destination_backend = destination_backend.clone();
            let cancelled = Arc::clone(cancelled);
            let stats = Arc::clone(stats);
            async move {
                let physical = transfer_bulk_batch(
                    storage,
                    source,
                    destination,
                    source_backend,
                    destination_backend,
                    source_bulk,
                    destination_bulk,
                    &batch,
                    cancelled,
                    stats,
                )
                .await?;
                Ok::<_, StorageError>((batch, physical))
            }
        }))
        .buffer_unordered(BULK_CONCURRENCY);
    while let Some(result) = clones.next().await {
        let (batch, physical) = result?;
        complete_bulk_entries(stats, &batch, physical, true, false);
        if trace {
            eprintln!(
                "bulk trace: materialized duplicate batch in {:.2?}",
                trace_started.elapsed()
            );
        }
    }
    stats.complete_object(Path::new(&destination.display()));
    planned.clear();
    Ok(true)
}
