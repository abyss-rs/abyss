use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{ArchiveCreateDialog, ArchiveCreateField};
use crate::archive::{ArchiveContainer, CompressionMethod, CompressionPreset, CompressionThreads};
use crate::ui::dialogs::render_dialog_buttons;
use crate::ui::helpers::{centered, fit};
use crate::ui::layout::{DialogButton, LayoutInfo};
use crate::ui::theme::{DIALOG, FOCUSED, HEADER};

pub(crate) fn render_archive_create_dialog(
    frame: &mut Frame,
    outer: Rect,
    dialog: &ArchiveCreateDialog,
    layout: &mut LayoutInfo,
) {
    let area = centered(outer, 86, 20);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Create archive ")
            .style(DIALOG),
        area,
    );
    let format_label = match dialog.container {
        ArchiveContainer::Auto => "Auto (stream / tar)",
        ArchiveContainer::SevenZip => "7z",
        ArchiveContainer::Zip => "ZIP",
        ArchiveContainer::Tar => "TAR",
    };
    let method_label = match dialog.method {
        CompressionMethod::Store => "Store",
        CompressionMethod::Zstd => "Zstandard",
        CompressionMethod::Gzip => "Gzip",
        CompressionMethod::Xz => "XZ",
        CompressionMethod::Bzip2 => "Bzip2",
        CompressionMethod::Lz4 => "LZ4",
        CompressionMethod::Brotli => "Brotli",
        CompressionMethod::Lzma2 => "LZMA2",
        CompressionMethod::Lzma => "LZMA",
        CompressionMethod::Ppmd => "PPMd",
        CompressionMethod::Deflate => "Deflate",
    };
    let preset_label = if dialog.level_enabled() {
        match dialog.preset {
            CompressionPreset::Fast => "Fast",
            CompressionPreset::Balanced => "Balanced",
            CompressionPreset::Maximum => "Maximum",
            CompressionPreset::Ultra => "Ultra",
            CompressionPreset::Custom => "Custom",
        }
    } else {
        "N/A"
    };
    let level = if dialog.level_enabled() {
        dialog.level.to_string()
    } else {
        "N/A".to_owned()
    };
    let threads = if dialog.threads_enabled() {
        match dialog.threads {
            CompressionThreads::Auto => "Auto".to_owned(),
            CompressionThreads::Count(value) => value.to_string(),
        }
    } else {
        "1 (method)".to_owned()
    };
    let solid = match dialog.container {
        ArchiveContainer::SevenZip => {
            if dialog.solid {
                "On"
            } else {
                "Off"
            }
        }
        ArchiveContainer::Zip => "No (format)",
        ArchiveContainer::Tar => {
            if dialog.method == CompressionMethod::Store {
                "N/A"
            } else {
                "Yes (inherent)"
            }
        }
        ArchiveContainer::Auto => {
            if dialog.pack_tar() {
                "Yes (inherent)"
            } else {
                "N/A"
            }
        }
    };
    let encryption = if dialog.encryption_enabled() {
        if !dialog.encryption {
            "Off"
        } else if dialog.container == ArchiveContainer::SevenZip {
            "AES-256 + encrypted names"
        } else {
            "AES-256 (names visible)"
        }
    } else {
        "N/A"
    };
    let mut filename = dialog.filename.clone();
    if dialog.focus == ArchiveCreateField::Filename {
        filename.insert(dialog.cursor.min(filename.len()), '│');
    }
    let filename = filename.into_iter().collect::<String>();
    let mut rows = vec![
        (ArchiveCreateField::Filename, "Filename", filename),
        (
            ArchiveCreateField::Format,
            "Format",
            format!("‹ {format_label} ›"),
        ),
        (
            ArchiveCreateField::Method,
            "Method",
            format!("‹ {method_label} ›"),
        ),
        (
            ArchiveCreateField::Preset,
            "Preset",
            format!("‹ {preset_label} ›"),
        ),
        (ArchiveCreateField::Level, "Level", format!("‹ {level} ›")),
        (
            ArchiveCreateField::Threads,
            "Threads",
            format!("‹ {threads} ›"),
        ),
        (ArchiveCreateField::Solid, "Solid", solid.to_owned()),
        (
            ArchiveCreateField::Encryption,
            "Encryption",
            encryption.to_owned(),
        ),
    ];
    if dialog.encryption {
        let masked = |length: usize, cursor: usize, focused: bool| {
            let mut value = vec!['•'; length];
            if focused {
                value.insert(cursor.min(value.len()), '│');
            }
            value.into_iter().collect::<String>()
        };
        rows.push((
            ArchiveCreateField::Password,
            "Password",
            masked(
                dialog.password.len(),
                dialog.password_cursor,
                dialog.focus == ArchiveCreateField::Password,
            ),
        ));
        rows.push((
            ArchiveCreateField::ConfirmPassword,
            "Confirm",
            masked(
                dialog.password_confirmation.len(),
                dialog.confirmation_cursor,
                dialog.focus == ArchiveCreateField::ConfirmPassword,
            ),
        ));
    }
    let disabled = Style::new().fg(Color::DarkGray).bg(Color::Gray);
    let visible_rows = usize::from(area.height.saturating_sub(6)).clamp(1, rows.len());
    let focused = rows
        .iter()
        .position(|(field, _, _)| *field == dialog.focus)
        .unwrap_or(0);
    let first_row = focused
        .saturating_sub(visible_rows / 2)
        .min(rows.len().saturating_sub(visible_rows));
    for (offset, (field, label, value)) in rows
        .into_iter()
        .skip(first_row)
        .take(visible_rows)
        .enumerate()
    {
        let rect = Rect::new(
            area.x + 2,
            area.y + 1 + offset as u16,
            area.width.saturating_sub(4),
            1,
        );
        let enabled = match field {
            ArchiveCreateField::Preset | ArchiveCreateField::Level => dialog.level_enabled(),
            ArchiveCreateField::Threads => dialog.threads_enabled(),
            ArchiveCreateField::Solid => dialog.container == ArchiveContainer::SevenZip,
            ArchiveCreateField::Encryption => dialog.encryption_enabled(),
            _ => true,
        };
        let style = if dialog.focus == field {
            FOCUSED
        } else if enabled {
            DIALOG
        } else {
            disabled
        };
        frame.render_widget(
            Paragraph::new(format!("{label:<12} {value}")).style(style),
            rect,
        );
        layout.archive_fields.push((field, rect));
    }
    let suffix = crate::archive::create_suffix(dialog.container, dialog.method, dialog.pack_tar());
    let detail = if area.width >= 72 {
        format!(
            "{} selected  •  output {suffix}  •  Tab/↑↓ fields  ←→ change  Space toggle",
            dialog.sources.len()
        )
    } else {
        format!("{} selected  •  output {suffix}", dialog.sources.len())
    };
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
