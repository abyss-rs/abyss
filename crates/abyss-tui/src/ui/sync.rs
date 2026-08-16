use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, SyncFilterMode, SyncMenuAction, SyncMenuCategory};
use crate::progress::human_bytes;
use crate::sync::{SyncComparison, SyncReason};
use crate::ui::dialogs::render_modal;
use crate::ui::helpers::{display_width, fit};
use crate::ui::layout::LayoutInfo;
use crate::ui::status::{render_buttons, render_status};
use crate::ui::theme::{CORE, DIALOG, FOCUSED, HEADER, SELECTED};

pub(crate) fn render_sync(frame: &mut Frame, app: &App, area: Rect) -> LayoutInfo {
    let button_area = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let status_area = Rect::new(area.x, area.bottom() - 2, area.width, 1);
    let top_area = Rect::new(area.x, area.y, area.width, 1);
    let content = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        status_area.y.saturating_sub(area.y + 1),
    );
    let mut layout = LayoutInfo {
        pane_rects: [Rect::default(); 2],
        console: Rect::default(),
        menu_headings: Vec::new(),
        menu_items: Vec::new(),
        sync_menu_headings: Vec::new(),
        sync_menu_items: Vec::new(),
        bookmark_rows: Vec::new(),
        bookmark_sets: Vec::new(),
        sort_menu: Rect::default(),
        sort_items: Vec::new(),
        tab_nav: Vec::new(),
        rows: Vec::new(),
        buttons: Vec::new(),
        job_rows: Vec::new(),
        dialog_buttons: Vec::new(),
        archive_fields: Vec::new(),
        hash_fields: Vec::new(),
        pane_rows: content.height.saturating_sub(3) as usize,
    };

    render_sync_top_bar(frame, app, top_area, &mut layout);
    render_sync_content(frame, app, content, &mut layout);
    render_status(frame, app, status_area);
    render_buttons(frame, app, button_area, &mut layout);

    if let Some(modal) = &app.modal {
        render_modal(frame, area, modal, &mut layout);
    } else if let Some(sync) = &app.sync
        && sync.menu.is_some()
    {
        render_sync_menu(frame, app, &mut layout);
    }

    layout
}

pub(crate) fn render_sync_top_bar(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    layout: &mut LayoutInfo,
) {
    frame.render_widget(Block::default().style(HEADER), area);
    let Some(sync) = &app.sync else {
        return;
    };
    let mut x = area.x;
    for category in SyncMenuCategory::ALL {
        let width = (display_width(category.label()) + 2) as u16;
        let rect = Rect::new(x, area.y, width.min(area.right().saturating_sub(x)), 1);
        if rect.width == 0 {
            break;
        }
        let selected = sync
            .menu
            .as_ref()
            .is_some_and(|menu| menu.category == category);
        frame.render_widget(
            Paragraph::new(format!(" {} ", category.label())).style(if selected {
                FOCUSED.add_modifier(Modifier::BOLD)
            } else {
                HEADER
            }),
            rect,
        );
        layout.sync_menu_headings.push((category, rect));
        x = rect.right();
    }

    let summary_width = area.right().saturating_sub(x);
    if summary_width > 0 {
        let summary_rect = Rect::new(x, area.y, summary_width, 1);
        let summary = format!(
            " Sync: {} [{}] ({}) ",
            sync.direction.label(),
            sync.strategy.label(),
            match sync.comparison {
                SyncComparison::Metadata => "Metadata",
                SyncComparison::Checksum => "Checksum",
                SyncComparison::DeltaSignature => "Delta SIMD",
            }
        );
        frame.render_widget(
            Paragraph::new(fit(&summary, summary_width as usize)).style(HEADER),
            summary_rect,
        );
    }
}

pub(crate) fn render_sync_content(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    _layout: &mut LayoutInfo,
) {
    let Some(sync) = &app.sync else {
        return;
    };
    let header_height = 3_u16.min(area.height);
    let header_area = Rect::new(area.x, area.y, area.width, header_height);
    let table_area = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height.saturating_sub(header_height),
    );

    // Header banner
    let src_str = sync.source.display();
    let dst_str = sync.destination.display();
    let line1 = format!(" SOURCE: {src_str}  ───➔  DESTINATION: {dst_str}");
    let line2 = format!(
        " Mode: [{}]  │  Method: [{}]  │  Direction: [{}]  │  Filter: [{}]",
        sync.strategy.label(),
        match sync.comparison {
            SyncComparison::Metadata => "Metadata (Size+mtime)",
            SyncComparison::Checksum => "Checksum (Hash)",
            SyncComparison::DeltaSignature => "Delta Signature (BLAKE3 SIMD)",
        },
        sync.direction.label(),
        sync.filter.label(),
    );
    let line3 = if sync.is_planning {
        " Scanning directories and computing differences… (Please wait)".to_string()
    } else if let Some(plan) = &sync.plan {
        let mut add_cnt = 0;
        let mut upd_cnt = 0;
        let mut delta_cnt = 0;
        let mut diff_cnt = 0;
        for f in &plan.files {
            match f.reason {
                SyncReason::Missing => add_cnt += 1,
                SyncReason::MetadataChanged => upd_cnt += 1,
                SyncReason::DeltaPatchable => delta_cnt += 1,
                SyncReason::ChecksumChanged => diff_cnt += 1,
                _ => {}
            }
        }
        format!(
            " Planned: {} file(s) ({})  │  + Add: {}  │  ~ Update: {}  │  Δ Delta: {}  │  ~ Diff: {}  │  = Unchanged: {}",
            plan.files.len(),
            human_bytes(plan.bytes),
            add_cnt,
            upd_cnt,
            delta_cnt,
            diff_cnt,
            plan.unchanged,
        )
    } else {
        " Press 7 or Enter to compare".to_string()
    };

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                fit(&line1, area.width as usize),
                FOCUSED.add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(fit(&line2, area.width as usize), CORE)),
            Line::from(Span::styled(
                fit(&line3, area.width as usize),
                SELECTED.add_modifier(Modifier::BOLD),
            )),
        ]),
        header_area,
    );

    // Diff table
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Synchronization Differences ")
            .style(DIALOG),
        table_area,
    );

    let inner = Rect::new(
        table_area.x + 1,
        table_area.y + 1,
        table_area.width.saturating_sub(2),
        table_area.height.saturating_sub(2),
    );
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Column header
    let col_header = format!(" {:<8} {:<45} {:<15}", "Action", "Relative Path", "Reason");
    frame.render_widget(
        Paragraph::new(Span::styled(
            fit(&col_header, inner.width as usize),
            HEADER.add_modifier(Modifier::BOLD),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let list_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let Some(plan) = &sync.plan else {
        if sync.is_planning {
            frame.render_widget(
                Paragraph::new("  Scanning files and comparing signatures…").style(CORE),
                list_area,
            );
        }
        return;
    };

    let files: Vec<&crate::sync::SyncFile> = match sync.filter {
        SyncFilterMode::All => plan.files.iter().collect(),
        SyncFilterMode::ChangesOnly => plan.files.iter().collect(),
    };

    if files.is_empty() {
        frame.render_widget(
            Paragraph::new("  No difference found. Directories are synchronized.").style(CORE),
            list_area,
        );
        return;
    }

    let visible_rows = list_area.height as usize;
    let selected = sync.selected_index.min(files.len().saturating_sub(1));
    let scroll_top = if selected >= visible_rows {
        selected + 1 - visible_rows
    } else {
        0
    };

    for (i, row_idx) in (scroll_top..files.len()).take(visible_rows).enumerate() {
        let file = files[row_idx];
        let is_selected = row_idx == selected;
        let badge_style = match file.reason {
            SyncReason::Missing => Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            SyncReason::MetadataChanged => {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            }
            SyncReason::DeltaPatchable => {
                Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            }
            SyncReason::ChecksumChanged => {
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            }
            SyncReason::TypeChanged => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            SyncReason::Orphaned => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        };
        let row_style = if is_selected {
            FOCUSED.add_modifier(Modifier::BOLD)
        } else {
            CORE
        };
        let row_rect = Rect::new(list_area.x, list_area.y + i as u16, list_area.width, 1);
        let badge = file.reason.badge();
        let reason_str = match file.reason {
            SyncReason::Missing => "New file",
            SyncReason::MetadataChanged => "Metadata changed",
            SyncReason::DeltaPatchable => "Delta SIMD patchable",
            SyncReason::ChecksumChanged => "Content modified",
            SyncReason::TypeChanged => "Type changed",
            SyncReason::Orphaned => "Extra at dest",
        };
        let row_line = Line::from(vec![
            Span::styled(
                format!(" {:<8} ", badge),
                if is_selected {
                    FOCUSED.add_modifier(Modifier::BOLD)
                } else {
                    badge_style
                },
            ),
            Span::styled(format!("{:<45} ", fit(&file.relative, 44)), row_style),
            Span::styled(format!("{:<15}", reason_str), row_style),
        ]);
        frame.render_widget(Paragraph::new(row_line), row_rect);
    }
}

pub(crate) fn render_sync_menu(frame: &mut Frame, app: &App, layout: &mut LayoutInfo) {
    let Some(sync) = &app.sync else {
        return;
    };
    let Some(menu) = sync.menu else {
        return;
    };
    let Some(anchor) = layout
        .sync_menu_headings
        .iter()
        .find(|(category, _)| *category == menu.category)
        .map(|(_, rect)| *rect)
    else {
        return;
    };
    let actions = SyncMenuAction::for_category(menu.category);
    let content_width = actions
        .iter()
        .map(|action| display_width(action.label()) + display_width(action.shortcut()) + 7)
        .max()
        .unwrap_or(24)
        .max(display_width(menu.category.label()) + 4);
    let width = (content_width as u16).min(frame.area().width).max(1);
    let available_height = frame.area().bottom().saturating_sub(anchor.y + 1).max(1);
    let wanted_rows = actions.len().max(1) as u16;
    let height = wanted_rows.saturating_add(2).min(available_height);
    let x = anchor.x.min(frame.area().right().saturating_sub(width));
    let area = Rect::new(x, anchor.y + 1, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", menu.category.label()))
            .style(DIALOG),
        area,
    );

    let inner_width = area.width.saturating_sub(2) as usize;
    for (index, &action) in actions.iter().enumerate() {
        if index as u16 >= height.saturating_sub(2) {
            break;
        }
        let selected = index == menu.selected;
        let style = if selected {
            FOCUSED.add_modifier(Modifier::BOLD)
        } else {
            DIALOG
        };
        let label = action.label();
        let shortcut = action.shortcut();
        let gap = inner_width
            .saturating_sub(display_width(label))
            .saturating_sub(display_width(shortcut))
            .saturating_sub(2);
        let text = format!(" {}{}{shortcut} ", label, " ".repeat(gap));
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + index as u16,
            area.width.saturating_sub(2),
            1,
        );
        frame.render_widget(Paragraph::new(fit(&text, inner_width)).style(style), rect);
        layout.sync_menu_items.push((action, rect));
    }
}
