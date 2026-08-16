use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::actions::fuzzy_matches;
use crate::app::dialogs::Modal;
use crate::app::menu::{BookmarkFocus, MenuAction, MenuCategory};
use crate::app::state::{App, Difference};
use crate::app::sync::{SyncDirection, SyncFilterMode};
use crate::storage::Location;
use crate::sync::{SyncComparison, SyncStrategy};
use crate::test_support::TempDir;

#[test]
fn ctrl_z_opens_and_navigates_the_categorized_menu_bar() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());

    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(app.app_menu.unwrap().category, MenuCategory::Navigate);

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.app_menu.unwrap().category, MenuCategory::Pane);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.app_menu.is_none());
    assert_eq!(app.panes[0].tab_count(), 2);

    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert!(app.app_menu.is_none());
}

#[test]
fn menu_inventory_exposes_hidden_commands_without_numbered_buttons() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.workspace
        .record_history(&Location::Local(temp.path().to_owned()));

    assert_eq!(
        app.visible_menu_actions(MenuCategory::Navigate),
        vec![MenuAction::DirectoryHistory, MenuAction::SmartJump]
    );
    assert_eq!(
        app.visible_menu_actions(MenuCategory::Pane),
        vec![
            MenuAction::NewTab,
            MenuAction::SynchronizedScrolling,
            MenuAction::DirectoryComparison,
        ]
    );
    let tools = app.visible_menu_actions(MenuCategory::Tools);
    assert!(tools.contains(&MenuAction::OpenSubshell));
    assert!(tools.contains(&MenuAction::Inspect));
    assert!(tools.contains(&MenuAction::DifferentialSync));
    assert!(!tools.contains(&MenuAction::Jobs));
    #[cfg(feature = "kubernetes")]
    assert!(!tools.contains(&MenuAction::VolumeSnapshot));

    for category in MenuCategory::ALL {
        for action in MenuAction::for_category(category) {
            assert!(!matches!(
                action.label(),
                "Help"
                    | "View"
                    | "Mkdir"
                    | "Copy"
                    | "Move"
                    | "Delete"
                    | "Refresh"
                    | "Quit"
                    | "Sources"
                    | "Sort"
            ));
        }
    }
}

#[test]
fn menu_visibility_tracks_tabs_history_and_toggles() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    assert_eq!(
        app.visible_menu_actions(MenuCategory::Navigate),
        vec![MenuAction::SmartJump]
    );
    assert!(
        !app.visible_menu_actions(MenuCategory::Pane)
            .contains(&MenuAction::CloseTab)
    );

    app.perform_menu_action(MenuAction::NewTab);
    assert!(
        app.visible_menu_actions(MenuCategory::Pane)
            .contains(&MenuAction::CloseTab)
    );
    assert!(!app.menu_action_checked(MenuAction::SynchronizedScrolling));
    app.perform_menu_action(MenuAction::SynchronizedScrolling);
    assert!(app.menu_action_checked(MenuAction::SynchronizedScrolling));
    app.perform_menu_action(MenuAction::DirectoryComparison);
    assert!(app.menu_action_checked(MenuAction::DirectoryComparison));
}

#[test]
fn bookmark_menu_uses_jump_and_set_focus_without_trapping_category_navigation() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    let unicode = Location::Local(temp.path().join("資料").join("café"));
    app.workspace.set_bookmark(1, &unicode);
    assert_eq!(app.bookmark_display(1), Some(unicode.display()));
    app.open_menu_category(MenuCategory::Bookmarks);
    assert_eq!(app.app_menu.unwrap().bookmark_focus, BookmarkFocus::Set);

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let menu = app.app_menu.unwrap();
    assert_eq!(menu.selected, 1);
    assert_eq!(menu.bookmark_focus, BookmarkFocus::Jump);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.app_menu.unwrap().bookmark_focus, BookmarkFocus::Set);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.app_menu.unwrap().category, MenuCategory::Tools);
}

#[test]
fn history_menu_action_opens_the_existing_history_dialog() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.workspace
        .record_history(&Location::Local(temp.path().to_owned()));

    app.perform_menu_action(MenuAction::DirectoryHistory);

    assert!(matches!(app.modal, Some(Modal::History(_))));
}

#[test]
fn sources_open_with_s_and_restore_with_escape() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    let original_location = app.panes[0].location.clone();
    let original_generation = app.panes[0].generation;

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(app.panes[0].showing_sources());
    assert!(!app.panes[1].showing_sources());
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.active, 1);
    assert!(app.panes[0].showing_sources());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(app.panes[1].showing_sources());
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!app.panes[1].showing_sources());
    assert!(app.panes[0].showing_sources());
    assert_eq!(app.panes[0].location, original_location);
    assert_eq!(app.panes[0].generation, original_generation);
}

#[test]
fn analyze_opens_on_four_and_leaves_on_escape() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());

    app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
    assert!(app.analyze.is_some());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.analyze.is_none());
}

#[test]
fn analyze_clean_opens_confirmation_popup() {
    let temp = TempDir::new();
    let target = temp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("a.o"), b"obj").unwrap();

    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
    assert!(app.analyze.is_some());

    for _ in 0..500 {
        let ready = app.analyze.as_ref().is_some_and(|session| {
            !matches!(
                session.clean_offer(),
                cleaner_tui::CleanOffer::Unavailable(_)
            )
        });
        if ready {
            break;
        }
        if let Some(session) = app.analyze.as_mut() {
            session.tick();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
    let ok = match &app.modal {
        Some(Modal::ConfirmClean { .. }) => true,
        Some(Modal::Message { title, .. }) => title == "Clean",
        _ => false,
    };
    assert!(ok, "expected clean confirmation popup");

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.analyze.is_some());
}

#[test]
fn analyze_refuses_when_sources_are_open() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(app.panes[0].showing_sources());

    app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
    assert!(app.analyze.is_none());
    assert!(
        app.status.contains("unavailable") || app.status.contains("local-only"),
        "unexpected status: {}",
        app.status
    );
}

#[test]
fn quit_uses_zero_not_three() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert!(app.sync.is_some());
    // 0 in Sync mode exits sync mode back to normal dual-pane
    app.handle_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert!(app.sync.is_none());
    // 0 in normal mode quits the application
    app.handle_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn sync_mode_opens_on_three_and_leaves_on_escape_or_zero() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert!(app.sync.is_some());

    // Toggle filter with 7
    app.handle_key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
    assert_eq!(
        app.sync.as_ref().unwrap().filter,
        SyncFilterMode::ChangesOnly
    );

    // Swap direction with 5
    app.handle_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE));
    assert_eq!(
        app.sync.as_ref().unwrap().direction,
        SyncDirection::RightToLeft
    );

    // Cycle comparison with 6
    app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
    assert_eq!(
        app.sync.as_ref().unwrap().comparison,
        SyncComparison::Checksum
    );

    // Cycle strategy with 8
    app.handle_key(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE));
    assert_eq!(
        app.sync.as_ref().unwrap().strategy,
        SyncStrategy::UpdateOnly
    );

    // Esc leaves sync mode
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.sync.is_none());
}

#[test]
fn file_commands_are_disabled_while_sources_are_visible() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert!(app.status.contains("unavailable"));
    app.handle_key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
    assert!(app.panes[0].marks.is_empty());
}

#[test]
fn pane_tabs_are_independent_and_cycle() {
    let temp = TempDir::new();
    let other = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());

    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert_eq!(app.panes[0].tab_count(), 2);
    assert_eq!(app.panes[1].tab_count(), 1);
    app.navigate_active_to(Location::Local(other.path().to_owned()));
    assert_eq!(
        app.panes[0].location,
        Location::Local(other.path().to_owned())
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    assert_eq!(
        app.panes[0].location,
        Location::Local(temp.path().to_owned())
    );
    app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(
        app.panes[0].location,
        Location::Local(other.path().to_owned())
    );
}

#[test]
fn bookmarks_jump_without_affecting_the_other_pane() {
    let temp = TempDir::new();
    let destination = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.workspace
        .set_bookmark(0, &Location::Local(destination.path().to_owned()));

    app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL));

    assert_eq!(
        app.panes[0].location,
        Location::Local(destination.path().to_owned())
    );
    assert_eq!(
        app.panes[1].location,
        Location::Local(temp.path().to_owned())
    );
}

#[test]
fn history_opens_and_filters_by_subsequence() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.workspace
        .record_history(&Location::Local(PathBuf::from("/var/lib/containers")));
    app.workspace
        .record_history(&Location::Local(PathBuf::from("/usr/local/bin")));

    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT));
    assert!(matches!(app.modal, Some(Modal::History(_))));
    assert!(fuzzy_matches("/var/lib/containers", "vlc"));
    assert!(!fuzzy_matches("/usr/local/bin", "xyz"));
}

#[test]
fn local_delete_defaults_to_recoverable_trash() {
    let temp = TempDir::new();
    std::fs::write(temp.path().join("recoverable.txt"), b"data").unwrap();
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
        .position(|entry| entry.name == "recoverable.txt")
        .unwrap();
    app.panes[0].select_index(index, 10);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(matches!(
        app.modal,
        Some(Modal::ConfirmDelete {
            trash_available: true,
            ..
        })
    ));
    assert!(temp.path().join("recoverable.txt").exists());
}

#[test]
fn directory_diff_detects_only_here_and_modified() {
    let left_dir = TempDir::new();
    let right_dir = TempDir::new();
    std::fs::write(left_dir.path().join("file1.txt"), b"data1").unwrap();
    std::fs::write(left_dir.path().join("file2.txt"), b"data2_left").unwrap();
    std::fs::write(
        right_dir.path().join("file2.txt"),
        b"data2_different_length",
    )
    .unwrap();

    let mut app = App::new(left_dir.path().to_owned(), right_dir.path().to_owned());
    for _ in 0..100 {
        app.poll_background();
        if !app.panes[0].loading && !app.panes[1].loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    app.comparison = true;

    let pane0 = &app.panes[0];
    let file1_entry = pane0
        .entries
        .iter()
        .find(|e| e.name == "file1.txt")
        .unwrap();
    assert_eq!(
        app.entry_difference(0, file1_entry),
        Some(Difference::OnlyHere)
    );

    let file2_entry = pane0
        .entries
        .iter()
        .find(|e| e.name == "file2.txt")
        .unwrap();
    assert_eq!(
        app.entry_difference(0, file2_entry),
        Some(Difference::Modified)
    );
}
