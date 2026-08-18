use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use quichash_core::database::DatabaseFormat;
use quichash_core::database::DatabaseHandler;
use quichash_core::{Algorithm, HashMode, hash_file_mode};

use super::create::create_database;
use super::types::HashCreateOptions;
use super::verify::{default_database_name, is_verification_file, verify_database};
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn names_follow_selection_and_qh_standard() {
    assert_eq!(
        default_database_name(&[PathBuf::from("/tmp/photo.jpg")], Path::new("/tmp")),
        "photo.jpg.qh"
    );
    assert_eq!(
        default_database_name(
            &[PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            Path::new("/tmp/project")
        ),
        "project.qh"
    );
}

#[test]
fn creates_and_verifies_mixed_selection_without_hashing_database() {
    let temporary = TempDir::new();
    let root = temporary.path();
    fs::write(root.join("one.txt"), b"one").unwrap();
    fs::create_dir(root.join("folder")).unwrap();
    fs::write(root.join("folder/two.txt"), b"two").unwrap();
    fs::write(root.join("unrelated.txt"), b"ignored").unwrap();
    let destination = root.join("selection.qh");
    let stats = CopyStats::default();
    let cancelled = AtomicBool::new(false);
    create_database(
        &HashCreateOptions {
            sources: vec![root.join("one.txt"), root.join("folder")],
            root: root.to_owned(),
            destination: destination.clone(),
            algorithm: Algorithm::Blake3,
            format: DatabaseFormat::Quichash,
            compressed: false,
            parallel: true,
        },
        &cancelled,
        &stats,
    )
    .unwrap();

    let manifest = DatabaseHandler::read_manifest(&destination).unwrap();
    let paths = manifest
        .entries
        .iter()
        .map(|entry| entry.relative_path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![PathBuf::from("folder/two.txt"), PathBuf::from("one.txt")]
    );
    verify_database(&destination, root, &cancelled, &CopyStats::default()).unwrap();
}

#[test]
fn detects_core_database_and_checksum_extensions() {
    let temporary = TempDir::new();
    for name in [
        "checks.qh",
        "checks.qh.xz",
        "checks.hashdeep",
        "checks.sha256",
        "checks.blake3.xz",
    ] {
        let path = temporary.path().join(name);
        fs::write(&path, b"not parsed until verification").unwrap();
        assert!(is_verification_file(&path), "{name}");
    }
    let ordinary = temporary.path().join("notes.txt");
    fs::write(&ordinary, b"ordinary text").unwrap();
    assert!(!is_verification_file(&ordinary));
}

#[test]
fn verifies_conventional_algorithm_file_and_reports_changes() {
    let temporary = TempDir::new();
    let root = temporary.path();
    let source = root.join("one.txt");
    fs::write(&source, b"one").unwrap();
    let digest = hash_file_mode(&source, &[Algorithm::Sha256], HashMode::Full)
        .unwrap()
        .remove(0)
        .to_hex();
    let checksum = root.join("checks.sha256");
    fs::write(&checksum, format!("{digest}  one.txt\n")).unwrap();
    let cancelled = AtomicBool::new(false);
    verify_database(&checksum, root, &cancelled, &CopyStats::default()).unwrap();

    fs::write(&source, b"changed").unwrap();
    let error = verify_database(&checksum, root, &cancelled, &CopyStats::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("changed one.txt"), "{error}");
}

#[test]
fn creates_compressed_qh_and_hashdeep_with_canonical_suffixes() {
    let temporary = TempDir::new();
    let root = temporary.path();
    let source = root.join("one.txt");
    fs::write(&source, b"one").unwrap();
    let cancelled = AtomicBool::new(false);
    for (format, compressed, dest, expected_name) in [
        (DatabaseFormat::Quichash, true, "compressed", "compressed.qh.zst"),
        (DatabaseFormat::Hashdeep, false, "portable", "portable.hashdeep"),
    ] {
        let actual = create_database(
            &HashCreateOptions {
                sources: vec![source.clone()],
                root: root.to_owned(),
                destination: root.join(dest),
                algorithm: Algorithm::Blake3,
                format,
                compressed,
                parallel: true,
            },
            &cancelled,
            &CopyStats::default(),
        )
        .unwrap();
        assert_eq!(actual, root.join(expected_name));
        verify_database(&actual, root, &cancelled, &CopyStats::default()).unwrap();
    }
}
