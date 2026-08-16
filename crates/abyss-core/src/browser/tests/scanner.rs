use std::fs;

use crate::browser::scanner::local::{read_directory_fallback, read_directory_streamed};
#[cfg(target_os = "macos")]
use crate::browser::scanner::macos::hide_dot_underscore_for_filesystem;
#[cfg(feature = "kubernetes")]
use crate::browser::scanner::remote::kubernetes_usage_name;
use crate::browser::types::BrowserKind;
use crate::test_support::TempDir;

#[test]
fn native_browser_policy_shows_dot_underscore_files() {
    let temp = TempDir::new();
    fs::write(temp.path().join("movie.mkv"), b"video").unwrap();
    fs::write(temp.path().join("._movie.mkv"), b"ordinary data").unwrap();
    let entries = read_directory_fallback(temp.path(), false, |_| true).unwrap();
    let names = entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(names.contains(&"movie.mkv".to_owned()));
    assert!(names.contains(&"._movie.mkv".to_owned()));
}

#[test]
fn non_native_browser_policy_hides_every_dot_underscore_entry() {
    let temp = TempDir::new();
    fs::write(temp.path().join("movie.mkv"), b"video").unwrap();
    fs::write(temp.path().join("._movie.mkv"), b"ordinary data").unwrap();
    fs::create_dir(temp.path().join("._folder")).unwrap();
    let entries = read_directory_fallback(temp.path(), true, |_| true).unwrap();
    let names = entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(names.contains(&"movie.mkv".to_owned()));
    assert!(!names.contains(&"._movie.mkv".to_owned()));
    assert!(!names.contains(&"._folder".to_owned()));
}

#[test]
#[cfg(target_os = "macos")]
fn only_apfs_and_hfs_use_native_visibility() {
    assert!(!hide_dot_underscore_for_filesystem(b"apfs"));
    assert!(!hide_dot_underscore_for_filesystem(b"hfs"));
    for filesystem in [b"exfat".as_slice(), b"msdos", b"ntfs", b"smbfs", b"fusefs"] {
        assert!(hide_dot_underscore_for_filesystem(filesystem));
    }
}

#[test]
fn native_stream_returns_kind_size_and_time() {
    use std::cell::Cell;

    let temp = TempDir::new();
    fs::create_dir(temp.path().join("folder")).unwrap();
    fs::write(temp.path().join("file"), b"12345").unwrap();
    let chunks = Cell::new(0);
    let entries = read_directory_streamed(temp.path(), false, |batch| {
        chunks.set(chunks.get() + usize::from(!batch.is_empty()));
        true
    })
    .unwrap();
    assert!(chunks.get() > 0);
    let file = entries.iter().find(|entry| entry.name == "file").unwrap();
    assert_eq!(file.kind, BrowserKind::File);
    assert_eq!(file.size, Some(5));
    assert!(file.modified.is_some());
    assert!(
        entries
            .iter()
            .any(|entry| { entry.name == "folder" && entry.kind == BrowserKind::Directory })
    );
}

#[test]
fn native_stream_filters_dot_underscore_before_emitting() {
    use std::cell::Cell;

    let temp = TempDir::new();
    fs::write(temp.path().join("visible"), b"data").unwrap();
    fs::write(temp.path().join("._hidden"), b"ordinary data").unwrap();
    let hidden_was_emitted = Cell::new(false);
    let entries = read_directory_streamed(temp.path(), true, |batch| {
        hidden_was_emitted
            .set(hidden_was_emitted.get() || batch.iter().any(|entry| entry.name == "._hidden"));
        true
    })
    .unwrap();

    assert!(!hidden_was_emitted.get());
    assert!(entries.iter().any(|entry| entry.name == "visible"));
    assert!(!entries.iter().any(|entry| entry.name == "._hidden"));
}

#[cfg(feature = "kubernetes")]
#[test]
fn kubernetes_usage_name_renders_gauge_and_warning() {
    let name = kubernetes_usage_name(b"media-pvc", Some("abyss-usage|90|900|1000|75|150|200"));
    let display = name.to_string_lossy();
    assert!(display.starts_with("⚠ media-pvc"));
    assert!(display.contains("[█████████░] 90%"));
    assert!(display.contains("inode 75%"));
}
