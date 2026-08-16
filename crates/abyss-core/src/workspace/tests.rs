use std::fs;
use std::path::PathBuf;

use super::state::*;
use super::tabs::*;
use crate::browser::{Pane, SortMode, SortSpec};
use crate::storage::Location;

#[test]
fn round_trip_preserves_local_remote_and_sort_state() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("workspace.toml");
    let mut state = WorkspaceState::default();
    state.set_bookmark(0, &Location::Local(PathBuf::from("/tmp/example")));
    state.record_history(
        &crate::storage::LocationCodec::parse("s3://profile/bucket/folder").unwrap(),
    );
    state.session = Some(SessionState {
        panes: [
            PaneSession {
                tabs: vec![TabState {
                    location: StoredLocation::Local(PathBuf::from("/tmp/example")),
                    sort: SortSpec {
                        mode: SortMode::Size,
                        reverse: true,
                        directories_first: false,
                    },
                }],
                active_tab: 0,
            },
            PaneSession {
                tabs: vec![TabState {
                    location: StoredLocation::Remote("s3://profile/bucket/folder".to_owned()),
                    sort: SortSpec::default(),
                }],
                active_tab: 0,
            },
        ],
        active_pane: 1,
        synchronized_scrolling: true,
        comparison: true,
        console_view: ConsoleViewState::Small,
    });

    state.save(&path).unwrap();
    let restored = WorkspaceState::load(&path).unwrap();
    assert_eq!(restored.bookmark(0).unwrap().display(), "/tmp/example");
    assert_eq!(restored.history[0].display(), "s3://profile/bucket/folder");
    let session = restored.session.unwrap();
    assert_eq!(session.active_pane, 1);
    assert_eq!(session.panes[0].tabs[0].sort.mode, SortMode::Size);
    assert!(session.synchronized_scrolling);
    assert!(session.comparison);
}

#[test]
fn workspace_file_contains_no_credential_fields() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("workspace.toml");
    let mut state = WorkspaceState::default();
    state.set_bookmark(
        0,
        &crate::storage::LocationCodec::parse("s3://profile/bucket").unwrap(),
    );
    state.save(&path).unwrap();
    let text = fs::read_to_string(path).unwrap().to_ascii_lowercase();
    for forbidden in ["secret", "password", "access_key", "token", "private_key"] {
        assert!(
            !text.contains(forbidden),
            "{forbidden} leaked into workspace"
        );
    }
}

#[test]
fn bandwidth_limit_survives_save_reload() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("workspace.toml");
    let state = WorkspaceState {
        bandwidth_limit: 52_428_800, // 50 MB/s
        ..Default::default()
    };
    state.save(&path).unwrap();
    let restored = WorkspaceState::load(&path).unwrap();
    assert_eq!(restored.bandwidth_limit, 52_428_800);
}

#[test]
fn close_at_preserves_or_adjusts_active_tab() {
    let mut tabs = PaneTabs::new(Pane::new(Location::Local(PathBuf::from("/one"))));
    tabs.open_tab();
    tabs.location = Location::Local(PathBuf::from("/two"));
    tabs.open_tab();
    tabs.location = Location::Local(PathBuf::from("/three"));

    assert_eq!(tabs.active_tab(), 2);
    assert!(tabs.close_at(0));
    assert_eq!(tabs.active_tab(), 1);
    assert_eq!(tabs.display_path(), "/three");

    assert!(tabs.close_at(1));
    assert_eq!(tabs.active_tab(), 0);
    assert_eq!(tabs.display_path(), "/two");
    assert!(!tabs.close_at(0));
    assert!(!tabs.close_at(5));
}
