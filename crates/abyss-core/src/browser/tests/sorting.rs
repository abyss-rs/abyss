use std::ffi::OsString;
use std::path::PathBuf;

use crate::browser::Pane;
use crate::browser::types::{BrowserEntry, BrowserKind, SortMode, SortSpec};

#[test]
fn hybrid_sorts_folders_by_name_then_files_by_descending_size() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    pane.entries = vec![
        BrowserEntry {
            name: "small".into(),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(1),
            modified: None,
            mode: None,
            ordinal: 1,
        },
        BrowserEntry {
            name: "z-dir".into(),
            raw_name: None,
            kind: BrowserKind::Directory,
            size: Some(0),
            modified: None,
            mode: None,
            ordinal: 2,
        },
        BrowserEntry {
            name: "large".into(),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(20),
            modified: None,
            mode: None,
            ordinal: 3,
        },
        BrowserEntry {
            name: "a-dir".into(),
            raw_name: None,
            kind: BrowserKind::Directory,
            size: Some(0),
            modified: None,
            mode: None,
            ordinal: 4,
        },
    ];
    pane.set_sort(SortSpec {
        mode: SortMode::Hybrid,
        reverse: false,
        directories_first: true,
    });
    let names = pane
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, ["a-dir", "z-dir", "large", "small"]);
}

#[test]
fn explicit_name_sort_uses_natural_episode_order() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    for (ordinal, name, size) in [
        (1, "s01e16.mkv", 50),
        (2, "s01e2.mkv", 500),
        (3, "s01e01.mkv", 5),
        (4, "s01e10.mkv", 1),
        (5, "s01e9.mkv", 1000),
    ] {
        pane.entries.push(BrowserEntry {
            name: name.into(),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(size),
            modified: None,
            mode: None,
            ordinal,
        });
    }

    pane.set_sort(SortSpec {
        mode: SortMode::Name,
        reverse: false,
        directories_first: true,
    });

    let names = pane
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "s01e01.mkv",
            "s01e2.mkv",
            "s01e9.mkv",
            "s01e10.mkv",
            "s01e16.mkv"
        ]
    );
}

#[test]
fn default_hybrid_detects_an_episode_collection() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    for episode in (1..=40).rev() {
        pane.entries.push(BrowserEntry {
            name: OsString::from(format!("Show.S01EP{episode:02}.mkv")),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(episode * 100),
            modified: None,
            mode: None,
            ordinal: episode,
        });
    }

    pane.set_sort(SortSpec::default());

    assert_eq!(
        pane.entries.first().unwrap().name,
        OsString::from("Show.S01EP01.mkv")
    );
    assert_eq!(
        pane.entries.last().unwrap().name,
        OsString::from("Show.S01EP40.mkv")
    );
}

#[test]
fn default_hybrid_detects_a_numeric_prefix_collection() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    for number in (1..=12).rev() {
        pane.entries.push(BrowserEntry {
            name: OsString::from(format!("{number:03} chapter.txt")),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(number * 100),
            modified: None,
            mode: None,
            ordinal: number,
        });
    }

    pane.set_sort(SortSpec::default());

    let names = pane
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names.first().unwrap(), "001 chapter.txt");
    assert_eq!(names.last().unwrap(), "012 chapter.txt");
    assert_eq!(pane.sort_label(), "Hybrid→Name");
}

#[test]
fn default_hybrid_keeps_size_order_for_random_files() {
    let mut pane = Pane::new(PathBuf::from("/tmp"));
    pane.entries = vec![
        BrowserEntry {
            name: "small-notes.txt".into(),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(10),
            modified: None,
            mode: None,
            ordinal: 1,
        },
        BrowserEntry {
            name: "large-archive.bin".into(),
            raw_name: None,
            kind: BrowserKind::File,
            size: Some(10_000),
            modified: None,
            mode: None,
            ordinal: 2,
        },
    ];

    pane.set_sort(SortSpec::default());

    assert_eq!(
        pane.entries.first().unwrap().name,
        OsString::from("large-archive.bin")
    );
}
