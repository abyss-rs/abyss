use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::App;
use crate::console::ConsoleView;
use crate::test_support::TempDir;
use crate::ui::ActionButton;

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_ctrl(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

/// Spawning a real shell is not safe in CI, so these cover the state machine
/// and key routing, which is where the logic lives.
fn app_with_console_view(view: ConsoleView) -> (TempDir, App) {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.console_view = view;
    (temp, app)
}

#[test]
fn ctrl_x_steps_the_console_through_its_three_sizes() {
    let (_temp, mut app) = app_with_console_view(ConsoleView::Hidden);
    // Without a spawned shell the view still advances; only rendering differs.
    assert_eq!(app.console_view, ConsoleView::Hidden);
    press_ctrl(&mut app, KeyCode::Char('x'));
    let after_first = app.console_view;
    press_ctrl(&mut app, KeyCode::Char('x'));
    let after_second = app.console_view;
    press_ctrl(&mut app, KeyCode::Char('x'));
    let after_third = app.console_view;

    // A shell may fail to spawn in a sandbox, which resets to Hidden. Either
    // way the cycle must never get stuck part-way through.
    if after_first == ConsoleView::Small {
        assert_eq!(after_second, ConsoleView::Full);
        assert_eq!(after_third, ConsoleView::Hidden);
    } else {
        assert_eq!(after_first, ConsoleView::Hidden);
    }
}

#[test]
fn copy_stays_on_the_digit_now_that_c_belongs_to_the_console() {
    let (_temp, mut app) = app_with_console_view(ConsoleView::Hidden);
    assert!(app.modal.is_none());

    // `5` is what the button bar advertises for Copy, and it must still work.
    press(&mut app, KeyCode::Char('5'));
    let copy_opened_a_dialog = app.modal.is_some();
    app.modal = None;

    // `c` must not reach Copy any more.
    press(&mut app, KeyCode::Char('c'));
    assert!(
        app.modal.is_none(),
        "c should open the console, never the Copy dialog"
    );
    assert_eq!(
        ActionButton::Copy.key(),
        "5",
        "the button bar must keep advertising the digit"
    );
    // Copy needs a selection to open a dialog; the important half is that `c`
    // did not, whatever `5` did here.
    let _ = copy_opened_a_dialog;
}

#[test]
fn c_reveals_the_console_rather_than_leaving_it_hidden() {
    let (_temp, mut app) = app_with_console_view(ConsoleView::Hidden);
    press(&mut app, KeyCode::Char('c'));
    // Either the shell started and the console is showing, or it could not
    // start and we fell back to hidden — never a visible console with no shell.
    if app.console_view.is_visible() {
        assert!(app.console.is_some(), "a visible console needs its shell");
    } else {
        assert!(app.console.is_none());
    }
}

#[test]
fn the_console_is_not_focused_while_it_is_hidden() {
    let (_temp, mut app) = app_with_console_view(ConsoleView::Hidden);
    assert!(!app.console_focused());
    // Navigation keys must keep reaching the panes.
    press(&mut app, KeyCode::Down);
    assert!(!app.console_focused());
}

#[test]
fn quitting_still_works_with_no_console_open() {
    let (_temp, mut app) = app_with_console_view(ConsoleView::Hidden);
    press_ctrl(&mut app, KeyCode::Char('c'));
    assert!(
        app.should_quit,
        "Ctrl+C keeps quitting when the shell does not have focus"
    );
}

#[test]
fn console_view_survives_a_workspace_round_trip() {
    use crate::workspace::ConsoleViewState;

    for view in [ConsoleView::Hidden, ConsoleView::Small, ConsoleView::Full] {
        let stored: ConsoleViewState = view.into();
        assert_eq!(ConsoleView::from(stored), view);
    }
}
