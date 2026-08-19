use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::dialogs::Modal;
use crate::app::state::App;
use crate::jobs::LaunchMode;
use crate::operation::ConflictChoice;

impl App {
    pub(crate) fn handle_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.modal.take() else {
            return;
        };
        match modal {
            Modal::Inspect(mut dialog) => match key.code {
                KeyCode::Up => {
                    dialog.scroll = dialog.scroll.saturating_sub(1);
                    self.modal = Some(Modal::Inspect(dialog));
                }
                KeyCode::Down => {
                    dialog.scroll = dialog.scroll.saturating_add(1);
                    self.modal = Some(Modal::Inspect(dialog));
                }
                KeyCode::PageUp => {
                    dialog.scroll = dialog.scroll.saturating_sub(5);
                    self.modal = Some(Modal::Inspect(dialog));
                }
                KeyCode::PageDown => {
                    dialog.scroll = dialog.scroll.saturating_add(5);
                    self.modal = Some(Modal::Inspect(dialog));
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('i') => {}
                _ => self.modal = Some(Modal::Inspect(dialog)),
            },
            Modal::Help | Modal::Message { .. } => {
                if !matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    self.modal = Some(modal);
                }
            }
            Modal::ArchiveCreate(dialog) => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Tab | KeyCode::Down => {
                    let mut dialog = dialog;
                    dialog.cycle_focus(1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::BackTab | KeyCode::Up => {
                    let mut dialog = dialog;
                    dialog.cycle_focus(-1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Char('b') | KeyCode::Char('B')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        || !dialog.text_field_focused() =>
                {
                    self.confirm_archive_create(dialog, LaunchMode::Background);
                }
                KeyCode::Enter => {
                    self.confirm_archive_create(dialog, LaunchMode::Foreground);
                }
                KeyCode::Left if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.move_text_cursor(-1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Right if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.move_text_cursor(1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Home if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.text_home();
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::End if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.text_end();
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Backspace if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.backspace_text();
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Delete if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.delete_text();
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Left => {
                    let mut dialog = dialog;
                    dialog.adjust(-1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Right | KeyCode::Char(' ') if !dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.adjust(1);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                KeyCode::Char(character) if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.insert_text(character);
                    self.modal = Some(Modal::ArchiveCreate(dialog));
                }
                _ => self.modal = Some(Modal::ArchiveCreate(dialog)),
            },
            Modal::HashCreate(dialog) => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Tab | KeyCode::Down => {
                    let mut dialog = dialog;
                    dialog.cycle_focus(1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::BackTab | KeyCode::Up => {
                    let mut dialog = dialog;
                    dialog.cycle_focus(-1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Char('b') | KeyCode::Char('B')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        || !dialog.text_field_focused() =>
                {
                    self.confirm_hash_create(dialog, LaunchMode::Background);
                }
                KeyCode::Enter => self.confirm_hash_create(dialog, LaunchMode::Foreground),
                KeyCode::Left if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.move_cursor(-1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Right if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.move_cursor(1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Home if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.cursor = 0;
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::End if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.cursor = dialog.filename.len();
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Backspace if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.backspace();
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Delete if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.delete();
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Left => {
                    let mut dialog = dialog;
                    dialog.adjust(-1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Right | KeyCode::Char(' ') if !dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.adjust(1);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                KeyCode::Char(character) if dialog.text_field_focused() => {
                    let mut dialog = dialog;
                    dialog.insert(character);
                    self.modal = Some(Modal::HashCreate(dialog));
                }
                _ => self.modal = Some(Modal::HashCreate(dialog)),
            },
            Modal::VerifyHash(database) => match key.code {
                KeyCode::Enter => {
                    self.start_hash_verify(database, LaunchMode::Foreground);
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    self.start_hash_verify(database, LaunchMode::Background);
                }
                KeyCode::Esc => {}
                _ => self.modal = Some(Modal::VerifyHash(database)),
            },
            Modal::ConfirmSync(plan) => match key.code {
                KeyCode::Enter | KeyCode::Char('y') => self.execute_sync_plan(plan),
                KeyCode::Esc | KeyCode::Char('n') => {}
                _ => self.modal = Some(Modal::ConfirmSync(plan)),
            },
            Modal::Find(mut find) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    self.open_find_selection(&find);
                }
                KeyCode::Up => {
                    find.move_selection(-1);
                    self.modal = Some(Modal::Find(find));
                }
                KeyCode::Down => {
                    find.move_selection(1);
                    self.modal = Some(Modal::Find(find));
                }
                KeyCode::PageUp => {
                    find.move_selection(-10);
                    self.modal = Some(Modal::Find(find));
                }
                KeyCode::PageDown => {
                    find.move_selection(10);
                    self.modal = Some(Modal::Find(find));
                }
                KeyCode::Backspace => {
                    find.backspace();
                    self.modal = Some(Modal::Find(find));
                }
                KeyCode::Char(character) => {
                    find.insert(character);
                    self.modal = Some(Modal::Find(find));
                }
                _ => self.modal = Some(Modal::Find(find)),
            },
            Modal::History(mut history) => {
                let match_count = history.matches().len();
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Up => {
                        history.selected = history.selected.saturating_sub(1);
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Down => {
                        history.selected =
                            (history.selected + 1).min(match_count.saturating_sub(1));
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Home => {
                        history.selected = 0;
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::End => {
                        history.selected = match_count.saturating_sub(1);
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Backspace => {
                        if history.cursor > 0 {
                            history.cursor -= 1;
                            history.query.remove(history.cursor);
                            history.selected = 0;
                        }
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Delete => {
                        if history.cursor < history.query.len() {
                            history.query.remove(history.cursor);
                            history.selected = 0;
                        }
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Left => {
                        history.cursor = history.cursor.saturating_sub(1);
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Right => {
                        history.cursor = (history.cursor + 1).min(history.query.len());
                        self.modal = Some(Modal::History(history));
                    }
                    KeyCode::Enter => {
                        let matches = history.matches();
                        let selected = matches
                            .get(history.selected)
                            .and_then(|index| history.entries.get(*index))
                            .cloned();
                        match selected.and_then(|location| location.parse().ok()) {
                            Some(location) => self.navigate_active_to(location),
                            None => self.set_status("No matching directory history entry"),
                        }
                    }
                    KeyCode::Char(character)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
                    {
                        history.insert(character);
                        self.modal = Some(Modal::History(history));
                    }
                    _ => self.modal = Some(Modal::History(history)),
                }
            }
            Modal::ConfirmDelete {
                paths,
                trash_available,
            } => match key.code {
                KeyCode::Char('t') | KeyCode::Enter if trash_available => self.start_trash(paths),
                KeyCode::Char('p') | KeyCode::Char('y') => self.start_delete(paths),
                KeyCode::Enter => self.start_delete(paths),
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => {
                    self.modal = Some(Modal::ConfirmDelete {
                        paths,
                        trash_available,
                    })
                }
            },
            Modal::ConfirmClean {
                path,
                dirs,
                files,
                bytes,
            } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if let Some(session) = self.analyze.as_mut() {
                        session.run_clean();
                    }
                    self.set_status(format!("Cleaning {}…", path.display()));
                }
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => {
                    self.modal = Some(Modal::ConfirmClean {
                        path,
                        dirs,
                        files,
                        bytes,
                    })
                }
            },
            Modal::Conflict { job_id, path } => {
                let choice = match key.code {
                    KeyCode::Char('o') => Some(ConflictChoice::Overwrite),
                    KeyCode::Char('a') => Some(ConflictChoice::OverwriteAll),
                    KeyCode::Char('s') => Some(ConflictChoice::Skip),
                    KeyCode::Char('n') => Some(ConflictChoice::SkipAll),
                    KeyCode::Esc => Some(ConflictChoice::Cancel),
                    _ => None,
                };
                if let Some(choice) = choice {
                    self.jobs.answer_conflict(job_id, choice);
                    self.show_next_conflict();
                } else {
                    self.modal = Some(Modal::Conflict { job_id, path });
                }
            }
            Modal::QuitJobs => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.jobs.cancel_all();
                    self.should_quit = true;
                }
                KeyCode::Char('n') | KeyCode::Esc => self.modal = None,
                _ => self.modal = Some(Modal::QuitJobs),
            },
            Modal::Input(mut input) => {
                if input.supports_background()
                    && ((key.modifiers.contains(KeyModifiers::CONTROL)
                        && (key.code == KeyCode::Char('b') || key.code == KeyCode::Char('B')))
                        || (key.modifiers.contains(KeyModifiers::SHIFT)
                            && key.code == KeyCode::Enter))
                {
                    self.submit_input(input, LaunchMode::Background);
                    return;
                }
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => self.submit_input(input, LaunchMode::Foreground),
                    KeyCode::Left => input.cursor = input.cursor.saturating_sub(1),
                    KeyCode::Right => input.cursor = (input.cursor + 1).min(input.value.len()),
                    KeyCode::Home => input.cursor = 0,
                    KeyCode::End => input.cursor = input.value.len(),
                    KeyCode::Backspace => {
                        if input.cursor > 0 {
                            input.cursor -= 1;
                            input.value.remove(input.cursor);
                        }
                        self.modal = Some(Modal::Input(input));
                    }
                    KeyCode::Delete => {
                        if input.cursor < input.value.len() {
                            input.value.remove(input.cursor);
                        }
                        self.modal = Some(Modal::Input(input));
                    }
                    KeyCode::Char(character)
                        if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                    {
                        input.insert(character);
                        self.modal = Some(Modal::Input(input));
                    }
                    _ => self.modal = Some(Modal::Input(input)),
                }
            }
        }
    }
}
