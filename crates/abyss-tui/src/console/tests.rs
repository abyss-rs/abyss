use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::command::cd_command;
use super::emulator::{OscCallbacks, parse_osc7};
use super::keys::encode_key;
use super::{ConsoleView, SMALL_ROWS};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn console_view_cycles_through_three_states_and_wraps() {
    let hidden = ConsoleView::Hidden;
    let small = hidden.next();
    let full = small.next();
    assert_eq!(small, ConsoleView::Small);
    assert_eq!(full, ConsoleView::Full);
    assert_eq!(full.next(), ConsoleView::Hidden);
}

#[test]
fn only_the_hidden_view_leaves_the_panes_at_full_height() {
    assert_eq!(ConsoleView::Hidden.rows(20), 0);
    assert_eq!(ConsoleView::Small.rows(20), SMALL_ROWS + 1);
    assert_eq!(ConsoleView::Full.rows(20), 20);
    assert!(!ConsoleView::Hidden.is_visible());
    assert!(ConsoleView::Small.is_visible());
    assert!(ConsoleView::Full.is_visible());
}

#[test]
fn a_short_terminal_cannot_be_given_more_console_than_it_has() {
    assert_eq!(ConsoleView::Small.rows(2), 2);
    assert_eq!(ConsoleView::Full.rows(2), 2);
}

#[test]
fn osc7_payloads_decode_to_local_paths() {
    assert_eq!(
        parse_osc7(b"file://workstation/home/alex"),
        Some(PathBuf::from("/home/alex"))
    );
    // An empty host is what most shells emit when HOST is unset.
    assert_eq!(parse_osc7(b"file:///tmp"), Some(PathBuf::from("/tmp")));
}

#[test]
fn osc7_percent_escapes_are_decoded() {
    assert_eq!(
        parse_osc7(b"file://host/home/alex/My%20Documents"),
        Some(PathBuf::from("/home/alex/My Documents"))
    );
}

#[test]
fn malformed_osc7_payloads_are_ignored_rather_than_panicking() {
    assert_eq!(parse_osc7(b""), None);
    assert_eq!(parse_osc7(b"http://example.com/x"), None);
    assert_eq!(parse_osc7(b"file://host"), None);
    assert_eq!(parse_osc7(b"file://"), None);
    // Invalid UTF-8 after decoding must not be forced through.
    assert_eq!(parse_osc7(b"file://host/%FF%FE"), None);
}

#[test]
fn the_emulator_reports_the_directory_a_shell_announces() {
    let mut parser = vt100::Parser::new_with_callbacks(4, 20, 0, OscCallbacks::default());
    parser.process(b"\x1b]7;file://host/var/log\x07");
    assert_eq!(
        parser.callbacks_mut().take_cwd(),
        Some(PathBuf::from("/var/log"))
    );
    // Taking it consumes it, so an unchanged prompt does not re-navigate.
    assert_eq!(parser.callbacks_mut().take_cwd(), None);
}

#[test]
fn window_titles_are_not_mistaken_for_directories() {
    let mut parser = vt100::Parser::new_with_callbacks(4, 20, 0, OscCallbacks::default());
    parser.process(b"\x1b]2;some title\x07");
    assert_eq!(parser.callbacks_mut().take_cwd(), None);
}

#[test]
fn directory_sync_is_hidden_from_shell_history() {
    let command = cd_command(Path::new("/tmp/project")).expect("utf-8 path");
    assert!(
        command.starts_with(' '),
        "the leading space is what keeps this out of history: {command:?}"
    );
    assert_eq!(command, " cd /tmp/project\n");
}

#[test]
fn directory_sync_quotes_awkward_paths() {
    let command = cd_command(Path::new("/tmp/two words")).expect("utf-8 path");
    assert_eq!(command, " cd '/tmp/two words'\n");
}

/// Spawns a real shell, so it is opt-in: CI sandboxes cannot be relied on to
/// provide a usable `$SHELL` or a pty. Run with
/// `cargo test -p abyss-tui --all-features -- --ignored console_shell`.
#[test]
#[ignore = "spawns a real shell on a pty"]
fn console_shell_runs_commands_and_reports_its_directory() {
    use std::time::{Duration, Instant};

    let directory = std::env::temp_dir();
    let mut console =
        super::Console::spawn(Some(&directory), 24, 80).expect("spawn a shell on a pty");

    // A command whose output we can recognise in the rendered screen.
    console.write(b"echo abyss-console-marker\n");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    while Instant::now() < deadline {
        console.drain(64);
        let text = console.screen().contents();
        // The echoed command itself also contains the marker, so require the
        // output line: two occurrences, or one on a line of its own.
        if text.matches("abyss-console-marker").count() >= 2 {
            seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        seen,
        "the shell never echoed the command back:\n{}",
        console.screen().contents()
    );
}

/// Proves the injected rc hook works against the user's real shell, which is
/// the half of cwd sync that cannot be unit tested. Opt-in for the same reason
/// as the test above.
#[test]
#[ignore = "spawns a real shell on a pty"]
fn console_shell_reports_directory_changes_through_osc7() {
    use std::time::{Duration, Instant};

    let start = std::env::temp_dir();
    let mut console = super::Console::spawn(Some(&start), 24, 80).expect("spawn a shell on a pty");

    // Give the first prompt a chance to land before changing anything.
    let settle = Instant::now() + Duration::from_secs(3);
    while Instant::now() < settle {
        console.drain(64);
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = console.take_shell_directory();

    let target = std::env::temp_dir();
    console.write(format!(" cd {}\n", target.display()).as_bytes());

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut reported = None;
    while Instant::now() < deadline && reported.is_none() {
        console.drain(64);
        reported = console.take_shell_directory();
        std::thread::sleep(Duration::from_millis(50));
    }

    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.ends_with("zsh") || shell.ends_with("bash") {
        let reported = reported.expect("zsh and bash both get an injected OSC 7 hook");
        assert!(
            reported.is_dir(),
            "the shell reported a directory that does not exist: {reported:?}"
        );
    }
}

#[test]
fn control_keys_become_the_bytes_a_shell_expects() {
    assert_eq!(
        encode_key(key_with(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(vec![0x03])
    );
    assert_eq!(
        encode_key(key_with(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        Some(vec![0x04])
    );
    // Ctrl+A must reach readline as 0x01, not as the letter.
    assert_eq!(
        encode_key(key_with(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        Some(vec![0x01])
    );
}

#[test]
fn navigation_keys_use_standard_escape_sequences() {
    assert_eq!(encode_key(key(KeyCode::Up)), Some(b"\x1b[A".to_vec()));
    assert_eq!(encode_key(key(KeyCode::Down)), Some(b"\x1b[B".to_vec()));
    assert_eq!(encode_key(key(KeyCode::Home)), Some(b"\x1b[H".to_vec()));
    assert_eq!(encode_key(key(KeyCode::Delete)), Some(b"\x1b[3~".to_vec()));
    assert_eq!(encode_key(key(KeyCode::F(1))), Some(b"\x1bOP".to_vec()));
}

#[test]
fn alt_is_sent_as_an_escape_prefix() {
    assert_eq!(
        encode_key(key_with(KeyCode::Char('f'), KeyModifiers::ALT)),
        Some(b"\x1bf".to_vec())
    );
}

#[test]
fn ordinary_and_multibyte_characters_pass_through() {
    assert_eq!(encode_key(key(KeyCode::Char('l'))), Some(b"l".to_vec()));
    assert_eq!(encode_key(key(KeyCode::Enter)), Some(vec![b'\r']));
    assert_eq!(encode_key(key(KeyCode::Backspace)), Some(vec![0x7f]));
    assert_eq!(
        encode_key(key(KeyCode::Char('é'))),
        Some("é".as_bytes().to_vec())
    );
}
