mod command;
mod emulator;
mod keys;
mod rc;
mod session;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::workspace::ConsoleViewState;

use self::emulator::OscCallbacks;
pub(crate) use self::keys::encode_key;
use self::session::PtySession;

/// Rows of shell output shown in the small view, excluding the border.
pub(crate) const SMALL_ROWS: u16 = 3;
/// Lines of shell output retained above the visible screen.
const SCROLLBACK: usize = 5_000;

/// How much of the screen the console occupies.
///
/// `Ctrl+X` steps through these in order and wraps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ConsoleView {
    #[default]
    Hidden,
    Small,
    Full,
}

impl From<ConsoleViewState> for ConsoleView {
    fn from(value: ConsoleViewState) -> Self {
        match value {
            ConsoleViewState::Hidden => Self::Hidden,
            ConsoleViewState::Small => Self::Small,
            ConsoleViewState::Full => Self::Full,
        }
    }
}

impl From<ConsoleView> for ConsoleViewState {
    fn from(value: ConsoleView) -> Self {
        match value {
            ConsoleView::Hidden => Self::Hidden,
            ConsoleView::Small => Self::Small,
            ConsoleView::Full => Self::Full,
        }
    }
}

impl ConsoleView {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Hidden => Self::Small,
            Self::Small => Self::Full,
            Self::Full => Self::Hidden,
        }
    }

    pub(crate) fn is_visible(self) -> bool {
        !matches!(self, Self::Hidden)
    }

    /// Rows the console needs, including its one-row border.
    pub(crate) fn rows(self, available: u16) -> u16 {
        match self {
            Self::Hidden => 0,
            Self::Small => (SMALL_ROWS + 1).min(available),
            Self::Full => available,
        }
    }
}

/// A persistent `$SHELL` on a pty, plus the terminal emulator that renders it.
pub(crate) struct Console {
    session: PtySession,
    parser: vt100::Parser<OscCallbacks>,
    pub(crate) focused: bool,
    /// Directory we pushed to the shell ourselves, so the OSC 7 it echoes
    /// back does not bounce the pane straight back again.
    suppressed: Option<PathBuf>,
    size: (u16, u16),
}

impl Console {
    /// Spawn the shell. `directory` seeds its working directory.
    pub(crate) fn spawn(directory: Option<&Path>, rows: u16, cols: u16) -> Result<Self, String> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let session = PtySession::spawn(directory, rows, cols)?;
        Ok(Self {
            session,
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                SCROLLBACK,
                OscCallbacks::default(),
            ),
            focused: true,
            suppressed: None,
            size: (rows, cols),
        })
    }

    pub(crate) fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Feed any pending shell output into the emulator.
    ///
    /// Returns `true` when something arrived and the frame needs redrawing.
    pub(crate) fn drain(&mut self, budget: usize) -> bool {
        let mut changed = false;
        for _ in 0..budget {
            let Some(chunk) = self.session.try_recv() else {
                break;
            };
            self.parser.process(&chunk);
            changed = true;
        }
        changed
    }

    /// The shell's own working directory, once it has reported one.
    ///
    /// Returns `None` while it matches a directory we pushed ourselves.
    pub(crate) fn take_shell_directory(&mut self) -> Option<PathBuf> {
        let reported = self.parser.callbacks_mut().take_cwd()?;
        if self.suppressed.as_ref() == Some(&reported) {
            self.suppressed = None;
            return None;
        }
        self.suppressed = None;
        Some(reported)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        self.session.write(bytes);
    }

    /// Move the shell to `directory` without recording it in shell history.
    pub(crate) fn change_directory(&mut self, directory: &Path) {
        let Some(command) = command::cd_command(directory) else {
            return;
        };
        self.suppressed = Some(directory.to_owned());
        self.session.write(command.as_bytes());
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.parser.screen_mut().set_size(rows, cols);
        self.session.resize(rows, cols);
    }

    pub(crate) fn scroll(&mut self, delta: isize) {
        let current = self.parser.screen().scrollback() as isize;
        let next = (current + delta).max(0) as usize;
        self.parser.screen_mut().set_scrollback(next);
    }

    pub(crate) fn reset_scroll(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    /// True once the shell has exited, so the console can be torn down.
    pub(crate) fn has_exited(&mut self) -> bool {
        self.session.has_exited()
    }
}
