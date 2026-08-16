use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::dialogs::{ArchiveCreateField, HashCreateField, InputAction, InputDialog, Modal};
use crate::app::state::{App, LastClick};
use crate::jobs::LaunchMode;
use crate::storage::Location;
use crate::ui::{ActionButton, DialogButton};

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if let Some(viewer) = &mut self.viewer {
            let rows = self.pane_rows.max(1);
            match mouse.kind {
                MouseEventKind::ScrollUp => viewer.scroll_vertical(-3, rows),
                MouseEventKind::ScrollDown => viewer.scroll_vertical(3, rows),
                _ => {}
            }
            return;
        }
        if self.sync.is_some() && self.modal.is_none() {
            self.handle_sync_mouse(mouse);
            return;
        }
        if self.modal.is_some() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && let Some(field) = self.layout.archive_field_at(mouse.column, mouse.row)
                && let Some(Modal::ArchiveCreate(mut dialog)) = self.modal.take()
            {
                if dialog.focus == field
                    && !matches!(
                        field,
                        ArchiveCreateField::Filename
                            | ArchiveCreateField::Password
                            | ArchiveCreateField::ConfirmPassword
                    )
                {
                    dialog.adjust(1);
                }
                dialog.focus = field;
                self.modal = Some(Modal::ArchiveCreate(dialog));
                return;
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && let Some(field) = self.layout.hash_field_at(mouse.column, mouse.row)
                && let Some(Modal::HashCreate(mut dialog)) = self.modal.take()
            {
                if dialog.focus == field && field != HashCreateField::Filename {
                    dialog.adjust(1);
                }
                dialog.focus = field;
                self.modal = Some(Modal::HashCreate(dialog));
                return;
            }
            if mouse.kind == MouseEventKind::Up(MouseButton::Left)
                && let Some(button) = self.layout.dialog_button_at(mouse.column, mouse.row)
            {
                match self.modal.take() {
                    Some(Modal::Input(input)) => match button {
                        DialogButton::Start => self.submit_input(input, LaunchMode::Foreground),
                        DialogButton::Background if input.supports_background() => {
                            self.submit_input(input, LaunchMode::Background)
                        }
                        DialogButton::Background => {
                            self.modal = Some(Modal::Input(input));
                        }
                        DialogButton::Cancel => {}
                    },
                    Some(Modal::ArchiveCreate(dialog)) => match button {
                        DialogButton::Start => {
                            self.confirm_archive_create(dialog, LaunchMode::Foreground)
                        }
                        DialogButton::Background => {
                            self.confirm_archive_create(dialog, LaunchMode::Background)
                        }
                        DialogButton::Cancel => {}
                    },
                    Some(Modal::HashCreate(dialog)) => match button {
                        DialogButton::Start => {
                            self.confirm_hash_create(dialog, LaunchMode::Foreground)
                        }
                        DialogButton::Background => {
                            self.confirm_hash_create(dialog, LaunchMode::Background)
                        }
                        DialogButton::Cancel => {}
                    },
                    Some(Modal::VerifyHash(database)) => match button {
                        DialogButton::Start => {
                            self.start_hash_verify(database, LaunchMode::Foreground)
                        }
                        DialogButton::Background => {
                            self.start_hash_verify(database, LaunchMode::Background)
                        }
                        DialogButton::Cancel => {}
                    },
                    Some(Modal::ConfirmClean { path, .. }) => match button {
                        DialogButton::Start => {
                            if let Some(session) = self.analyze.as_mut() {
                                session.run_clean();
                            }
                            self.set_status(format!("Cleaning {}…", path.display()));
                        }
                        DialogButton::Cancel | DialogButton::Background => {}
                    },
                    other => self.modal = other,
                }
            }
            return;
        }
        if self.jobs_panel.is_some() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && let Some(id) = self.layout.job_at(mouse.column, mouse.row)
                && let Some(panel) = &mut self.jobs_panel
            {
                panel.selected = Some(id);
            }
            return;
        }
        if self.viewer_load.is_some()
            || self.archive_load.is_some()
            || self.foreground_job.is_some()
        {
            return;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(id) = self.layout.job_at(mouse.column, mouse.row)
        {
            self.open_jobs_panel(Some(id));
            return;
        }
        if self.pending_resolve.take().is_some() {
            self.clear_status();
        }
        if self.app_menu.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.handle_app_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                    return;
                }
                MouseEventKind::ScrollDown => {
                    self.handle_app_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                    return;
                }
                _ => {}
            }
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(pane) = self.layout.pane_at(mouse.column, mouse.row) {
                    self.active = pane;
                }
                let rows = self.pane_rows;
                if self.panes[self.active].showing_sources() {
                    self.panes[self.active].source_move_by(-3, rows);
                    self.show_selected_source_error();
                } else {
                    self.active_pane_mut().move_by(-3, rows);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(pane) = self.layout.pane_at(mouse.column, mouse.row) {
                    self.active = pane;
                }
                let rows = self.pane_rows;
                if self.panes[self.active].showing_sources() {
                    self.panes[self.active].source_move_by(3, rows);
                    self.show_selected_source_error();
                } else {
                    self.active_pane_mut().move_by(3, rows);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(action) = self.layout.menu_item_at(mouse.column, mouse.row) {
                    self.app_menu = None;
                    self.perform_menu_action(action);
                    return;
                }
                if let Some(index) = self.layout.bookmark_set_at(mouse.column, mouse.row) {
                    self.app_menu = None;
                    self.assign_bookmark(index);
                    return;
                }
                if let Some(index) = self.layout.bookmark_row_at(mouse.column, mouse.row) {
                    self.app_menu = None;
                    self.jump_to_bookmark(index);
                    return;
                }
                if let Some(choice) = self.layout.sort_item_at(mouse.column, mouse.row) {
                    if let Some(menu) = self.sort_menu {
                        self.apply_sort_choice(menu.pane, choice);
                        self.sort_menu = None;
                    }
                    return;
                }
                if let Some(category) = self.layout.menu_heading_at(mouse.column, mouse.row) {
                    if self.app_menu.is_some_and(|menu| menu.category == category) {
                        self.app_menu = None;
                    } else {
                        self.open_menu_category(category);
                    }
                    return;
                }
                if self.layout.sort_menu_at(mouse.column, mouse.row) {
                    if self.panes[self.active].showing_sources() {
                        self.set_status("Sorting is unavailable in Sources");
                    } else {
                        self.open_sort_menu(self.active);
                    }
                    return;
                }
                if self.app_menu.take().is_some() || self.sort_menu.take().is_some() {
                    return;
                }
                if let Some((pane, delta)) = self.layout.tab_nav_at(mouse.column, mouse.row) {
                    self.active = pane;
                    self.switch_tab(delta);
                    return;
                }
                if self.layout.button_at(mouse.column, mouse.row).is_some() {
                    return;
                }
                if let Some((pane, index)) = self.layout.row_at(mouse.column, mouse.row) {
                    self.active = pane;
                    let rows = self.pane_rows;
                    if self.panes[pane].showing_sources() {
                        self.panes[pane].source_select_index(index, rows);
                        self.show_selected_source_error();
                    } else {
                        self.panes[pane].select_index(index, rows);
                    }
                    let now = Instant::now();
                    let double = self.last_click.is_some_and(|last| {
                        last.pane == pane
                            && last.index == index
                            && now.duration_since(last.at) < Duration::from_millis(400)
                    });
                    self.last_click = Some(LastClick {
                        pane,
                        index,
                        at: now,
                    });
                    if double {
                        if self.panes[pane].showing_sources() {
                            self.activate_source();
                        } else {
                            self.activate_current();
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(button) = self.layout.button_at(mouse.column, mouse.row) {
                    self.trigger(button);
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some((pane, index)) = self.layout.row_at(mouse.column, mouse.row) {
                    if self.panes[pane].showing_sources() {
                        self.active = pane;
                        self.panes[pane].source_select_index(index, self.pane_rows);
                        self.set_status("Marking is unavailable in Sources");
                        return;
                    }
                    self.active = pane;
                    let rows = self.pane_rows;
                    self.panes[pane].select_index(index, rows);
                    let old = self.panes[pane].selected;
                    self.panes[pane].toggle_mark_and_advance(rows);
                    self.panes[pane].selected = old;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn trigger(&mut self, button: ActionButton) {
        if self.modal.is_some() || self.viewer.is_some() || self.foreground_job.is_some() {
            return;
        }
        if self.analyze.is_some() {
            match button {
                ActionButton::EscLeave => {
                    self.leave_analyze();
                }
                ActionButton::Help => {
                    if let Some(session) = self.analyze.as_mut() {
                        session.show_help_status();
                    }
                }
                ActionButton::Copy => {
                    if let Some(session) = self.analyze.as_mut() {
                        session.toggle_sort();
                    }
                }
                ActionButton::Move => {
                    self.open_analyze_clean_confirm();
                }
                ActionButton::Delete => {
                    if let Some(session) = self.analyze.as_mut() {
                        session.toggle_delete_confirm();
                    }
                }
                ActionButton::Refresh => {
                    if let Some(session) = self.analyze.as_mut() {
                        session.refresh();
                    }
                }
                ActionButton::Analyze
                | ActionButton::Sync
                | ActionButton::View
                | ActionButton::Mkdir => {}
                ActionButton::Quit => {
                    if self.jobs.has_active() {
                        self.modal = Some(Modal::QuitJobs);
                    } else {
                        self.should_quit = true;
                    }
                }
            }
            return;
        }
        if self.panes[self.active].showing_sources() {
            match button {
                ActionButton::Help => self.modal = Some(Modal::Help),
                ActionButton::Refresh => {
                    let pane = self.active;
                    self.panes[pane].refresh_sources(pane, &self.browser);
                    self.set_status("Rediscovering storage sources…");
                }
                ActionButton::Quit => {
                    if self.jobs.has_active() {
                        self.modal = Some(Modal::QuitJobs);
                    } else {
                        self.should_quit = true;
                    }
                }
                _ => self.set_status("File commands are unavailable in Sources"),
            }
            return;
        }
        if self.panes[self.active].is_archive()
            && matches!(
                button,
                ActionButton::Move
                    | ActionButton::Delete
                    | ActionButton::Refresh
                    | ActionButton::Analyze
            )
        {
            if button == ActionButton::Analyze {
                self.set_status("Analyze is local-only");
            }
            return;
        }
        match button {
            ActionButton::Help => self.modal = Some(Modal::Help),
            ActionButton::View => self.view_current(),
            ActionButton::Mkdir => {
                if self.panes[self.active].is_archive() {
                    self.start_archive_test();
                    return;
                }
                if self.location_read_only(&self.panes[self.active].location) {
                    self.set_status("The active storage source is read only");
                    return;
                }
                self.modal = Some(Modal::Input(InputDialog::new(
                    "Create directory",
                    "Name:",
                    String::new(),
                    InputAction::Mkdir,
                )));
            }
            ActionButton::Copy | ActionButton::Move => {
                if self.panes[self.active].is_archive() {
                    if button == ActionButton::Move {
                        self.set_status("Archives are read only; use Copy to extract");
                        return;
                    }
                    if self.panes[1 - self.active].is_archive() {
                        self.set_status("Extraction destination must be a filesystem pane");
                        return;
                    }
                    let roots = self.panes[self.active].selected_archive_members();
                    let Some(index) = self.panes[self.active].archive_index() else {
                        return;
                    };
                    if roots.is_empty() {
                        self.set_status("Nothing selected");
                        return;
                    }
                    let destination = self.panes[1 - self.active].location.display();
                    let action = InputAction::Extract {
                        index,
                        roots,
                        base: self.panes[self.active].archive_directory(),
                        password: self.panes[self.active].archive_password(),
                        temporary: self.panes[self.active].archive_temporary(),
                    };
                    self.modal = Some(Modal::Input(InputDialog::new(
                        "Extract",
                        "Destination:",
                        destination,
                        action,
                    )));
                    return;
                }
                if self.panes[1 - self.active].is_archive() {
                    self.set_status("Archives are read only; cannot copy into one");
                    return;
                }
                if self.location_read_only(&self.panes[1 - self.active].location) {
                    self.set_status("The destination storage source is read only");
                    return;
                }
                if button == ActionButton::Move
                    && self.location_read_only(&self.panes[self.active].location)
                {
                    self.set_status("The active storage source is read only; use Copy");
                    return;
                }
                let sources = self.panes[self.active].selected_locations();
                if sources.is_empty() {
                    self.set_status("Nothing selected");
                    return;
                }
                let destination = self.panes[1 - self.active].location.display();
                let (title, action) = if button == ActionButton::Copy {
                    ("Copy", InputAction::Copy(sources))
                } else {
                    ("Move / Rename", InputAction::Move(sources))
                };
                self.modal = Some(Modal::Input(InputDialog::new(
                    title,
                    "Destination:",
                    destination,
                    action,
                )));
            }
            ActionButton::Delete => {
                if self.panes[self.active].is_archive() {
                    self.set_status("Archives are read only");
                    return;
                }
                if self.location_read_only(&self.panes[self.active].location) {
                    self.set_status("The active storage source is read only");
                    return;
                }
                let paths = self.panes[self.active].selected_locations();
                if paths.is_empty() {
                    self.set_status("Nothing selected");
                } else {
                    let trash_available = paths.iter().all(Location::is_local);
                    self.modal = Some(Modal::ConfirmDelete {
                        paths,
                        trash_available,
                    });
                }
            }
            ActionButton::Refresh => self.refresh_all(),
            ActionButton::Sync => self.open_sync_session(),
            ActionButton::Analyze => self.open_analyze(),
            ActionButton::EscLeave => {
                if self.analyze.is_some() {
                    self.leave_analyze();
                } else if self.sync.is_some() {
                    self.leave_sync_mode();
                }
            }
            ActionButton::Quit => {
                if self.sync.is_some() {
                    self.leave_sync_mode();
                } else if self.jobs.has_active() {
                    self.modal = Some(Modal::QuitJobs);
                } else {
                    self.should_quit = true;
                }
            }
        }
    }
}
