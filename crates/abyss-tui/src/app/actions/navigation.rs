use crate::app::dialogs::{HistoryDialog, InputAction, InputDialog, Modal};
use crate::app::menu::SortMenu;
use crate::app::state::App;
use crate::archive::ArchiveRequest;
use crate::browser::{BrowserKind, SortMode, SourceProbeStatus};
use crate::storage::Location;
use crate::workspace::SessionState;

impl App {
    pub(crate) fn move_active_by(&mut self, amount: isize, rows: usize) {
        self.active_pane_mut().move_by(amount, rows);
        self.sync_selection();
    }

    pub(crate) fn sync_selection(&mut self) {
        if !self.synchronized_scrolling {
            return;
        }
        let active = self.active;
        let other = 1 - active;
        let Some(name) = self.panes[active].current().map(|entry| entry.name.clone()) else {
            return;
        };
        if !self.panes[other].select_name(&name, self.pane_rows) {
            let index = self.panes[active].selected;
            self.panes[other].select_index(index, self.pane_rows);
        }
    }

    pub(crate) fn switch_tab(&mut self, amount: isize) {
        self.panes[self.active].switch_by(amount);
        self.panes[self.active].reload(self.active, &self.browser);
        self.record_active_location();
        self.set_status(format!(
            "Tab {}/{}",
            self.panes[self.active].active_tab() + 1,
            self.panes[self.active].tab_count()
        ));
    }

    pub(crate) fn open_directory_history(&mut self) {
        if self.workspace.history.is_empty() {
            self.set_status("Directory history is empty");
        } else {
            self.modal = Some(Modal::History(HistoryDialog::new(
                self.workspace.history.clone(),
            )));
        }
    }

    pub(crate) fn open_smart_jump(&mut self) {
        self.modal = Some(Modal::Input(InputDialog::new(
            "Smart jump",
            "Directory keyword:",
            String::new(),
            InputAction::SmartJump,
        )));
    }

    pub(crate) fn open_new_tab(&mut self) {
        self.panes[self.active].open_tab();
        self.panes[self.active].reload(self.active, &self.browser);
        self.record_active_location();
        self.set_status(format!(
            "Opened tab {}/{}",
            self.panes[self.active].active_tab() + 1,
            self.panes[self.active].tab_count()
        ));
    }

    pub(crate) fn close_active_tab(&mut self) {
        if self.panes[self.active].close_tab() {
            self.panes[self.active].reload(self.active, &self.browser);
            self.set_status(format!(
                "Closed tab; {}/{} active",
                self.panes[self.active].active_tab() + 1,
                self.panes[self.active].tab_count()
            ));
        } else {
            self.set_status("Each pane must keep at least one tab");
        }
    }

    pub(crate) fn toggle_synchronized_scrolling(&mut self) {
        self.synchronized_scrolling = !self.synchronized_scrolling;
        self.set_status(if self.synchronized_scrolling {
            "Synchronized scrolling enabled"
        } else {
            "Synchronized scrolling disabled"
        });
        self.sync_selection();
    }

    pub(crate) fn toggle_directory_comparison(&mut self) {
        self.comparison = !self.comparison;
        self.set_status(if self.comparison {
            "Directory comparison enabled"
        } else {
            "Directory comparison disabled"
        });
    }

    pub(crate) fn assign_bookmark(&mut self, index: usize) {
        let location = self.panes[self.active].location.clone();
        self.workspace.set_bookmark(index, &location);
        if let Err(error) = self.persist_workspace() {
            self.set_status(error);
        } else {
            self.set_status(format!(
                "Bookmark {} set to {}",
                index + 1,
                location.display()
            ));
        }
    }

    pub(crate) fn jump_to_bookmark(&mut self, index: usize) {
        let Some(bookmark) = self.workspace.bookmark(index).cloned() else {
            self.set_status(format!(
                "Bookmark {} is empty; assign it with Ctrl+Shift+{}",
                index + 1,
                index + 1
            ));
            return;
        };
        match bookmark.parse() {
            Ok(location) => self.navigate_active_to(location),
            Err(error) => self.set_status(format!("Bookmark {} is invalid: {error}", index + 1)),
        }
    }

    pub(crate) fn navigate_active_to(&mut self, location: Location) {
        self.record_active_location();
        self.panes[self.active].change_location(self.active, location, &self.browser);
        self.record_active_location();
    }

    pub(crate) fn record_active_location(&mut self) {
        let location = self.panes[self.active].location.clone();
        self.workspace.record_history(&location);
        // Keep the shell on the same directory the user is browsing.
        self.sync_console_directory();
    }

    /// Record history without pushing a `cd` back to the shell.
    ///
    /// Used when the shell itself moved us, where echoing the `cd` back would
    /// print a redundant line into the console for no gain.
    pub(crate) fn record_active_location_silently(&mut self) {
        let location = self.panes[self.active].location.clone();
        self.workspace.record_history(&location);
    }

    pub(crate) fn persist_workspace(&mut self) -> Result<(), String> {
        self.workspace.session = Some(SessionState {
            panes: [self.panes[0].session(), self.panes[1].session()],
            active_pane: self.active,
            synchronized_scrolling: self.synchronized_scrolling,
            comparison: self.comparison,
            console_view: self.console_view.into(),
        });
        self.workspace.save_default()
    }

    pub(crate) fn activate_source(&mut self) {
        let pane = self.active;
        let Some(entry) = self.panes[pane].selected_source().cloned() else {
            return;
        };
        match entry.status {
            SourceProbeStatus::Checking => {
                self.set_status("Source is still being checked");
            }
            SourceProbeStatus::Unavailable(error) => {
                let _ = self.panes[pane].retry_selected_source(pane, &self.browser);
                self.set_status(error);
            }
            SourceProbeStatus::Ready if entry.source.location.is_local() => {
                let was_local = self.panes[pane].location.is_local();
                let path = self.panes[pane].local_restore_path();
                self.panes[pane].close_sources();
                if !was_local {
                    self.panes[pane].change_directory(pane, path, &self.browser);
                }
                self.record_active_location();
                self.set_status("Opened local filesystem");
            }
            SourceProbeStatus::Ready => {
                self.panes[pane].close_sources();
                self.panes[pane].change_location(pane, entry.source.location, &self.browser);
                self.record_active_location();
                self.set_status(format!("Opened {}", entry.source.name));
            }
        }
    }

    pub(crate) fn open_sort_menu(&mut self, pane: usize) {
        let selected = SortMode::ALL
            .iter()
            .position(|mode| *mode == self.panes[pane].sort.mode)
            .unwrap_or(0);
        self.active = pane;
        self.app_menu = None;
        self.sort_menu = Some(SortMenu { pane, selected });
    }

    pub(crate) fn open_sources(&mut self) {
        let pane = self.active;
        self.panes[pane].open_sources(pane, &self.browser);
        if self.panes[pane].showing_sources() {
            self.set_status("Discovering and checking storage sources…");
        } else {
            self.clear_status();
        }
    }

    pub(crate) fn apply_sort_choice(&mut self, pane: usize, choice: usize) {
        let mut sort = self.panes[pane].sort;
        match choice {
            0..=5 => sort.mode = SortMode::ALL[choice],
            6 => sort.reverse = !sort.reverse,
            7 => sort.directories_first = !sort.directories_first,
            _ => return,
        }
        self.panes[pane].set_sort(sort);
    }

    pub(crate) fn activate_current(&mut self) {
        let Some(entry) = self.panes[self.active].current().cloned() else {
            return;
        };
        if self.panes[self.active].is_archive() {
            match entry.kind {
                BrowserKind::Parent => {
                    self.panes[self.active].change_to_parent(self.active, &self.browser);
                }
                BrowserKind::Directory => {
                    self.panes[self.active].open_archive_directory(&entry.name);
                }
                BrowserKind::File => self.open_current_archive_member(true),
                _ => self.set_status("Unsupported archive member"),
            }
            return;
        }
        let Some(location) = self.panes[self.active].current_location() else {
            return;
        };
        match entry.kind {
            BrowserKind::Parent => {
                self.panes[self.active].change_to_parent(self.active, &self.browser);
                self.record_active_location();
            }
            BrowserKind::Directory => {
                self.navigate_active_to(location);
            }
            BrowserKind::File => match location {
                Location::Local(path) => self.start_archive_load(
                    ArchiveRequest::Path {
                        pane: self.active,
                        path,
                    },
                    None,
                ),
                Location::Remote(_) => self.start_remote_download(location, true),
            },
            BrowserKind::Symlink | BrowserKind::Unknown => match location {
                Location::Local(path) => self.resolve_for_open(path),
                Location::Remote(_) => self.set_status("Remote symbolic links are not followed"),
            },
            BrowserKind::Other => self.set_status("Unsupported filesystem object"),
        }
    }

    pub(crate) fn view_current(&mut self) {
        let Some(entry) = self.panes[self.active].current().cloned() else {
            return;
        };
        if self.panes[self.active].is_archive() {
            if entry.kind == BrowserKind::File {
                self.open_current_archive_member(false);
            } else {
                self.set_status("Select a file to view");
            }
            return;
        }
        if entry.kind == BrowserKind::Parent || entry.kind == BrowserKind::Directory {
            self.set_status("Select a file to view");
            return;
        }
        if let Some(location) = self.panes[self.active].current_location() {
            if entry.kind == BrowserKind::File {
                match location {
                    Location::Local(path) => self.open_viewer(path),
                    Location::Remote(_) => self.start_remote_download(location, false),
                }
            } else {
                match location {
                    Location::Local(path) => self.resolve_for_open(path),
                    Location::Remote(_) => {
                        self.set_status("Remote symbolic links are not followed")
                    }
                }
            }
        }
    }

    pub(crate) fn go_parent(&mut self) {
        self.panes[self.active].change_to_parent(self.active, &self.browser);
        self.record_active_location();
    }

    pub(crate) fn refresh_all(&mut self) {
        self.panes[0].reload(0, &self.browser);
        self.panes[1].reload(1, &self.browser);
    }
}
