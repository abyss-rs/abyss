use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::symlink;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{CloneCapabilities, copy_regular_file, move_path, remove_directory_tree};
use crate::inventory::Inventory;
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn recursive_removal_never_follows_a_symlink() {
    let temp = TempDir::new();
    let outside = temp.path().join("outside");
    let selected = temp.path().join("selected");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"safe").unwrap();
    fs::create_dir(&selected).unwrap();
    symlink(&outside, selected.join("link")).unwrap();

    remove_directory_tree(&selected).unwrap();

    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"safe");
    assert!(!selected.exists());
}

#[test]
fn copy_preserves_sparse_length_and_contents() {
    let temp = TempDir::new();
    let source = temp.path().join("sparse");
    let destination = temp.path().join("copy");
    let mut file = fs::File::create(&source).unwrap();
    file.write_all(b"start").unwrap();
    file.seek(SeekFrom::Start(8 * 1024 * 1024)).unwrap();
    file.write_all(b"end").unwrap();
    drop(file);
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

    assert_eq!(
        fs::metadata(&source).unwrap().len(),
        fs::metadata(&destination).unwrap().len()
    );
    assert_eq!(&fs::read(&destination).unwrap()[..5], b"start");
    assert!(fs::read(&destination).unwrap().ends_with(b"end"));
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
