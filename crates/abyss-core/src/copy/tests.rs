use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::engine::run;
use super::target::{resolve_target, usable_parent};
use crate::Error;
use crate::test_support::TempDir;

#[test]
fn existing_directory_is_a_container() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();

    assert_eq!(
        resolve_target(&source, &destination).unwrap(),
        destination.join("source")
    );
}

#[test]
fn relative_destination_uses_the_current_directory_as_parent() {
    assert_eq!(
        usable_parent(std::path::Path::new("copy")),
        std::path::Path::new(".")
    );
}

#[test]
#[cfg(unix)]
fn copies_and_overwrites_a_tree() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let container = temp.path().join("container");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("nested/file"), b"new data").unwrap();
    symlink("nested/file", source.join("link")).unwrap();
    fs::create_dir(&container).unwrap();
    fs::create_dir(container.join("source")).unwrap();
    fs::create_dir(container.join("source/nested")).unwrap();
    fs::write(container.join("source/nested/file"), b"old").unwrap();

    run(&source, &container, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(
        fs::read(container.join("source/nested/file")).unwrap(),
        b"new data"
    );
    assert_eq!(
        fs::read_link(container.join("source/link")).unwrap(),
        std::path::Path::new("nested/file")
    );
}

#[test]
fn skips_source_appledouble_and_preserves_destination_companion() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("movie"), b"video").unwrap();
    fs::write(source.join("._movie"), [0x00, 0x05, 0x16, 0x07, 1, 2, 3, 4]).unwrap();
    fs::create_dir(destination.join("source")).unwrap();
    fs::write(
        destination.join("source/._movie"),
        [0x00, 0x05, 0x16, 0x07, 4, 3, 2, 1],
    )
    .unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(
        fs::read(destination.join("source/movie")).unwrap(),
        b"video"
    );
    assert_eq!(
        fs::read(destination.join("source/._movie")).unwrap(),
        [0x00, 0x05, 0x16, 0x07, 4, 3, 2, 1]
    );
    assert!(source.join("._movie").exists());
}

#[test]
fn preserves_non_appledouble_companion_without_reporting_a_cleanup_error() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::write(&source, b"data").unwrap();
    fs::write(temp.path().join("._destination"), b"user data").unwrap();

    let result = run(&source, &destination, Arc::new(AtomicBool::new(false)));

    assert!(result.is_ok());
    assert_eq!(fs::read(&destination).unwrap(), b"data");
    assert_eq!(
        fs::read(temp.path().join("._destination")).unwrap(),
        b"user data"
    );
}

#[test]
fn copies_a_single_file_into_an_existing_directory() {
    let temp = TempDir::new();
    let source = temp.path().join("movie.bin");
    let destination = temp.path().join("destination");
    fs::write(&source, b"file data").unwrap();
    fs::create_dir(&destination).unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(
        fs::read(destination.join("movie.bin")).unwrap(),
        b"file data"
    );
}

#[test]
fn preserves_hard_links() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("first"), b"same blocks").unwrap();
    fs::hard_link(source.join("first"), source.join("second")).unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    let first_path = destination.join("first");
    let second_path = destination.join("second");
    let first = fs::metadata(&first_path).unwrap();
    let second = fs::metadata(&second_path).unwrap();
    assert!(same_test_file(&first_path, &first, &second_path, &second));
}

#[test]
fn supports_unicode_and_spaces() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    let name = "café archive";
    fs::create_dir(&source).unwrap();
    fs::write(source.join(name), b"bytes").unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(fs::read(destination.join(name)).unwrap(), b"bytes");
}

#[test]
fn supports_names_near_the_filesystem_limit() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    let name = "x".repeat(240);
    fs::create_dir(&source).unwrap();
    fs::write(source.join(&name), b"long name").unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(fs::read(destination.join(name)).unwrap(), b"long name");
}

#[test]
fn rejects_a_destination_inside_the_source() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();

    let error = run(
        &source,
        &source.join("nested"),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap_err();

    assert!(matches!(error, Error::Message(message) if message.contains("inside the source")));
}

#[test]
#[cfg(unix)]
fn preserves_basic_file_and_directory_metadata() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file"), b"metadata").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
    fs::set_permissions(source.join("file"), fs::Permissions::from_mode(0o640)).unwrap();

    run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap();

    assert_eq!(fs::metadata(&destination).unwrap().mode() & 0o777, 0o750);
    assert_eq!(
        fs::metadata(destination.join("file")).unwrap().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
fn same_test_file(
    _left_path: &std::path::Path,
    left: &fs::Metadata,
    _right_path: &std::path::Path,
    right: &fs::Metadata,
) -> bool {
    left.ino() == right.ino()
}

#[cfg(windows)]
fn same_test_file(
    left_path: &std::path::Path,
    _left: &fs::Metadata,
    right_path: &std::path::Path,
    _right: &fs::Metadata,
) -> bool {
    let left = crate::native::path_identity(left_path).unwrap();
    let right = crate::native::path_identity(right_path).unwrap();
    left.volume == right.volume && left.index == right.index
}

#[test]
fn rejects_file_directory_conflicts() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("item"), b"file").unwrap();
    fs::create_dir(&destination).unwrap();
    fs::create_dir(destination.join("source")).unwrap();
    fs::create_dir(destination.join("source/item")).unwrap();

    let error = run(&source, &destination, Arc::new(AtomicBool::new(false))).unwrap_err();

    assert!(matches!(error, Error::Message(message) if message.contains("type conflict")));
}
