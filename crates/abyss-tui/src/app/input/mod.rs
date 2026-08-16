mod modal;
mod mouse;
mod panels;

use std::fs;
use std::path::{Component, Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::Zeroizing;

use crate::app::actions::parse_bandwidth_limit;
use crate::app::dialogs::{InputAction, InputDialog, Modal};
use crate::app::menu::MenuAction;
use crate::app::runner::os_string_from_external;
use crate::app::state::App;
use crate::jobs::LaunchMode;
use crate::operation::OperationKind;
use crate::progress::human_bytes;
use crate::search::SearchKind;
use crate::storage::{Location, LocationCodec};
use crate::ui::ActionButton;
use crate::workspace::query_smart_jump_in;

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) {
        // The size cycle works from anywhere, including with the shell focused,
        // so there is always a way back out of a fullscreen console.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('x')
            && self.modal.is_none()
            && self.analyze.is_none()
            && self.sync.is_none()
            && self.viewer.is_none()
            && self.diff.is_none()
            && self.monitor.is_none()
        {
            self.cycle_console();
            return;
        }
        // While the shell has focus its keys win, Ctrl+C included: it has to
        // reach the foreground process rather than cancel an Abyss job.
        if self.console_focused()
            && self.modal.is_none()
            && self.analyze.is_none()
            && self.sync.is_none()
            && self.viewer.is_none()
            && self.app_menu.is_none()
            && self.jobs_panel.is_none()
            && self.foreground_job.is_none()
            && self.handle_console_key(key)
        {
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if let Some(id) = self.foreground_job {
                self.jobs.cancel(id);
                if self.jobs.job(id).is_some_and(|job| !job.is_active()) {
                    self.foreground_job = None;
                }
                self.set_status(format!("Cancelling job #{id}…"));
            } else if self.jobs.has_active() {
                self.jobs.cancel_all();
                self.set_status("Cancelling all jobs…");
            } else {
                self.should_quit = true;
            }
            return;
        }

        if self.analyze.is_some() && self.modal.is_none() {
            self.handle_analyze_key(key);
            return;
        }

        if self.sync.is_some() && self.modal.is_none() {
            self.handle_sync_key(key);
            return;
        }

        if self.monitor.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.monitor = None;
            }
            return;
        }
        if self.diff.is_some() {
            self.handle_diff_key(key);
            return;
        }
        if self.viewer.is_some() {
            self.handle_viewer_key(key);
            return;
        }
        if self.viewer_load.is_some() {
            if key.code == KeyCode::Esc {
                self.viewer_load = None;
                self.viewer_loading = None;
                self.viewer_temp = None;
                self.clear_status();
            }
            return;
        }
        if self.app_menu.is_some() {
            self.handle_app_key(key);
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('z')
            && self.sort_menu.is_some()
        {
            self.sort_menu = None;
            self.open_app_menu();
            return;
        }
        if self.sort_menu.is_some() {
            self.handle_sort_key(key);
            return;
        }
        if self.modal.is_some() {
            self.handle_modal_key(key);
            return;
        }
        if self.jobs_panel.is_some() {
            self.handle_jobs_panel_key(key);
            return;
        }
        if self.archive_load.is_some() {
            if key.code == KeyCode::Esc {
                self.archive_load = None;
                self.clear_status();
            }
            return;
        }
        if self.sync_load.is_some() {
            if key.code == KeyCode::Esc {
                self.sync_load = None;
                self.clear_status();
            }
            return;
        }
        if let Some(id) = self.foreground_job {
            match key.code {
                KeyCode::Esc => {
                    self.jobs.cancel(id);
                    if self.jobs.job(id).is_some_and(|job| !job.is_active()) {
                        self.foreground_job = None;
                    }
                    self.set_status(format!("Cancelling job #{id}…"));
                }
                KeyCode::Char('0') | KeyCode::Char('q') => {
                    self.modal = Some(Modal::QuitJobs);
                }
                _ => {}
            }
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
            self.open_app_menu();
            return;
        }

        if self.pending_resolve.take().is_some() {
            self.clear_status();
            if key.code == KeyCode::Esc {
                return;
            }
        }

        if self.panes[self.active].showing_sources() {
            self.handle_source_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('h') => {
                    self.perform_menu_action(MenuAction::DirectoryHistory);
                    return;
                }
                KeyCode::Char('j') => {
                    self.perform_menu_action(MenuAction::SmartJump);
                    return;
                }
                _ => {}
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(digit @ '1'..='9') = key.code {
                let index = digit as usize - '1' as usize;
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.assign_bookmark(index);
                } else {
                    self.jump_to_bookmark(index);
                }
                return;
            }
            match key.code {
                KeyCode::Char('t') => {
                    self.perform_menu_action(MenuAction::NewTab);
                    return;
                }
                KeyCode::Char('w') => {
                    self.perform_menu_action(MenuAction::CloseTab);
                    return;
                }
                KeyCode::Char('l') => {
                    self.perform_menu_action(MenuAction::SynchronizedScrolling);
                    return;
                }
                _ => {}
            }
        }

        let rows = self.pane_rows;
        match key.code {
            KeyCode::Up => self.move_active_by(-1, rows),
            KeyCode::Down => self.move_active_by(1, rows),
            KeyCode::PageUp => {
                let rows = rows.max(1);
                self.move_active_by(-(rows as isize), rows);
            }
            KeyCode::PageDown => {
                let rows = rows.max(1);
                self.move_active_by(rows as isize, rows);
            }
            KeyCode::Home => {
                self.active_pane_mut().move_home(rows);
                self.sync_selection();
            }
            KeyCode::End => {
                self.active_pane_mut().move_end(rows);
                self.sync_selection();
            }
            KeyCode::Tab => self.active = 1 - self.active,
            KeyCode::Char('[') => self.switch_tab(-1),
            KeyCode::Char(']') => self.switch_tab(1),
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.perform_menu_action(MenuAction::OpenSubshell)
            }
            KeyCode::Enter => self.activate_current(),
            KeyCode::Backspace => self.go_parent(),
            KeyCode::Char('=') | KeyCode::Char('+') => {
                let rows = self.pane_rows;
                self.active_pane_mut().toggle_mark_and_advance(rows);
            }
            KeyCode::Char('1') | KeyCode::Char('?') => self.trigger(ActionButton::Help),
            KeyCode::Char('2') => self.trigger(ActionButton::View),
            KeyCode::Char('3') => self.trigger(ActionButton::Sync),
            KeyCode::Char('4') => self.trigger(ActionButton::Analyze),
            // `c` opens the console; Copy is on 5, as the button bar shows.
            KeyCode::Char('5') => self.trigger(ActionButton::Copy),
            KeyCode::Char('c') => self.focus_console(),
            KeyCode::Char('6') | KeyCode::Char('m') => self.trigger(ActionButton::Move),
            KeyCode::Char('7') => self.trigger(ActionButton::Mkdir),
            KeyCode::Char('8') | KeyCode::Char('d') => self.trigger(ActionButton::Delete),
            KeyCode::Char('9') | KeyCode::Char('r') => self.trigger(ActionButton::Refresh),
            KeyCode::Char('0') | KeyCode::Char('q') => self.trigger(ActionButton::Quit),
            KeyCode::Char('s') => self.open_sources(),
            KeyCode::Char('j') => self.perform_menu_action(MenuAction::Jobs),
            KeyCode::Char('o') => self.perform_menu_action(MenuAction::OpenDefaultApp),
            KeyCode::Char('e') | KeyCode::Char('v') => {
                self.perform_menu_action(MenuAction::OpenEditor)
            }
            KeyCode::Char('x') => self.perform_menu_action(MenuAction::DirectoryComparison),
            KeyCode::Char('y') => self.perform_menu_action(MenuAction::DifferentialSync),
            #[cfg(feature = "kubernetes")]
            KeyCode::Char('k') => self.perform_menu_action(MenuAction::VolumeSnapshot),
            KeyCode::Char('p') | KeyCode::Char('a') => {
                self.perform_menu_action(MenuAction::CreateArchive)
            }
            KeyCode::Char('h') | KeyCode::Char('H') => self.perform_menu_action(MenuAction::Hashes),
            KeyCode::Char('i') => self.perform_menu_action(MenuAction::Inspect),
            KeyCode::Char('f') => self.perform_menu_action(MenuAction::FindFiles),
            KeyCode::Char('g') => self.perform_menu_action(MenuAction::GrepTree),
            KeyCode::Char('\\') => self.perform_menu_action(MenuAction::DiffPanes),
            KeyCode::Char('t') => self.perform_menu_action(MenuAction::SystemMonitor),
            _ => {}
        }
    }

    pub(crate) fn handle_paste(&mut self, value: &str) {
        match &mut self.modal {
            Some(Modal::Input(input)) => input.paste(value),
            Some(Modal::History(history)) => {
                for character in value.chars() {
                    history.insert(character);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn submit_input(&mut self, input: InputDialog, launch: LaunchMode) {
        let value = input.text();
        if value.is_empty() {
            self.modal = Some(Modal::Input(input));
            return;
        }
        match input.action {
            InputAction::Mkdir => {
                let path = Path::new(&value);
                let valid = path.components().count() == 1
                    && matches!(path.components().next(), Some(Component::Normal(_)));
                if !valid {
                    self.show_error("Create directory", "Enter one directory name");
                    return;
                }
                let name = path.as_os_str().as_encoded_bytes();
                let destination = match self.panes[self.active].location.child(name) {
                    Ok(destination) => destination,
                    Err(error) => {
                        self.show_error("Create directory", error.to_string());
                        return;
                    }
                };
                let result = match &destination {
                    Location::Local(path) => {
                        fs::create_dir(path).map_err(|error| error.to_string())
                    }
                    Location::Remote(remote) => {
                        let storage = self.browser.storage();
                        storage
                            .backend(remote)
                            .and_then(|backend| storage.block_on(backend.create_dir(&remote.path)))
                            .map_err(|error| error.to_string())
                    }
                };
                match result {
                    Ok(()) => {
                        self.set_status(format!("Created {}", destination.display()));
                        let created = destination
                            .file_name()
                            .map(os_string_from_external)
                            .expect("validated directory name");
                        for pane in 0..2 {
                            if pane == self.active {
                                self.panes[pane].reload_selecting(
                                    pane,
                                    created.clone(),
                                    &self.browser,
                                );
                            } else {
                                self.panes[pane].reload(pane, &self.browser);
                            }
                        }
                    }
                    Err(error) => self.show_error("Create directory", error),
                }
            }
            InputAction::FindFiles => self.start_search(value, SearchKind::Files),
            InputAction::GrepTree => self.start_search(value, SearchKind::Contents),
            InputAction::SmartJump => match query_smart_jump_in(&self.workspace.visits, &value) {
                Ok(path) => self.navigate_active_to(Location::Local(path)),
                Err(error) => self.show_error("Smart jump", error),
            },
            InputAction::BandwidthLimit => match parse_bandwidth_limit(&value) {
                Ok(limit) => {
                    self.jobs.set_bandwidth_limit(limit);
                    self.workspace.bandwidth_limit = limit;
                    if let Err(error) = self.persist_workspace() {
                        self.set_status(error);
                    } else if limit == 0 {
                        self.set_status("Bandwidth limit disabled");
                    } else {
                        self.set_status(format!("Bandwidth limited to {}/s", human_bytes(limit)));
                    }
                }
                Err(error) => self.show_error("Bandwidth limit", error),
            },
            InputAction::Copy(sources) => {
                let destination = match self.resolve_input_location(&value) {
                    Ok(value) => value,
                    Err(error) => {
                        self.show_error("Copy", error);
                        return;
                    }
                };
                self.start_copy(sources, destination, OperationKind::Copy, launch);
            }
            InputAction::Move(sources) => {
                let destination = match self.resolve_input_location(&value) {
                    Ok(value) => value,
                    Err(error) => {
                        self.show_error("Move", error);
                        return;
                    }
                };
                self.start_copy(sources, destination, OperationKind::Move, launch);
            }
            InputAction::ArchivePassword(request) => {
                self.start_archive_load(request, Some(Zeroizing::new(value)));
            }
            InputAction::Extract {
                index,
                roots,
                base,
                password,
                temporary,
            } => {
                let destination = match self.resolve_input_location(&value) {
                    Ok(value) => value,
                    Err(error) => {
                        self.show_error("Extract", error);
                        return;
                    }
                };
                self.start_extract(index, roots, base, destination, password, temporary, launch);
            }
        }
    }

    pub(crate) fn resolve_input_location(&self, value: &str) -> Result<Location, String> {
        if value.contains("://") {
            return LocationCodec::parse(value).map_err(|error| error.to_string());
        }
        match &self.panes[self.active].location {
            Location::Local(cwd) => {
                let path = PathBuf::from(value);
                Ok(Location::Local(if path.is_absolute() {
                    path
                } else {
                    cwd.join(path)
                }))
            }
            Location::Remote(_) => self.panes[self.active]
                .location
                .child(value.as_bytes())
                .map_err(|error| error.to_string()),
        }
    }
}
