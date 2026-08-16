use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::state::App;
use crate::app::sync::{SyncMenu, SyncMenuAction, SyncMenuCategory};
use crate::ui::ActionButton;

impl App {
    pub(crate) fn handle_sync_key(&mut self, key: KeyEvent) {
        let menu_opt = self.sync.as_ref().and_then(|s| s.menu);
        if let Some(menu) = menu_opt {
            let category = menu.category;
            match key.code {
                KeyCode::Esc => {
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = None;
                    }
                }
                KeyCode::Left => {
                    let next_cat = category.shifted(-1);
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = Some(SyncMenu {
                            category: next_cat,
                            selected: 0,
                        });
                    }
                }
                KeyCode::Right => {
                    let next_cat = category.shifted(1);
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = Some(SyncMenu {
                            category: next_cat,
                            selected: 0,
                        });
                    }
                }
                KeyCode::Up => {
                    let selected = menu.selected.saturating_sub(1);
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = Some(SyncMenu { category, selected });
                    }
                }
                KeyCode::Down => {
                    let actions = SyncMenuAction::for_category(category);
                    let selected = (menu.selected + 1).min(actions.len().saturating_sub(1));
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = Some(SyncMenu { category, selected });
                    }
                }
                KeyCode::Enter => {
                    let actions = SyncMenuAction::for_category(category);
                    let action = actions.get(menu.selected).copied();
                    if let Some(sync) = self.sync.as_mut() {
                        sync.menu = None;
                    }
                    if let Some(action) = action {
                        self.perform_sync_menu_action(action);
                    }
                }
                _ => {}
            }
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
            if let Some(sync) = self.sync.as_mut() {
                sync.menu = Some(SyncMenu {
                    category: SyncMenuCategory::Strategy,
                    selected: 0,
                });
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('0') | KeyCode::Char('q') => self.leave_sync_mode(),
            KeyCode::Char('1') | KeyCode::Char('?') => self.trigger(ActionButton::Help),
            KeyCode::Char('2') => self.inspect_sync_item(),
            KeyCode::Char('3') | KeyCode::Enter => self.run_sync_session(false),
            KeyCode::Char('5') => self.swap_sync_direction(),
            KeyCode::Char('6') => self.cycle_sync_comparison(),
            KeyCode::Char('7') => {
                let label = if let Some(sync) = self.sync.as_mut() {
                    sync.filter = sync.filter.toggled();
                    Some(sync.filter.label())
                } else {
                    None
                };
                if let Some(label) = label {
                    self.set_status(format!("Sync Filter: {label}"));
                }
            }
            KeyCode::Char('8') => self.cycle_sync_strategy(),
            KeyCode::Char('9') | KeyCode::Char('r') => self.rescan_sync_plan(),
            KeyCode::Char('b') | KeyCode::Char('B') => self.run_sync_session(true),
            KeyCode::Up => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.selected_index = sync.selected_index.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(sync) = self.sync.as_mut() {
                    let total = sync.plan.as_ref().map_or(0, |p| p.files.len());
                    sync.selected_index = (sync.selected_index + 1).min(total.saturating_sub(1));
                }
            }
            KeyCode::PageUp => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.selected_index = sync.selected_index.saturating_sub(10);
                }
            }
            KeyCode::PageDown => {
                if let Some(sync) = self.sync.as_mut() {
                    let total = sync.plan.as_ref().map_or(0, |p| p.files.len());
                    sync.selected_index = (sync.selected_index + 10).min(total.saturating_sub(1));
                }
            }
            KeyCode::Home => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.selected_index = 0;
                }
            }
            KeyCode::End => {
                if let Some(sync) = self.sync.as_mut() {
                    let total = sync.plan.as_ref().map_or(0, |p| p.files.len());
                    sync.selected_index = total.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_sync_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(category) = self.layout.sync_menu_heading_at(mouse.column, mouse.row) {
                if let Some(sync) = self.sync.as_mut() {
                    sync.menu = Some(SyncMenu {
                        category,
                        selected: 0,
                    });
                }
                return;
            }
            if let Some(action) = self.layout.sync_menu_item_at(mouse.column, mouse.row) {
                if let Some(sync) = self.sync.as_mut() {
                    sync.menu = None;
                }
                self.perform_sync_menu_action(action);
                return;
            }
            if let Some(button) = self.layout.button_at(mouse.column, mouse.row) {
                match button {
                    ActionButton::EscLeave | ActionButton::Quit => self.leave_sync_mode(),
                    ActionButton::Help => self.trigger(ActionButton::Help),
                    ActionButton::View => self.inspect_sync_item(),
                    ActionButton::Mkdir => {
                        let label = if let Some(sync) = self.sync.as_mut() {
                            sync.filter = sync.filter.toggled();
                            Some(sync.filter.label())
                        } else {
                            None
                        };
                        if let Some(label) = label {
                            self.set_status(format!("Filter: {label}"));
                        }
                    }
                    ActionButton::Copy => self.swap_sync_direction(),
                    ActionButton::Move => self.cycle_sync_comparison(),
                    ActionButton::Delete => self.cycle_sync_strategy(),
                    ActionButton::Refresh => self.rescan_sync_plan(),
                    ActionButton::Sync => self.run_sync_session(false),
                    _ => {}
                }
                return;
            }
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.selected_index = sync.selected_index.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(sync) = self.sync.as_mut() {
                    let total = sync.plan.as_ref().map_or(0, |p| p.files.len());
                    sync.selected_index = (sync.selected_index + 3).min(total.saturating_sub(1));
                }
            }
            _ => {}
        }
    }
}
