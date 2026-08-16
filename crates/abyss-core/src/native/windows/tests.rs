use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{CloneCapabilities, copy_regular_file, move_path, remove_path};
use crate::inventory::Inventory;
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn copyfile2_path_copies_contents() {
    let temp = TempDir::new();
    let source = temp.path().join("source.bin");
    let destination = temp.path().join("destination.bin");
    fs::write(&source, vec![7_u8; 2 * 1024 * 1024]).unwrap();
    let inventory =
        Inventory::scan_for_copy_with_progress(&source, &AtomicBool::new(false), None).unwrap();

    copy_regular_file(
        &source,
        &destination,
        &inventory.entries[0],
        &Arc::new(AtomicBool::new(false)),
        &Arc::new(CopyStats::default()),
        &mut CloneCapabilities::default(),
    )
    .unwrap();

    assert_eq!(fs::read(destination).unwrap(), vec![7_u8; 2 * 1024 * 1024]);
}

#[test]
fn native_disposition_deletes_readonly_files() {
    let temp = TempDir::new();
    let path = temp.path().join("readonly");
    fs::write(&path, b"delete").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();

    remove_path(&path, false).unwrap();

    assert!(!path.exists());
}

#[test]
fn native_move_does_not_replace_an_existing_target() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"destination").unwrap();

    assert!(move_path(&source, &destination).is_err());
    assert_eq!(fs::read(source).unwrap(), b"source");
    assert_eq!(fs::read(destination).unwrap(), b"destination");
}
