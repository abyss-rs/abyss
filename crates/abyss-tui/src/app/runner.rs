use std::ffi::OsString;
use std::fs;
use std::io::{self, Write, stdout};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::time::Duration;

use crossterm::cursor::{MoveToColumn, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::style::ResetColor;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
use ratatui::DefaultTerminal;

use crate::Error;
use crate::app::App;
use crate::storage::{Location, LocationCodec};
use crate::workspace::WorkspaceState;

#[cfg(unix)]
pub(crate) fn os_string_from_external(value: Vec<u8>) -> OsString {
    OsString::from_vec(value)
}

#[cfg(windows)]
pub(crate) fn os_string_from_external(value: Vec<u8>) -> OsString {
    match String::from_utf8(value) {
        Ok(value) => value.into(),
        Err(error) => format!(
            "<raw:{}>",
            percent_encoding::percent_encode(error.as_bytes(), percent_encoding::NON_ALPHANUMERIC,)
        )
        .into(),
    }
}

pub fn run(left: Option<String>, right: Option<String>) -> Result<(), Error> {
    let t0 = std::time::Instant::now();
    let profile = std::env::var_os("ABYSS_PROFILE").is_some();
    let left = left.as_deref().map(starting_location).transpose()?;
    let right = right.as_deref().map(starting_location).transpose()?;
    let (workspace, workspace_warning) = WorkspaceState::load_default();
    if profile {
        eprintln!("[PROFILE WorkspaceState::load_default]: {:?}", t0.elapsed());
    }
    if false {
        return Err(Error::message("the TUI requires an interactive terminal"));
    }

    let t_init = std::time::Instant::now();
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(Error::io("initialize terminal for", ".", error));
        }
    };
    if profile {
        eprintln!("[PROFILE ratatui::try_init]: {:?}", t_init.elapsed());
    }
    if let Err(error) = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste) {
        let _ = restore_terminal(terminal);
        return Err(Error::io("enable terminal input for", ".", error));
    }

    let t_app = std::time::Instant::now();
    let app = App::from_workspace(left, right, workspace, workspace_warning);
    if profile {
        eprintln!("[PROFILE App::from_workspace]: {:?}", t_app.elapsed());
    }
    let result = if profile {
        app.run_profile(&mut terminal, true, t0)
    } else {
        app.run(&mut terminal)
    };
    let cleanup = restore_terminal(terminal);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(Error::io("restore terminal after", ".", error)),
        (Ok(()), Ok(())) => Ok(()),
    }
}

pub(crate) fn restore_terminal(mut terminal: DefaultTerminal) -> io::Result<()> {
    let mut first_error = None;
    remember_cleanup_error(
        &mut first_error,
        execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            DisableBracketedPaste,
            ResetColor,
            Show
        ),
    );
    remember_cleanup_error(&mut first_error, terminal.backend_mut().flush());
    drain_terminal_input();
    remember_cleanup_error(&mut first_error, disable_raw_mode());
    remember_cleanup_error(
        &mut first_error,
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            ResetColor,
            Show,
            MoveToColumn(0)
        ),
    );
    remember_cleanup_error(&mut first_error, terminal.backend_mut().flush());
    drop(terminal);
    first_error.map_or(Ok(()), Err)
}

fn drain_terminal_input() {
    while matches!(event::poll(Duration::ZERO), Ok(true)) {
        if event::read().is_err() {
            break;
        }
    }
}

fn remember_cleanup_error(slot: &mut Option<io::Error>, result: io::Result<()>) {
    if let Err(error) = result
        && slot.is_none()
    {
        *slot = Some(error);
    }
}

fn starting_location(value: &str) -> Result<Location, Error> {
    let location =
        LocationCodec::parse(value).map_err(|error| Error::message(error.to_string()))?;
    let Location::Local(path) = location else {
        return Ok(location);
    };
    let path = if path.as_os_str().is_empty() {
        std::env::current_dir()
            .map_err(|error| Error::io("read current directory for", ".", error))?
    } else if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| Error::io("read current directory for", &path, error))?
            .join(path)
    };
    let metadata = fs::metadata(&path)
        .map_err(|error| Error::io("inspect starting directory", &path, error))?;
    if !metadata.is_dir() {
        return Err(Error::message(format!(
            "starting pane is not a directory: {}",
            path.display()
        )));
    }
    Ok(Location::Local(path))
}
