use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::tree::*;
use super::{BULK_COMPRESS_FILE_MAX, BULK_MAX_BYTES, BULK_MAX_ENTRIES, BulkItem};
use crate::progress::CopyStats;
use crate::remote_operation::locations::*;
use crate::storage::{
    EntryKind, Location, StorageBackend, StorageError, StorageRuntime, TreeWriteEntry, WireProgress,
};
#[allow(clippy::too_many_arguments)]
pub(crate) async fn transfer_bulk_batch(
    storage: Arc<StorageRuntime>,
    source: Location,
    destination: Location,
    source_backend: Option<Arc<dyn StorageBackend>>,
    destination_backend: Option<Arc<dyn StorageBackend>>,
    source_bulk: bool,
    destination_bulk: bool,
    batch: &[BulkItem],
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
) -> Result<u64, StorageError> {
    ensure_not_cancelled(&cancelled)?;
    let batch_wire = Arc::new(AtomicU64::new(0));
    let file_entries = batch
        .iter()
        .filter(|item| item.entry.kind == EntryKind::File && item.clone_from.is_none())
        .map(|item| item.entry.clone())
        .collect::<Vec<_>>();
    let input = if source_bulk {
        let Location::Remote(remote) = &source else {
            unreachable!("bulk source has a remote backend")
        };
        source_backend
            .as_ref()
            .expect("bulk source backend")
            .read_tree(
                &remote.path,
                file_entries,
                Some(wire_progress(Arc::clone(&stats), Arc::clone(&batch_wire))),
            )
            .await?
    } else {
        concatenate_sources(
            Arc::clone(&storage),
            batch
                .iter()
                .filter(|item| item.entry.kind == EntryKind::File && item.clone_from.is_none())
                .cloned()
                .collect(),
            Arc::clone(&cancelled),
            Arc::clone(&stats),
        )
    };

    if destination_bulk {
        let Location::Remote(remote) = &destination else {
            unreachable!("bulk destination has a remote backend")
        };
        let writes = batch
            .iter()
            .map(|item| TreeWriteEntry {
                entry: item.entry.clone(),
                overwrite: item.overwrite,
                clone_from: item.clone_from.clone(),
            })
            .collect();
        destination_backend
            .as_ref()
            .expect("bulk destination backend")
            .write_tree(
                &remote.path,
                writes,
                input,
                Some(wire_progress(Arc::clone(&stats), Arc::clone(&batch_wire))),
            )
            .await?;
    } else {
        consume_tree_stream(&storage, input, batch, &cancelled, &stats).await?;
    }
    Ok(batch_wire.load(Ordering::Relaxed))
}

pub(crate) fn bulk_logical_bytes(entries: &[BulkItem]) -> u64 {
    entries
        .iter()
        .filter(|item| item.entry.kind == EntryKind::File)
        .fold(0_u64, |total, item| total.saturating_add(item.entry.size))
}

pub(crate) fn bulk_batches(entries: &[BulkItem]) -> Vec<Vec<BulkItem>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut bytes = 0_u64;
    for item in entries {
        let item_bytes = if item.entry.kind == EntryKind::File {
            item.entry.size
        } else {
            0
        };
        if item.entry.kind == EntryKind::File && item.entry.size > BULK_COMPRESS_FILE_MAX {
            if !current.is_empty() {
                batches.push(std::mem::take(&mut current));
                bytes = 0;
            }
            batches.push(vec![item.clone()]);
            continue;
        }
        if !current.is_empty()
            && (current.len() >= BULK_MAX_ENTRIES
                || bytes.saturating_add(item_bytes) > BULK_MAX_BYTES)
        {
            batches.push(std::mem::take(&mut current));
            bytes = 0;
        }
        bytes = bytes.saturating_add(item_bytes);
        current.push(item.clone());
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

pub(crate) fn wire_progress(stats: Arc<CopyStats>, batch_wire: Arc<AtomicU64>) -> WireProgress {
    Arc::new(move |bytes| {
        stats.current_wire.fetch_add(bytes, Ordering::Relaxed);
        batch_wire.fetch_add(bytes, Ordering::Relaxed);
    })
}

pub(crate) fn complete_bulk_entries(
    stats: &CopyStats,
    entries: &[BulkItem],
    physical: u64,
    cloned: bool,
    streamed: bool,
) {
    let mut files = 0_u64;
    let mut logical = 0_u64;
    for item in entries {
        if item.entry.kind == EntryKind::File {
            files += 1;
            logical = logical.saturating_add(item.entry.size);
        } else {
            stats.complete_object(Path::new(&item.destination.display()));
        }
    }
    stats.complete_bulk_files(files, logical, physical, cloned, streamed);
}
