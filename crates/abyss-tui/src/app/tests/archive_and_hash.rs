use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use quichash_core::database::{DatabaseFormat, DatabaseHandler};
use quichash_core::{Algorithm, HashMode, Manifest, ManifestEntry, hash_file_mode};

use crate::app::dialogs::archive::default_archive_name;
use crate::app::dialogs::{
    ArchiveCreateDialog, ArchiveCreateField, HashCreateField, InputAction, InputDialog, Modal,
};
use crate::app::state::App;
use crate::archive::{ArchiveContainer, CompressionMethod, CompressionPreset};
use crate::test_support::TempDir;

#[test]
fn hash_creation_uses_blake3_qh_defaults_and_background_b() {
    let temp = TempDir::new();
    std::fs::write(temp.path().join("input.txt"), b"hash me").unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    for _ in 0..100 {
        app.poll_background();
        if !app.panes[0].loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    app.panes[0].selected = app.panes[0]
        .entries
        .iter()
        .position(|entry| entry.name == "input.txt")
        .unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    let Some(Modal::HashCreate(dialog)) = &app.modal else {
        panic!("expected hash create dialog");
    };
    assert_eq!(dialog.filename(), "input.txt.qh");
    assert_eq!(dialog.algorithm, crate::hashing::HashAlgorithm::Blake3);
    assert_eq!(dialog.format, crate::hashing::HashDatabaseFormat::Quichash);
    assert!(dialog.parallel);
    assert_eq!(dialog.focus, HashCreateField::Algorithm);

    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.foreground_job.is_none());
    assert!(app.jobs.history().iter().any(|job| {
        job.kind == crate::operation::OperationKind::Hash
            && job.launch == crate::jobs::LaunchMode::Background
    }));
}

#[test]
fn h_on_qh_opens_verification_and_b_runs_it_in_background() {
    let temp = TempDir::new();
    let source = temp.path().join("input.txt");
    std::fs::write(&source, b"hash me").unwrap();
    let manifest = Manifest {
        entries: vec![ManifestEntry {
            relative_path: PathBuf::from("input.txt"),
            size: 7,
            mode: HashMode::Full,
            digests: hash_file_mode(&source, &[Algorithm::Blake3], HashMode::Full).unwrap(),
        }],
    };
    let database = temp.path().join("input.txt.qh");
    DatabaseHandler::write_manifest_file(&database, &manifest, DatabaseFormat::Quichash, false)
        .unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    for _ in 0..100 {
        app.poll_background();
        if !app.panes[0].loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    app.panes[0].selected = app.panes[0]
        .entries
        .iter()
        .position(|entry| entry.name == "input.txt.qh")
        .unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert!(matches!(app.modal, Some(Modal::VerifyHash(_))));
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.foreground_job.is_none());
    assert!(app.jobs.history().iter().any(|job| {
        job.kind == crate::operation::OperationKind::Verify
            && job.launch == crate::jobs::LaunchMode::Background
    }));
}

#[test]
fn transfer_dialogs_offer_background_without_reserving_plain_b() {
    let mut dialog = InputDialog::new(
        "Copy",
        "Destination:",
        String::new(),
        InputAction::Copy(vec![PathBuf::from("/source").into()]),
    );
    assert!(dialog.supports_background());
    dialog.insert('b');
    assert_eq!(dialog.text(), "b");

    let mkdir = InputDialog::new(
        "Create directory",
        "Name:",
        String::new(),
        InputAction::Mkdir,
    );
    assert!(!mkdir.supports_background());
}

#[test]
fn archive_creation_starts_with_enter() {
    let temp = TempDir::new();
    std::fs::write(temp.path().join("input.txt"), b"archive me").unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    for _ in 0..100 {
        app.poll_background();
        if !app.panes[0].loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let index = app.panes[0]
        .entries
        .iter()
        .position(|entry| entry.name == "input.txt")
        .unwrap();
    app.panes[0].selected = index;

    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    let Some(Modal::ArchiveCreate(dialog)) = &app.modal else {
        panic!("expected archive create dialog");
    };
    assert_eq!(default_archive_name(&dialog.sources), "input.txt.zst");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.foreground_job.is_some());
}

#[test]
fn archive_dialog_tracks_format_method_preset_and_suffix() {
    let temp = TempDir::new();
    let source = temp.path().join("a");
    std::fs::write(&source, b"archive me").unwrap();
    let mut dialog = ArchiveCreateDialog::new(vec![source]);
    assert_eq!(dialog.filename(), "a.zst");
    assert_eq!(dialog.container, ArchiveContainer::Auto);
    assert_eq!(dialog.method, CompressionMethod::Zstd);
    assert_eq!(dialog.level, 3);

    dialog.focus = ArchiveCreateField::Format;
    dialog.adjust(1);
    assert_eq!(dialog.container, ArchiveContainer::SevenZip);
    assert_eq!(dialog.method, CompressionMethod::Lzma2);
    assert_eq!(dialog.level, 5);
    assert_eq!(dialog.filename(), "a.7z");

    dialog.focus = ArchiveCreateField::Preset;
    dialog.adjust(2);
    assert_eq!(dialog.preset, CompressionPreset::Ultra);
    assert_eq!(dialog.level, 9);
    dialog.focus = ArchiveCreateField::Encryption;
    dialog.adjust(1);
    assert!(dialog.encryption);
    assert!(dialog.field_visible(ArchiveCreateField::Password));

    dialog.filename = "café.7z".chars().collect();
    dialog.focus = ArchiveCreateField::Format;
    dialog.adjust(1);
    assert_eq!(dialog.container, ArchiveContainer::Zip);
    assert_eq!(dialog.method, CompressionMethod::Deflate);
    assert_eq!(dialog.level, 9);
    assert_eq!(dialog.filename(), "café.zip");

    dialog.focus = ArchiveCreateField::Method;
    dialog.adjust(2);
    assert_eq!(dialog.method, CompressionMethod::Zstd);
    dialog.focus = ArchiveCreateField::Format;
    dialog.adjust(1);
    assert_eq!(dialog.container, ArchiveContainer::Tar);
    assert_eq!(dialog.method, CompressionMethod::Zstd);
    assert_eq!(dialog.filename(), "café.tar.zst");

    dialog.preset = CompressionPreset::Custom;
    dialog.level = 22;
    dialog.adjust(2);
    assert_eq!(dialog.container, ArchiveContainer::SevenZip);
    assert_eq!(dialog.method, CompressionMethod::Lzma2);
    assert_eq!(dialog.level, 9);
    assert_eq!(dialog.filename(), "café.7z");
}

#[test]
fn encrypted_archive_fields_submit_the_background_job_in_place() {
    let temp = TempDir::new();
    let source = temp.path().join("input.txt");
    std::fs::write(&source, b"archive me").unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    let mut dialog = ArchiveCreateDialog::new(vec![source]);
    dialog.focus = ArchiveCreateField::Format;
    dialog.adjust(1);
    dialog.encryption = true;
    dialog.focus = ArchiveCreateField::Password;
    app.modal = Some(Modal::ArchiveCreate(dialog));

    for character in "secret".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for character in "secret".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert!(matches!(app.modal, Some(Modal::ArchiveCreate(_))));
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT));
    assert!(app.modal.is_none());
    assert!(app.foreground_job.is_none());
    assert!(app.jobs.history().iter().any(|job| {
        job.kind == crate::operation::OperationKind::Archive
            && job.launch == crate::jobs::LaunchMode::Background
    }));
}

#[test]
fn archive_creation_b_starts_background_job() {
    let temp = TempDir::new();
    std::fs::write(temp.path().join("input.txt"), b"archive me").unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    for _ in 0..100 {
        app.poll_background();
        if !app.panes[0].loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let index = app.panes[0]
        .entries
        .iter()
        .position(|entry| entry.name == "input.txt")
        .unwrap();
    app.panes[0].selected = index;

    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(app.modal, Some(Modal::ArchiveCreate(_))));
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.foreground_job.is_none());
    assert!(
        app.jobs
            .history()
            .iter()
            .any(|job| job.kind == crate::operation::OperationKind::Archive
                && job.launch == crate::jobs::LaunchMode::Background)
    );
}
