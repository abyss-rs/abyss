use std::collections::HashMap;
use std::path::Path;

use futures_util::StreamExt;

use super::{BULK_CONCURRENCY, BulkItem, DEDUP_MIN_BYTES, TRANSFER_CHUNK};
use crate::remote_operation::locations::*;
use crate::storage::{EntryKind, Location, StorageError};
pub(crate) async fn discover_local_duplicates(
    entries: &mut [BulkItem],
) -> Result<(), StorageError> {
    let mut groups = HashMap::<u64, Vec<usize>>::new();
    for (index, item) in entries.iter().enumerate() {
        if item.entry.kind == EntryKind::File
            && item.entry.size >= DEDUP_MIN_BYTES
            && matches!(item.source, Location::Local(_))
        {
            groups.entry(item.entry.size).or_default().push(index);
        }
    }
    for (size, mut pending) in groups {
        while pending.len() > 1 {
            let representative = pending.remove(0);
            let Location::Local(existing) = &entries[representative].source else {
                unreachable!()
            };
            let existing = existing.clone();
            let mut comparisons = futures_util::stream::iter(pending.into_iter().map(|index| {
                let existing = existing.clone();
                let Location::Local(candidate) = &entries[index].source else {
                    unreachable!()
                };
                let candidate = candidate.clone();
                async move {
                    Ok::<_, StorageError>((
                        index,
                        local_files_equal(&existing, &candidate, size).await?,
                    ))
                }
            }))
            .buffer_unordered(BULK_CONCURRENCY);
            let mut different = Vec::new();
            let mut equal = Vec::new();
            while let Some(result) = comparisons.next().await {
                let (index, matches) = result?;
                if matches {
                    equal.push(index);
                } else {
                    different.push(index);
                }
            }
            drop(comparisons);
            let source = entries[representative].entry.path.clone();
            for index in equal {
                entries[index].clone_from = Some(source.clone());
            }
            pending = different;
        }
    }
    Ok(())
}

pub(crate) async fn local_files_equal(
    left: &Path,
    right: &Path,
    size: u64,
) -> Result<bool, StorageError> {
    use tokio::io::AsyncReadExt as _;

    let mut left = tokio::fs::File::open(left)
        .await
        .map_err(io_storage_error)?;
    let mut right = tokio::fs::File::open(right)
        .await
        .map_err(io_storage_error)?;
    let mut left_buffer = vec![0_u8; TRANSFER_CHUNK];
    let mut right_buffer = vec![0_u8; TRANSFER_CHUNK];
    let mut remaining = size;
    while remaining > 0 {
        let length = left_buffer.len().min(remaining as usize);
        left.read_exact(&mut left_buffer[..length])
            .await
            .map_err(io_storage_error)?;
        right
            .read_exact(&mut right_buffer[..length])
            .await
            .map_err(io_storage_error)?;
        if left_buffer[..length] != right_buffer[..length] {
            return Ok(false);
        }
        remaining -= length as u64;
    }
    Ok(true)
}
