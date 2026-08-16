use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::state::App;
use crate::console::{Console, ConsoleView, encode_key};
use crate::storage::Location;

impl App {
    /// Directory a newly spawned shell should start in.
    fn console_start_directory(&self) -> Option<PathBuf> {
        match &self.panes[self.active].location {
            Location::Local(path) => Some(path.clone()),
            Location::Remote(_) => None,
        }
    }

    /// Ensure the shell exists, spawning it on first use.
    ///
    /// Returns `false` when it could not start, leaving the console hidden.
    fn ensure_console(&mut self) -> bool {
        if self.console.is_some() {
            return true;
        }
        let directory = self.console_start_directory();
        let (rows, cols) = self.console_size();
        match Console::spawn(directory.as_deref(), rows, cols) {
            Ok(console) => {
                self.console = Some(console);
                true
            }
            Err(error) => {
                self.show_error("Console", error);
                self.console_view = ConsoleView::Hidden;
                false
            }
        }
    }

    /// Rows and columns available to the emulator for the current view.
    fn console_size(&self) -> (u16, u16) {
        let area = self.layout.console;
        if area.width == 0 || area.height == 0 {
            // Before the first draw, guess something usable; the next frame
            // resizes both the pty and the emulator to the real geometry.
            return (crate::console::SMALL_ROWS, 80);
        }
        (area.height, area.width)
    }

    /// `Ctrl+X` — step the console through hidden → small → full → hidden.
    pub(crate) fn cycle_console(&mut self) {
        let next = self.console_view.next();
        self.console_view = next;
        if next.is_visible() {
            if !self.ensure_console() {
                return;
            }
            if let Some(console) = self.console.as_mut() {
                console.focused = true;
                console.reset_scroll();
            }
        } else if let Some(console) = self.console.as_mut() {
            console.focused = false;
        }
    }

    /// `c` — open the console if hidden, and put the cursor in it either way.
    pub(crate) fn focus_console(&mut self) {
        if !self.console_view.is_visible() {
            self.console_view = ConsoleView::Small;
            if !self.ensure_console() {
                return;
            }
        } else if !self.ensure_console() {
            return;
        }
        if let Some(console) = self.console.as_mut() {
            console.focused = true;
            console.reset_scroll();
        }
    }

    pub(crate) fn console_focused(&self) -> bool {
        self.console_view.is_visible() && self.console.as_ref().is_some_and(|c| c.focused)
    }

    /// Route a key press to the shell, keeping pane navigation for ourselves.
    ///
    /// Returns `true` when the console consumed the key.
    pub(crate) fn handle_console_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            // Leaves the console on screen but hands the keyboard back.
            KeyCode::Esc => {
                if let Some(console) = self.console.as_mut() {
                    console.focused = false;
                }
                self.set_status("Console unfocused — press c to return");
                return true;
            }
            // Pane navigation still works while typing, as it does in MC.
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::Tab => {
                return false;
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                // With the panes hidden there is nothing else these could
                // drive, so they scroll the shell's scrollback instead.
                if self.console_view == ConsoleView::Full {
                    let delta = if key.code == KeyCode::PageUp { 1 } else { -1 };
                    let rows = self.layout.console.height.max(1) as isize;
                    if let Some(console) = self.console.as_mut() {
                        console.scroll(delta * rows);
                    }
                    return true;
                }
                return false;
            }
            _ => {}
        }

        let Some(bytes) = encode_key(key) else {
            return false;
        };
        if let Some(console) = self.console.as_mut() {
            console.reset_scroll();
            console.write(&bytes);
        }
        true
    }

    /// Match the pty and emulator to the area the last frame gave the console.
    pub(crate) fn resize_console(&mut self) {
        let area = self.layout.console;
        if area.width == 0 || area.height == 0 {
            return;
        }
        if let Some(console) = self.console.as_mut() {
            console.resize(area.height, area.width);
        }
    }

    /// Mirror a pane move into the shell so both agree on the directory.
    pub(crate) fn sync_console_directory(&mut self) {
        if self.console.is_none() {
            return;
        }
        let Location::Local(path) = self.panes[self.active].location.clone() else {
            return;
        };
        if let Some(console) = self.console.as_mut() {
            console.change_directory(&path);
        }
    }

    /// Follow a `cd` typed in the shell by moving the active pane.
    pub(crate) fn poll_console_directory(&mut self) {
        let Some(directory) = self
            .console
            .as_mut()
            .and_then(Console::take_shell_directory)
        else {
            return;
        };
        if !directory.is_dir() {
            return;
        }
        if self.panes[self.active].location == Location::Local(directory.clone()) {
            return;
        }
        self.panes[self.active].change_directory(self.active, directory, &self.browser);
        self.record_active_location_silently();
    }

    /// Drain shell output; reports whether the frame needs redrawing.
    pub(crate) fn poll_console(&mut self) -> bool {
        let Some(console) = self.console.as_mut() else {
            return false;
        };
        let changed = console.drain(64);
        if console.has_exited() {
            self.console = None;
            self.console_view = ConsoleView::Hidden;
            self.set_status("Console shell exited");
            return true;
        }
        if changed {
            self.poll_console_directory();
        }
        changed
    }
}
