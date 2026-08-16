use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use crate::archive::{ArchiveFormat, ArchiveIndex, ArchiveMember};
use crate::browser::types::{BrowserEntry, BrowserKind};
use crate::browser::{BrowserService, Pane};
use crate::storage::{Location, RemoteLocation, StorageSource};
use crate::test_support::TempDir;
use unarc_rs::unified::ArchiveFormat as UnifiedFormat;

#[test]
fn equals_marks_and_advances() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    pane.entries = vec![
        BrowserEntry::parent(),
        BrowserEntry::unknown(OsString::from("one"), 1),
        BrowserEntry::unknown(OsString::from("two"), 2),
    ];
    pane.selected = 1;
    pane.toggle_mark_and_advance(10);
    assert!(pane.marks.contains(&OsString::from("one")));
    assert_eq!(pane.selected_paths(), vec![PathBuf::from("/tmp/one")]);
    assert_eq!(pane.selected, 2);
    assert_eq!(pane.entries[0].kind, BrowserKind::Parent);
}

#[test]
fn archive_pane_synthesizes_directories_and_navigates_them() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    let members = (1..=12)
        .map(|number| ArchiveMember {
            path: format!("season/{number:03}.mkv"),
            size: (13 - number) * 100,
            is_directory: false,
        })
        .chain([ArchiveMember {
            path: "cover.jpg".to_owned(),
            size: 50,
            is_directory: false,
        }])
        .collect();
    pane.enter_archive(
        ArchiveIndex {
            source: PathBuf::from("/tmp/show.zip"),
            format: ArchiveFormat::Unified(UnifiedFormat::Zip),
            members,
        },
        None,
        None,
        "show.zip".to_owned(),
    );

    assert!(pane.is_archive());
    assert!(
        pane.entries
            .iter()
            .any(|entry| { entry.name == "season" && entry.kind == BrowserKind::Directory })
    );
    pane.open_archive_directory(&OsString::from("season"));
    assert_eq!(pane.current_archive_member(), None);
    assert_eq!(pane.entries[1].name, OsString::from("001.mkv"));
    assert_eq!(pane.entries.last().unwrap().name, OsString::from("012.mkv"));
    assert_eq!(pane.display_path(), "/tmp/show.zip!/season");
}

#[test]
fn deleting_a_long_tail_moves_cursor_up_and_clamps_scroll() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    pane.entries.push(BrowserEntry::parent());
    for index in 0..100 {
        pane.entries.push(BrowserEntry::unknown(
            OsString::from(format!("file-{index:03}")),
            index + 1,
        ));
    }
    pane.selected = 99;
    pane.offset = 90;
    let deleted = (80..100)
        .map(|index| PathBuf::from(format!("/tmp/file-{index:03}")))
        .collect::<Vec<_>>();

    pane.remove_deleted_paths(&deleted, 10);

    assert_eq!(pane.current().unwrap().name, OsString::from("file-079"));
    assert!(pane.offset <= pane.selected);
    assert!(pane.selected < pane.offset + 10);
    assert!(pane.offset + 10 <= pane.entries.len());
}

#[test]
fn plus_style_marking_selects_every_file_in_a_long_batch() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    pane.entries.push(BrowserEntry::parent());
    for index in 0..100 {
        pane.entries.push(BrowserEntry::unknown(
            OsString::from(format!("file-{index:03}")),
            index + 1,
        ));
    }
    pane.selected = 1;

    for _ in 0..100 {
        pane.toggle_mark_and_advance(20);
    }

    assert_eq!(pane.marks.len(), 100);
    assert_eq!(pane.selected_paths().len(), 100);
}

#[test]
fn going_to_parent_selects_the_folder_that_was_left() {
    let temp = TempDir::new();
    let child = temp.path().join("child");
    fs::create_dir(&child).unwrap();
    let service = BrowserService::new();
    let mut pane = Pane::new(child);

    pane.change_to_parent(0, &service);
    let generation = pane.generation;
    let sort = pane.sort;
    pane.apply_directory(
        generation,
        &Location::Local(temp.path().to_owned()),
        sort,
        Ok(vec![BrowserEntry {
            name: OsString::from("child"),
            raw_name: None,
            kind: BrowserKind::Directory,
            size: Some(0),
            modified: None,
            mode: Some(0o755),
            ordinal: 1,
        }]),
    );

    assert_eq!(pane.cwd, temp.path());
    assert_eq!(pane.current().unwrap().name, OsString::from("child"));
}

#[test]
fn reload_can_select_a_newly_created_folder() {
    let temp = TempDir::new();
    let service = BrowserService::new();
    let mut pane = Pane::new(temp.path().to_owned());
    pane.entries = vec![
        BrowserEntry::parent(),
        BrowserEntry::unknown(OsString::from("old"), 1),
    ];
    pane.selected = 1;

    pane.reload_selecting(0, OsString::from("created"), &service);
    let generation = pane.generation;
    let sort = pane.sort;
    pane.apply_directory(
        generation,
        &Location::Local(temp.path().to_owned()),
        sort,
        Ok(vec![
            BrowserEntry {
                name: OsString::from("created"),
                raw_name: None,
                kind: BrowserKind::Directory,
                size: Some(0),
                modified: None,
                mode: Some(0o755),
                ordinal: 1,
            },
            BrowserEntry::unknown(OsString::from("old"), 2),
        ]),
    );

    assert_eq!(pane.current().unwrap().name, OsString::from("created"));
}

#[test]
fn source_view_preserves_the_complete_directory_state() {
    let temp = TempDir::new();
    let service = BrowserService::new();
    let mut pane = Pane::new(temp.path().to_owned());
    pane.entries = vec![
        BrowserEntry::parent(),
        BrowserEntry::unknown(OsString::from("kept"), 1),
    ];
    pane.selected = 1;
    pane.offset = 1;
    pane.marks.insert(OsString::from("kept"));
    let generation = pane.generation;

    pane.open_sources(0, &service);
    assert!(pane.showing_sources());
    pane.close_sources();

    assert_eq!(pane.location, Location::Local(temp.path().to_owned()));
    assert_eq!(pane.entries.len(), 2);
    assert_eq!(pane.selected, 1);
    assert_eq!(pane.offset, 1);
    assert!(pane.marks.contains(&OsString::from("kept")));
    assert_eq!(pane.generation, generation);
}

#[test]
fn stale_source_results_are_rejected_and_cursor_is_stable() {
    let temp = TempDir::new();
    let service = BrowserService::new();
    let mut pane = Pane::new(temp.path().to_owned());
    pane.open_sources(0, &service);
    let first_generation = pane.source_view.as_ref().unwrap().generation;
    let mut sources = vec![StorageSource::local()];
    let mut remote = StorageSource::local();
    remote.id = "remote".to_owned();
    remote.name = "Remote".to_owned();
    remote.location = Location::Remote(RemoteLocation {
        scheme: "s3".to_owned(),
        connection: "remote".to_owned(),
        path: crate::storage::StoragePath::Remote(String::new()),
    });
    sources.push(remote);
    pane.apply_sources(first_generation, sources.clone(), 10);
    pane.source_select_index(1, 10);

    pane.refresh_sources(0, &service);
    let second_generation = pane.source_view.as_ref().unwrap().generation;
    pane.apply_sources(first_generation, vec![StorageSource::local()], 10);
    assert_eq!(pane.source_view.as_ref().unwrap().entries.len(), 2);
    pane.apply_sources(second_generation, sources, 10);
    assert_eq!(
        pane.selected_source().unwrap().source.id,
        "remote",
        "selection follows the stable source ID"
    );
}

#[test]
fn last_local_directory_survives_remote_browsing() {
    let temp = TempDir::new();
    let service = BrowserService::new();
    let mut pane = Pane::new(temp.path().to_owned());
    pane.change_location(
        0,
        Location::Remote(RemoteLocation {
            scheme: "s3".to_owned(),
            connection: "missing".to_owned(),
            path: crate::storage::StoragePath::Remote(String::new()),
        }),
        &service,
    );
    assert_eq!(pane.local_restore_path(), temp.path());
}
