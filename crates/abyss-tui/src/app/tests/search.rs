use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::dialogs::{FindDialog, Modal};
use crate::app::menu::MenuAction;
use crate::app::state::App;
use crate::search::{SearchHit, SearchKind};
use crate::test_support::TempDir;

fn hits() -> Vec<SearchHit> {
    vec![
        SearchHit {
            path: PathBuf::from("/tmp/project/src/main.rs"),
            line: Some(12),
            preview: "fn main() {".to_owned(),
        },
        SearchHit {
            path: PathBuf::from("/tmp/project/src/lib.rs"),
            line: Some(3),
            preview: "pub fn helper() {".to_owned(),
        },
        SearchHit {
            path: PathBuf::from("/tmp/project/README.md"),
            line: Some(1),
            preview: "# Project".to_owned(),
        },
    ]
}

#[test]
fn the_filter_narrows_results_without_searching_again() {
    let mut dialog = FindDialog::new("fn".to_owned(), SearchKind::Contents, hits(), false);
    assert_eq!(dialog.matches().len(), 3);

    for character in "helper".chars() {
        dialog.insert(character);
    }
    let matches = dialog.matches();
    assert_eq!(matches.len(), 1, "only lib.rs previews mention helper");
    assert!(dialog.hits[matches[0]].path.ends_with("lib.rs"));
}

#[test]
fn the_filter_also_matches_on_path() {
    let mut dialog = FindDialog::new("fn".to_owned(), SearchKind::Contents, hits(), false);
    for character in "README".chars() {
        dialog.insert(character);
    }
    assert_eq!(dialog.matches().len(), 1);
}

#[test]
fn backspace_widens_the_filter_again() {
    let mut dialog = FindDialog::new("fn".to_owned(), SearchKind::Contents, hits(), false);
    for character in "helper".chars() {
        dialog.insert(character);
    }
    assert_eq!(dialog.matches().len(), 1);
    for _ in 0..6 {
        dialog.backspace();
    }
    assert_eq!(dialog.matches().len(), 3);
    assert!(dialog.text().is_empty());
}

#[test]
fn selection_stays_inside_the_filtered_list() {
    let mut dialog = FindDialog::new("fn".to_owned(), SearchKind::Contents, hits(), false);
    dialog.move_selection(10);
    assert_eq!(dialog.selected, 2, "cannot move past the last result");
    dialog.move_selection(-10);
    assert_eq!(dialog.selected, 0, "cannot move above the first");
    assert!(dialog.current().is_some());
}

#[test]
fn a_filter_matching_nothing_has_no_current_selection() {
    let mut dialog = FindDialog::new("fn".to_owned(), SearchKind::Contents, hits(), false);
    for character in "zzzzzz".chars() {
        dialog.insert(character);
    }
    assert!(dialog.matches().is_empty());
    assert!(dialog.current().is_none(), "nothing to open");
    // Moving the selection on an empty list must not panic or drift.
    dialog.move_selection(1);
    assert_eq!(dialog.selected, 0);
}

#[test]
fn the_title_names_the_search_and_flags_truncation() {
    let full = FindDialog::new("todo".to_owned(), SearchKind::Contents, hits(), false);
    assert!(full.title().contains("Grep in Tree"));
    assert!(full.title().contains("todo"));
    assert!(!full.title().contains('+'));

    let capped = FindDialog::new("todo".to_owned(), SearchKind::Files, hits(), true);
    assert!(capped.title().contains("Find Files"));
    assert!(
        capped.title().contains('+'),
        "a capped result set should say so: {}",
        capped.title()
    );
}

#[test]
fn f_and_g_open_the_search_prompts() {
    let temp = TempDir::new();
    fs::write(temp.path().join("file.txt"), b"contents").unwrap();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());

    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(
        matches!(app.modal, Some(Modal::Input(_))),
        "f should ask what to find"
    );
    app.modal = None;

    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert!(
        matches!(app.modal, Some(Modal::Input(_))),
        "g should ask what to grep for"
    );
}

#[test]
fn searching_is_offered_only_for_local_directories() {
    let temp = TempDir::new();
    let app = App::new(temp.path().to_owned(), temp.path().to_owned());
    assert!(app.menu_action_available(MenuAction::FindFiles));
    assert!(app.menu_action_available(MenuAction::GrepTree));
}
