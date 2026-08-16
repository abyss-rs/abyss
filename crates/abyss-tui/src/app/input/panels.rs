use cleaner_tui::Outcome;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::dialogs::{InputAction, InputDialog, Modal};
use crate::app::menu::{BookmarkFocus, MenuCategory};
use crate::app::state::App;
use crate::browser::SortMode;
use crate::jobs::JobState;
use crate::ui::ActionButton;

impl App {
    pub(crate) fn handle_source_key(&mut self, key: KeyEvent) {
        let pane = self.active;
        let rows = self.pane_rows;
        match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('s') => {
                self.panes[pane].close_sources();
                self.clear_status();
            }
            KeyCode::Tab => self.active = 1 - self.active,
            KeyCode::Up => {
                self.panes[pane].source_move_by(-1, rows);
                self.show_selected_source_error();
            }
            KeyCode::Down => {
                self.panes[pane].source_move_by(1, rows);
                self.show_selected_source_error();
            }
            KeyCode::PageUp => {
                self.panes[pane].source_move_by(-(rows.max(1) as isize), rows);
                self.show_selected_source_error();
            }
            KeyCode::PageDown => {
                self.panes[pane].source_move_by(rows.max(1) as isize, rows);
                self.show_selected_source_error();
            }
            KeyCode::Home => {
                self.panes[pane].source_move_home(rows);
                self.show_selected_source_error();
            }
            KeyCode::End => {
                self.panes[pane].source_move_end(rows);
                self.show_selected_source_error();
            }
            KeyCode::Enter => self.activate_source(),
            KeyCode::Char('r') | KeyCode::Char('9') => {
                self.panes[pane].refresh_sources(pane, &self.browser);
                self.set_status("Rediscovering storage sources…");
            }
            KeyCode::Char('1') | KeyCode::Char('?') => self.trigger(ActionButton::Help),
            KeyCode::Char('0') | KeyCode::Char('q') => self.trigger(ActionButton::Quit),
            KeyCode::Char('3') => self.trigger(ActionButton::Sync),
            KeyCode::Char('4') => self.trigger(ActionButton::Analyze),
            KeyCode::Char('j') => self.open_jobs_panel(None),
            KeyCode::Char('c') => self.focus_console(),
            KeyCode::Char('2')
            | KeyCode::Char('5')
            | KeyCode::Char('6')
            | KeyCode::Char('7')
            | KeyCode::Char('8')
            | KeyCode::Char('m')
            | KeyCode::Char('d')
            | KeyCode::Char('h')
            | KeyCode::Char('+')
            | KeyCode::Char('=') => {
                self.set_status("File commands are unavailable in Sources");
            }
            _ => {}
        }
    }

    pub(crate) fn handle_sort_key(&mut self, key: KeyEvent) {
        let Some(mut menu) = self.sort_menu else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.sort_menu = None,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                menu.pane = 1 - menu.pane;
                menu.selected = SortMode::ALL
                    .iter()
                    .position(|mode| *mode == self.panes[menu.pane].sort.mode)
                    .unwrap_or(0);
                self.active = menu.pane;
                self.sort_menu = Some(menu);
            }
            KeyCode::Up => {
                menu.selected = menu.selected.checked_sub(1).unwrap_or(7);
                self.sort_menu = Some(menu);
            }
            KeyCode::Down => {
                menu.selected = (menu.selected + 1) % 8;
                self.sort_menu = Some(menu);
            }
            KeyCode::Enter => {
                self.apply_sort_choice(menu.pane, menu.selected);
                self.sort_menu = None;
            }
            KeyCode::Char(' ') => {
                self.apply_sort_choice(menu.pane, menu.selected);
                self.sort_menu = Some(menu);
            }
            _ => self.sort_menu = Some(menu),
        }
    }

    pub(crate) fn handle_app_key(&mut self, key: KeyEvent) {
        let Some(mut menu) = self.app_menu else {
            return;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
            self.app_menu = None;
            return;
        }
        match key.code {
            KeyCode::Esc => self.app_menu = None,
            KeyCode::Left
                if menu.category == MenuCategory::Bookmarks
                    && menu.bookmark_focus == BookmarkFocus::Set
                    && self.workspace.bookmark(menu.selected).is_some() =>
            {
                menu.bookmark_focus = BookmarkFocus::Jump;
                self.app_menu = Some(menu);
            }
            KeyCode::Right
                if menu.category == MenuCategory::Bookmarks
                    && menu.bookmark_focus == BookmarkFocus::Jump =>
            {
                menu.bookmark_focus = BookmarkFocus::Set;
                self.app_menu = Some(menu);
            }
            KeyCode::Left | KeyCode::Right => {
                menu.category =
                    menu.category
                        .shifted(if key.code == KeyCode::Left { -1 } else { 1 });
                menu.selected = 0;
                menu.bookmark_focus = BookmarkFocus::Jump;
                self.normalize_menu_selection(&mut menu);
                self.app_menu = Some(menu);
            }
            KeyCode::Up | KeyCode::Down => {
                let count = if menu.category == MenuCategory::Bookmarks {
                    9
                } else {
                    self.visible_menu_actions(menu.category).len()
                };
                if count > 0 {
                    let amount = if key.code == KeyCode::Up { -1 } else { 1 };
                    menu.selected =
                        (menu.selected as isize + amount).rem_euclid(count as isize) as usize;
                    if menu.category == MenuCategory::Bookmarks {
                        menu.bookmark_focus = if self.workspace.bookmark(menu.selected).is_some() {
                            BookmarkFocus::Jump
                        } else {
                            BookmarkFocus::Set
                        };
                    }
                }
                self.app_menu = Some(menu);
            }
            KeyCode::Char('s') if menu.category == MenuCategory::Bookmarks => {
                self.app_menu = None;
                self.assign_bookmark(menu.selected);
            }
            KeyCode::Enter if menu.category == MenuCategory::Bookmarks => {
                self.app_menu = None;
                match menu.bookmark_focus {
                    BookmarkFocus::Jump if self.workspace.bookmark(menu.selected).is_some() => {
                        self.jump_to_bookmark(menu.selected)
                    }
                    BookmarkFocus::Jump | BookmarkFocus::Set => self.assign_bookmark(menu.selected),
                }
            }
            KeyCode::Enter => {
                let action = self
                    .visible_menu_actions(menu.category)
                    .get(menu.selected)
                    .copied();
                self.app_menu = None;
                if let Some(action) = action {
                    self.perform_menu_action(action);
                }
            }
            _ => self.app_menu = Some(menu),
        }
    }

    pub(crate) fn handle_viewer_key(&mut self, key: KeyEvent) {
        let rows = self.pane_rows.max(1);
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('2')
        ) {
            self.viewer = None;
            self.viewer_highlight = None;
            self.clear_status();
            return;
        }
        let viewer = self.viewer.as_mut().expect("viewer exists");
        match key.code {
            KeyCode::Up => viewer.scroll_vertical(-1, rows),
            KeyCode::Down => viewer.scroll_vertical(1, rows),
            KeyCode::PageUp => viewer.scroll_vertical(-(rows as isize), rows),
            KeyCode::PageDown => viewer.scroll_vertical(rows as isize, rows),
            KeyCode::Left => viewer.scroll_horizontal(-4),
            KeyCode::Right => viewer.scroll_horizontal(4),
            KeyCode::Home => viewer.home(),
            KeyCode::End => viewer.end(rows),
            _ => {}
        }
    }

    pub(crate) fn handle_analyze_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.trigger(ActionButton::EscLeave),
            KeyCode::Char('0') => self.trigger(ActionButton::Quit),
            KeyCode::Char('1') | KeyCode::Char('?') => self.trigger(ActionButton::Help),
            KeyCode::Char('5') => self.trigger(ActionButton::Copy),
            KeyCode::Char('6') | KeyCode::Char('c') => self.trigger(ActionButton::Move),
            KeyCode::Char('8') | KeyCode::Char('d') => self.trigger(ActionButton::Delete),
            KeyCode::Char('9') | KeyCode::Char('r') => self.trigger(ActionButton::Refresh),
            KeyCode::Char('2') | KeyCode::Char('3') | KeyCode::Char('4') | KeyCode::Char('7') => {}
            _ => {
                let Some(session) = self.analyze.as_mut() else {
                    return;
                };
                if session.handle_event(Event::Key(key)) == Outcome::Exit {
                    self.leave_analyze();
                }
            }
        }
    }

    pub(crate) fn handle_jobs_panel_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('j')) {
            self.jobs_panel = None;
            return;
        }
        let ids = self
            .jobs
            .history()
            .into_iter()
            .map(|job| job.id)
            .collect::<Vec<_>>();
        let Some(selected) = self.jobs_panel.and_then(|panel| panel.selected) else {
            return;
        };
        let mut index = ids.iter().position(|id| *id == selected).unwrap_or(0);
        let mut status = None;
        match key.code {
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                status = Some(if self.jobs.reorder_queued(selected, -1) {
                    format!("Moved queued job #{selected} earlier")
                } else {
                    "Only queued jobs can be reordered".to_owned()
                });
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                status = Some(if self.jobs.reorder_queued(selected, 1) {
                    format!("Moved queued job #{selected} later")
                } else {
                    "Only queued jobs can be reordered".to_owned()
                });
            }
            KeyCode::Up => index = index.saturating_sub(1),
            KeyCode::Down => index = (index + 1).min(ids.len().saturating_sub(1)),
            KeyCode::Char('p') => {
                status = Some(if self.jobs.toggle_pause(selected) {
                    if self
                        .jobs
                        .job(selected)
                        .is_some_and(|job| matches!(job.state, JobState::Paused))
                    {
                        format!("Paused job #{selected}")
                    } else {
                        format!("Resumed job #{selected}")
                    }
                } else {
                    "Selected job cannot be paused or resumed".to_owned()
                });
            }
            KeyCode::Char('b') => {
                let current = self.jobs.bandwidth_limit();
                let value = if current == 0 {
                    "0".to_owned()
                } else {
                    format!("{:.1} MiB/s", current as f64 / 1024.0 / 1024.0)
                };
                self.modal = Some(Modal::Input(InputDialog::new(
                    "Bandwidth limit",
                    "Rate (0 = unlimited):",
                    value,
                    InputAction::BandwidthLimit,
                )));
            }
            KeyCode::Char('c') if self.jobs.job(selected).is_some_and(|job| job.is_active()) => {
                self.jobs.cancel(selected);
                if self.foreground_job == Some(selected)
                    && self.jobs.job(selected).is_some_and(|job| !job.is_active())
                {
                    self.foreground_job = None;
                }
                status = Some(format!("Cancelling job #{selected}…"));
            }
            _ => {}
        }
        if let Some(panel) = &mut self.jobs_panel {
            panel.selected = ids.get(index).copied();
        }
        if let Some(status) = status {
            self.set_status(status);
        }
    }
}
