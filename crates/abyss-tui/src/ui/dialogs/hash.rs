use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{HashCreateDialog, HashCreateField};
use crate::hashing::HashDatabaseFormat;
use crate::ui::dialogs::render_dialog_buttons;
use crate::ui::helpers::{centered, fit};
use crate::ui::layout::{DialogButton, LayoutInfo};
use crate::ui::theme::{DIALOG, FOCUSED, HEADER};

pub(crate) fn render_hash_create_dialog(
    frame: &mut Frame,
    outer: Rect,
    dialog: &HashCreateDialog,
    layout: &mut LayoutInfo,
) {
    let area = centered(outer, 78, 13);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Create hash database ")
            .style(DIALOG),
        area,
    );
    let mut filename = dialog.filename.clone();
    if dialog.focus == HashCreateField::Filename {
        filename.insert(dialog.cursor.min(filename.len()), '│');
    }
    let rows = [
        (
            HashCreateField::Filename,
            "Filename",
            filename.into_iter().collect::<String>(),
        ),
        (
            HashCreateField::Algorithm,
            "Algorithm",
            format!("‹ {} ›", dialog.algorithm.display_name()),
        ),
        (
            HashCreateField::Format,
            "Format",
            format!(
                "‹ {} ›",
                match dialog.format {
                    HashDatabaseFormat::Quichash => "QuicHash (.qh)",
                    HashDatabaseFormat::Hashdeep => "hashdeep (.hashdeep)",
                }
            ),
        ),
        (
            HashCreateField::Compression,
            "Compression",
            if dialog.compression_enabled() {
                if dialog.compressed {
                    "XZ (.qh.xz)"
                } else {
                    "Off"
                }
            } else {
                "N/A for hashdeep"
            }
            .to_owned(),
        ),
        (
            HashCreateField::Parallel,
            "Processing",
            if dialog.parallel {
                "Multicore"
            } else {
                "Sequential"
            }
            .to_owned(),
        ),
    ];
    for (offset, (field, label, value)) in rows.into_iter().enumerate() {
        let rect = Rect::new(
            area.x + 2,
            area.y + 1 + offset as u16,
            area.width.saturating_sub(4),
            1,
        );
        let enabled = field != HashCreateField::Compression || dialog.compression_enabled();
        let style = if field == dialog.focus {
            FOCUSED
        } else if enabled {
            DIALOG
        } else {
            Style::new().fg(Color::DarkGray).bg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(format!("{label:<13} {value}")).style(style),
            rect,
        );
        layout.hash_fields.push((field, rect));
    }
    let detail = format!(
        "{} selected  •  Tab/↑↓ fields  ←→ change  Space toggle",
        dialog.sources.len()
    );
    frame.render_widget(
        Paragraph::new(fit(&detail, area.width.saturating_sub(4) as usize)).style(HEADER),
        Rect::new(
            area.x + 2,
            area.bottom().saturating_sub(4),
            area.width.saturating_sub(4),
            1,
        ),
    );
    render_dialog_buttons(
        frame,
        area,
        &[
            (DialogButton::Start, "[ Enter  Start ]"),
            (DialogButton::Background, "[ B  Background ]"),
            (DialogButton::Cancel, "[ Esc  Cancel ]"),
        ],
        layout,
    );
}

pub(crate) fn render_hash_verify_dialog(
    frame: &mut Frame,
    outer: Rect,
    database: &Path,
    layout: &mut LayoutInfo,
) {
    let area = centered(outer, 78, 9);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Check hashes ")
            .style(DIALOG),
        area,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Database: {}\nRoot: {}\n\nVerify every stored file and algorithm.",
            database.display(),
            database
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .display()
        ))
        .style(DIALOG)
        .wrap(Wrap { trim: false }),
        Rect::new(
            area.x + 2,
            area.y + 1,
            area.width.saturating_sub(4),
            area.height.saturating_sub(4),
        ),
    );
    render_dialog_buttons(
        frame,
        area,
        &[
            (DialogButton::Start, "[ Enter  Start ]"),
            (DialogButton::Background, "[ B  Background ]"),
            (DialogButton::Cancel, "[ Esc  Cancel ]"),
        ],
        layout,
    );
}
