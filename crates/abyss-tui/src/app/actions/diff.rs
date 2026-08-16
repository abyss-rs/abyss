use crossterm::event::KeyCode;

use crate::app::state::{App, DiffView};
use crate::diff::diff_files;
use crate::storage::Location;

impl App {
    /// Diff the selected file against the same-named file in the other pane.
    ///
    /// Falls back to the other pane's own selection when there is no matching
    /// name, which is what you want when comparing two differently named files.
    pub(crate) fn diff_with_other_pane(&mut self) {
        let other = 1 - self.active;
        let Some(Location::Local(left)) = self.panes[self.active].current_location() else {
            self.set_status("Diff needs a local file in the active pane");
            return;
        };
        if left.is_dir() {
            self.set_status("Select a file to diff, not a directory");
            return;
        }

        let name = left.file_name().map(std::ffi::OsStr::to_os_string);
        let counterpart = name
            .as_ref()
            .map(|name| match &self.panes[other].location {
                Location::Local(directory) => directory.join(name),
                Location::Remote(_) => left.clone(),
            })
            .filter(|candidate| candidate.is_file() && *candidate != left);

        let right = match counterpart {
            Some(path) => path,
            None => match self.panes[other].current_location() {
                Some(Location::Local(path)) if path.is_file() && path != left => path,
                _ => {
                    self.set_status("No file in the other pane to diff against");
                    return;
                }
            },
        };

        match diff_files(&left, &right) {
            Ok(diff) if diff.identical => {
                self.set_status("The two files are identical");
            }
            Ok(diff) => {
                self.diff = Some(DiffView {
                    left_name: display_name(&left),
                    right_name: display_name(&right),
                    diff,
                    vertical: 0,
                    horizontal: 0,
                });
                self.clear_status();
            }
            Err(error) => self.show_error("Diff", error),
        }
    }

    pub(crate) fn handle_diff_key(&mut self, key: crossterm::event::KeyEvent) {
        let rows = self.pane_rows.max(1);
        let Some(view) = self.diff.as_mut() else {
            return;
        };
        let last = view.diff.lines.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.diff = None,
            KeyCode::Up => view.vertical = view.vertical.saturating_sub(1),
            KeyCode::Down => view.vertical = (view.vertical + 1).min(last),
            KeyCode::PageUp => view.vertical = view.vertical.saturating_sub(rows),
            KeyCode::PageDown => view.vertical = (view.vertical + rows).min(last),
            KeyCode::Home => view.vertical = 0,
            KeyCode::End => view.vertical = last,
            KeyCode::Left => view.horizontal = view.horizontal.saturating_sub(4),
            KeyCode::Right => view.horizontal = view.horizontal.saturating_add(4),
            _ => {}
        }
    }
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl App {
    /// Open the system monitor, sampling the machine immediately.
    pub(crate) fn open_monitor(&mut self) {
        self.monitor = Some(crate::monitor::Monitor::new());
        self.clear_status();
    }
}
