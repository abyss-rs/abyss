mod kubernetes;
mod stream;

use crate::remote_operation::bulk::batch::bulk_batches;
use crate::remote_operation::bulk::{BULK_MAX_BYTES, BULK_MAX_ENTRIES, BulkItem};
use std::path::PathBuf;

use crate::storage::{EntryKind, Location, TreeEntry};

fn bulk_item(index: usize, size: u64) -> BulkItem {
    let name = format!("file-{index}").into_bytes();
    BulkItem {
        entry: TreeEntry {
            path: vec![name.clone()],
            kind: EntryKind::File,
            size,
        },
        source: Location::Local(PathBuf::from(format!("/source/file-{index}"))),
        destination: Location::Local(PathBuf::from(format!("/destination/file-{index}"))),
        overwrite: false,
        clone_from: None,
    }
}

#[test]
fn bulk_batches_bound_entry_count() {
    let entries = (0..(BULK_MAX_ENTRIES + 17))
        .map(|index| bulk_item(index, 1))
        .collect::<Vec<_>>();
    let batches = bulk_batches(&entries);
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), BULK_MAX_ENTRIES);
    assert_eq!(batches[1].len(), 17);
}

#[test]
fn bulk_batches_bound_logical_bytes() {
    let entries = vec![
        bulk_item(0, BULK_MAX_BYTES),
        bulk_item(1, 1),
        bulk_item(2, 2),
    ];
    let batches = bulk_batches(&entries);
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[1].len(), 2);
}
