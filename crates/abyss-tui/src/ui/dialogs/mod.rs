mod archive;
mod hash;
mod input;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::dialogs::Modal;
use crate::progress::human_bytes;
use crate::ui::helpers::{centered, fit};
use crate::ui::layout::{DialogButton, LayoutInfo};
use crate::ui::theme::{DIALOG, ERROR, FOCUSED, HEADER};

pub(crate) use self::archive::render_archive_create_dialog;
pub(crate) use self::hash::{render_hash_create_dialog, render_hash_verify_dialog};
pub(crate) use self::input::render_input_dialog;

pub(crate) fn render_modal(frame: &mut Frame, area: Rect, modal: &Modal, layout: &mut LayoutInfo) {
    match modal {
        Modal::Help => render_box(
            frame,
            area,
            "Help",
            "Ctrl+Z  Open the top menu bar. Use ←/→ between Navigate, Pane, Bookmarks, and Tools; ↑/↓ selects; Enter runs; Esc closes. Menu shortcut labels use compact Mac glyphs: ⌃ Control, ⌥ Option, ⇧ Shift.\n\n↑↓/PgUp/PgDn  Navigate        Tab  Switch pane\nEnter Open     Backspace Parent    Shift+Enter Subshell\n+/= Mark       O Default app       E/V Editor      I Inspect\nS Sources    1 Help    2 View    3 Mkdir    4/c Copy/extract\n5/m Move    6/d Delete    7/r Refresh    8 (empty)    9 Analyze    0/q Quit\nA/P Archive   H Create/check hashes   J Jobs   X Directory diff\nY Differential sync      K Kubernetes PVC snapshot\nCtrl+L Synchronized scrolling     Ctrl+T/W New/close tab\n[ / ] Switch tab          Ctrl+1..9 Jump bookmark\nCtrl+Alt+1..9 Assign bookmark      Alt+H Fuzzy history\nAlt+J zoxide/autojump\n\n9 Analyze opens the full cleaner disk-usage UI on local panes. While Analyze is open the bottom bar starts with Esc Quit (leave Analyze), then 1 Help, 4 Sort, 5 Clean, 6 Delete, 7 Refresh, 0 Exit (quit app). Esc/q leave Analyze. Clean (5/c) opens a Confirm/Cancel dialog with path, folder/file counts, and size. Sort in dual-pane opens from the Sort: header click.\n\nSources replaces only the active pane. Enter opens a ready source; Enter retries an unavailable source; R rediscovers; Esc/Backspace/S closes it. File commands are disabled there.\n\nInside an archive the bar stays numbered: 1 Help, 2 View, 3 Test (background integrity check of the whole archive), 4 Extract, 0 Quit. Analyze is unavailable in archives. Slots 5/7 stay in place but inactive (no move/refresh); 6 Delete is inactive too (read-only).\n\nCreate archive/hash and hash check: Enter starts foreground; B starts background. Copy/Move/Extract use Enter or Alt+B. Up to three safe, non-overlapping jobs run at once. Archives are read only; 4/c extracts to the other pane.",
            false,
            80,
            24,
        ),
        Modal::Input(input) => {
            let mut value = if input.masked {
                vec!['•'; input.value.len()]
            } else {
                input.value.clone()
            };
            value.insert(input.cursor.min(value.len()), '│');
            let text: String = value.into_iter().collect();
            render_input_dialog(frame, area, input, &text, layout);
        }
        Modal::ArchiveCreate(dialog) => {
            render_archive_create_dialog(frame, area, dialog, layout);
        }
        Modal::HashCreate(dialog) => {
            render_hash_create_dialog(frame, area, dialog, layout);
        }
        Modal::VerifyHash(database) => {
            render_hash_verify_dialog(frame, area, database, layout);
        }
        Modal::ConfirmSync(plan) => {
            let missing = plan
                .files
                .iter()
                .filter(|file| file.reason == crate::sync::SyncReason::Missing)
                .count();
            let changed = plan.files.len().saturating_sub(missing);
            render_box(
                frame,
                area,
                "Differential sync preview",
                &format!(
                    "{} → {}\n\nMode: {:?}\n{} missing, {} changed, {} unchanged\n{} directories to create, {} to copy\n\nEnter/Y  Queue sync       N/Esc  Cancel",
                    plan.source.display(),
                    plan.destination.display(),
                    plan.comparison,
                    missing,
                    changed,
                    plan.unchanged,
                    plan.directories.len(),
                    human_bytes(plan.bytes),
                ),
                false,
                84,
                13,
            );
        }
        Modal::Find(find) => {
            let matches = find.matches();
            let height = (matches.len().min(16) as u16 + 5).max(7);
            let dialog = centered(area, 100, height);
            frame.render_widget(Clear, dialog);
            let inner_width = dialog.width.saturating_sub(4) as usize;
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} — filter, ↑↓, Enter ", find.title()))
                    .style(DIALOG),
                dialog,
            );
            let mut query = find.query.clone();
            query.insert(find.cursor.min(query.len()), '│');
            frame.render_widget(
                Paragraph::new(format!("Filter: {}", query.into_iter().collect::<String>()))
                    .style(HEADER),
                Rect::new(
                    dialog.x + 2,
                    dialog.y + 1,
                    dialog.width.saturating_sub(4),
                    1,
                ),
            );
            let visible = dialog.height.saturating_sub(4) as usize;
            let start = find.selected.saturating_sub(visible.saturating_sub(1));
            for (row, hit_index) in matches.iter().skip(start).take(visible).enumerate() {
                let match_index = start + row;
                let hit = &find.hits[*hit_index];
                // Content matches carry a line number worth showing; name
                // matches would just repeat the path.
                let text = match hit.line {
                    Some(line) => format!(
                        "{}:{line}  {}",
                        hit.path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        hit.preview.trim()
                    ),
                    None => hit.preview.clone(),
                };
                frame.render_widget(
                    Paragraph::new(fit(&text, inner_width)).style(
                        if match_index == find.selected {
                            FOCUSED
                        } else {
                            DIALOG
                        },
                    ),
                    Rect::new(
                        dialog.x + 2,
                        dialog.y + 3 + row as u16,
                        dialog.width.saturating_sub(4),
                        1,
                    ),
                );
            }
        }
        Modal::History(history) => {
            let matches = history.matches();
            let height = (matches.len().min(14) as u16 + 5).max(7);
            let dialog = centered(area, 92, height);
            frame.render_widget(Clear, dialog);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Directory history — type to filter, ↑↓, Enter ")
                    .style(DIALOG),
                dialog,
            );
            let mut query = history.query.clone();
            query.insert(history.cursor.min(query.len()), '│');
            frame.render_widget(
                Paragraph::new(format!("Search: {}", query.into_iter().collect::<String>()))
                    .style(HEADER),
                Rect::new(
                    dialog.x + 2,
                    dialog.y + 1,
                    dialog.width.saturating_sub(4),
                    1,
                ),
            );
            let visible = dialog.height.saturating_sub(4) as usize;
            let start = history.selected.saturating_sub(visible.saturating_sub(1));
            for (row, entry_index) in matches.iter().skip(start).take(visible).enumerate() {
                let match_index = start + row;
                let text = history.entries[*entry_index].display();
                frame.render_widget(
                    Paragraph::new(fit(&text, dialog.width.saturating_sub(4) as usize)).style(
                        if match_index == history.selected {
                            FOCUSED
                        } else {
                            DIALOG
                        },
                    ),
                    Rect::new(
                        dialog.x + 2,
                        dialog.y + 3 + row as u16,
                        dialog.width.saturating_sub(4),
                        1,
                    ),
                );
            }
        }
        Modal::ConfirmDelete {
            paths,
            trash_available,
        } => {
            render_box(
                frame,
                area,
                if *trash_available {
                    "Move to Trash?"
                } else {
                    "Delete permanently?"
                },
                &if *trash_available {
                    format!(
                        "Move {} selected item(s) to the operating-system Trash?\n\nEnter/T  Trash (recoverable)    P  Delete permanently    Esc  Cancel",
                        paths.len()
                    )
                } else {
                    format!(
                        "Permanently delete {} selected remote item(s)?\n\nThis cannot be undone.\n\nEnter/P  Delete       Esc  Cancel",
                        paths.len()
                    )
                },
                true,
                78,
                9,
            );
        }
        Modal::ConfirmClean {
            path,
            dirs,
            files,
            bytes,
        } => {
            render_confirm_clean_dialog(frame, area, path, *dirs, *files, *bytes, layout);
        }
        Modal::Conflict { job_id, path } => render_box(
            frame,
            area,
            &format!("Job #{job_id}: destination exists"),
            &format!(
                "{}\n\nO  Overwrite once    A  Overwrite all\nS  Skip once         N  Skip all         Esc  Cancel",
                path.display()
            ),
            true,
            76,
            9,
        ),
        Modal::Message { title, text, error } => render_box(
            frame,
            area,
            title,
            &format!("{text}\n\nEnter/Esc  Close"),
            *error,
            76,
            9,
        ),
        Modal::QuitJobs => render_box(
            frame,
            area,
            "Jobs are running",
            "Cancel all active and queued jobs, then quit?\n\nY/Enter  Quit    N/Esc  Continue",
            true,
            62,
            7,
        ),
        Modal::Inspect(dialog) => {
            let width = 84;
            let height = (dialog.lines.len() as u16 + 5).clamp(10, 24);
            let dialog_area = centered(area, width, height);
            frame.render_widget(Clear, dialog_area);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", dialog.title))
                    .style(DIALOG),
                dialog_area,
            );

            let text_area = Rect::new(
                dialog_area.x + 2,
                dialog_area.y + 1,
                dialog_area.width.saturating_sub(4),
                dialog_area.height.saturating_sub(3),
            );

            let formatted_text = dialog.lines.join("\n");
            frame.render_widget(
                Paragraph::new(formatted_text)
                    .style(DIALOG)
                    .scroll((dialog.scroll as u16, 0))
                    .wrap(Wrap { trim: false }),
                text_area,
            );

            let footer = Rect::new(
                dialog_area.x + 2,
                dialog_area.bottom().saturating_sub(2),
                dialog_area.width.saturating_sub(4),
                1,
            );
            frame.render_widget(
                Paragraph::new("[ Esc / Enter / i  Close ]")
                    .style(FOCUSED)
                    .alignment(ratatui::layout::Alignment::Center),
                footer,
            );
        }
    }
}

pub(crate) fn render_confirm_clean_dialog(
    frame: &mut Frame,
    outer: Rect,
    path: &std::path::Path,
    dirs: usize,
    files: usize,
    bytes: u64,
    layout: &mut LayoutInfo,
) {
    let area = centered(outer, 78, 12);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Clean temporary files ")
            .style(DIALOG),
        area,
    );

    let inner = Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(4),
    );
    let path_line = fit(&path.display().to_string(), inner.width as usize);
    let size = human_bytes(bytes);
    let body = format!(
        "Remove matched temp folders and files under:\n\n{path_line}\n\n  Folders   {dirs}\n  Files     {files}\n  Size      {size}"
    );
    frame.render_widget(
        Paragraph::new(body)
            .style(DIALOG)
            .wrap(Wrap { trim: false }),
        inner,
    );

    render_dialog_buttons(
        frame,
        area,
        &[
            (DialogButton::Start, "[ Enter  Confirm ]"),
            (DialogButton::Cancel, "[ Esc  Cancel ]"),
        ],
        layout,
    );
}

pub(crate) fn render_dialog_buttons(
    frame: &mut Frame,
    area: Rect,
    buttons: &[(DialogButton, &str)],
    layout: &mut LayoutInfo,
) {
    let total = buttons
        .iter()
        .map(|(_, label)| label.chars().count() as u16)
        .sum::<u16>()
        .saturating_add((buttons.len().saturating_sub(1) * 2) as u16);
    let mut x = area.x + area.width.saturating_sub(total) / 2;
    let y = area.bottom().saturating_sub(2);
    for &(button, label) in buttons {
        let width = label.chars().count() as u16;
        let rect = Rect::new(x, y, width, 1);
        frame.render_widget(Paragraph::new(label).style(FOCUSED), rect);
        layout.dialog_buttons.push((button, rect));
        x = x.saturating_add(width + 2);
    }
}

pub(crate) fn render_box(
    frame: &mut Frame,
    outer: Rect,
    title: &str,
    text: &str,
    error: bool,
    width: u16,
    height: u16,
) {
    let area = centered(outer, width, height);
    frame.render_widget(Clear, area);
    let style = if error { ERROR } else { DIALOG };
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {title} "))
                    .border_style(style),
            )
            .style(style)
            .wrap(Wrap { trim: false }),
        area,
    );
}
