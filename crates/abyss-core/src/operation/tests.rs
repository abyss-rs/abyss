use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::copy::{extract_paths, test_archive};
use super::delete::{delete_paths, delete_scanned_root};
use super::move_op::move_paths;
use crate::Error;
use crate::archive::{ArchiveCreateOptions, ArchiveIndex, create_archive};
use crate::copy::{ConflictDecision, ConflictResolver};
use crate::inventory::Inventory;
use crate::progress::CopyStats;
use crate::test_support::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

struct Overwrite;

impl ConflictResolver for Overwrite {
    fn resolve(&self, _destination: &Path) -> Result<ConflictDecision, Error> {
        Ok(ConflictDecision::Overwrite)
    }
}

#[test]
fn extracts_a_selected_archive_directory_safely() {
    let temp = TempDir::new();
    let archive_path = temp.path().join("sample.zip");
    let file = fs::File::create(&archive_path).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file("folder/001.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"one").unwrap();
    zip.start_file("other.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"other").unwrap();
    zip.start_file("folder/._001.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&[0x00, 0x05, 0x16, 0x07, 0, 0]).unwrap();
    zip.finish().unwrap();

    let index = ArchiveIndex::open(&archive_path, None).unwrap();
    let destination = temp.path().join("output");
    fs::create_dir_all(destination.join("folder")).unwrap();
    fs::write(destination.join("folder/001.txt"), b"old").unwrap();
    let cancelled = AtomicBool::new(false);
    let stats = CopyStats::default();
    extract_paths(
        &index,
        &["folder".to_owned()],
        "",
        &destination,
        None,
        &cancelled,
        &stats,
        &Overwrite,
    )
    .unwrap();

    assert_eq!(
        fs::read(destination.join("folder/001.txt")).unwrap(),
        b"one"
    );
    assert!(!destination.join("other.txt").exists());
    assert_eq!(
        fs::read(destination.join("folder/._001.txt")).unwrap(),
        [0x00, 0x05, 0x16, 0x07, 0, 0]
    );
}

#[test]
fn tests_tar_zst_archive_integrity() {
    let temp = TempDir::new();
    let source = temp.path().join("folder");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("a.txt"), b"alpha").unwrap();
    fs::write(source.join("b.txt"), b"bravo").unwrap();
    let destination = temp.path().join("folder.tar.zst");
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();

    let index = ArchiveIndex::open(&destination, None).unwrap();
    let cancelled = AtomicBool::new(false);
    let stats = CopyStats::default();
    test_archive(&index, None, &cancelled, &stats).unwrap();

    let snapshot = stats.snapshot();
    assert!(snapshot.objects_done >= 2);
    assert_eq!(snapshot.objects_done, snapshot.total_objects);
    assert_eq!(snapshot.logical_done, snapshot.total_bytes);
    assert!(snapshot.logical_done >= 10);
}

#[test]
fn single_item_move_supports_an_exact_rename_target() {
    let temp = TempDir::new();
    let source = temp.path().join("old name");
    let destination = temp.path().join("new name");
    fs::write(&source, b"content").unwrap();
    fs::write(temp.path().join("._old name"), [0x00, 0x05, 0x16, 0x07]).unwrap();
    fs::write(temp.path().join("._new name"), [0x00, 0x05, 0x16, 0x07]).unwrap();

    move_paths(
        std::slice::from_ref(&source),
        &destination,
        Arc::new(AtomicBool::new(false)),
        Arc::new(CopyStats::default()),
        &Overwrite,
    )
    .unwrap();

    assert!(!source.exists());
    assert_eq!(fs::read(destination).unwrap(), b"content");
    assert!(temp.path().join("._old name").exists());
    assert!(temp.path().join("._new name").exists());
}

#[test]
#[cfg(unix)]
fn recursive_delete_does_not_follow_symbolic_links() {
    let temp = TempDir::new();
    let outside = temp.path().join("outside");
    let selected = temp.path().join("selected");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"safe").unwrap();
    fs::create_dir(&selected).unwrap();
    symlink(&outside, selected.join("link")).unwrap();

    delete_paths(
        std::slice::from_ref(&selected),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();

    assert!(!selected.exists());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"safe");
}

#[test]
fn deleting_a_selected_folder_removes_hidden_appledouble_contents() {
    let temp = TempDir::new();
    let selected = temp.path().join("selected");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("file"), b"data").unwrap();
    fs::write(selected.join("._file"), [0x00, 0x05, 0x16, 0x07, 0, 0]).unwrap();

    delete_paths(
        std::slice::from_ref(&selected),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();

    assert!(!selected.exists());
}

#[test]
fn deleting_a_stale_missing_selection_is_successful() {
    let temp = TempDir::new();
    let missing = temp.path().join("already-gone");

    delete_paths(&[missing], &AtomicBool::new(false), &CopyStats::default()).unwrap();
}

#[test]
fn delete_retries_when_an_entry_appears_after_the_scan() {
    let temp = TempDir::new();
    let selected = temp.path().join("selected");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("known"), b"known").unwrap();
    let cancelled = AtomicBool::new(false);
    let inventory = Inventory::scan(&selected, &cancelled).unwrap();

    fs::write(selected.join("late-metadata"), b"late").unwrap();
    let failures =
        delete_scanned_root(&selected, &inventory, &cancelled, &CopyStats::default()).unwrap();

    assert!(failures.is_empty());
    assert!(!selected.exists());
}

#[test]
fn deletes_every_path_in_a_large_selected_batch() {
    let temp = TempDir::new();
    let paths = (0..64)
        .map(|index| {
            let path = temp.path().join(format!("file-{index:03}"));
            fs::write(&path, b"x").unwrap();
            path
        })
        .collect::<Vec<_>>();

    delete_paths(&paths, &AtomicBool::new(false), &CopyStats::default()).unwrap();

    assert!(paths.iter().all(|path| !path.exists()));
}

#[test]
#[cfg(unix)]
fn recursive_delete_removes_special_files_and_later_selections() {
    let temp = TempDir::new();
    let unsupported = temp.path().join("unsupported");
    let deletable = temp.path().join("deletable");
    fs::create_dir(&unsupported).unwrap();
    let _socket = UnixDatagram::bind(unsupported.join("socket")).unwrap();
    fs::write(&deletable, b"remove me").unwrap();

    delete_paths(
        &[unsupported.clone(), deletable.clone()],
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();

    assert!(!deletable.exists());
    assert!(!unsupported.exists());
}
