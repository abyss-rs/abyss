use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Difference};
use crate::browser::{BrowserEntry, BrowserKind, SourceProbeStatus};
use crate::ui::helpers::{
    fit, fit_filename, format_age, human_bytes_compact, pad_left, pad_right, truncate_middle,
};
use crate::ui::layout::LayoutInfo;
use crate::ui::theme::{CORE, ERROR, FOCUSED, HEADER, MARK_SELECTED, MARKED, SELECTED};

pub(crate) fn render_pane(
    frame: &mut Frame,
    app: &App,
    pane_index: usize,
    area: Rect,
    layout: &mut LayoutInfo,
) {
    let pane = &app.panes[pane_index];
    let active = app.active == pane_index;
    let border_style = if active {
        Style::new()
            .fg(Color::Cyan)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        CORE
    };
    let tab_label = format!(" [ {}/{} ] ", pane.active_tab() + 1, pane.tab_count());
    let tab_width = tab_label.len();
    // Leave room for borders, path title padding, and the right-aligned tab counter.
    let title_width = area.width.saturating_sub(6 + tab_width as u16) as usize;
    let mut display = if pane.showing_sources() {
        format!("Sources — {}", pane.display_path())
    } else if pane.is_archive() {
        format!("[RO] {}", pane.display_path())
    } else {
        pane.display_path()
    };
    if app.synchronized_scrolling {
        display.insert_str(0, "[SYNC] ");
    }
    if app.comparison {
        display.insert_str(0, "[DIFF] ");
    }
    if pane.has_pvc_warning {
        display.insert_str(0, "[PVC >85%] ");
    }
    let title = truncate_middle(&display, title_width);
    let title = if active {
        format!("▶ {title}")
    } else {
        format!("  {title}")
    };
    let title_style = if active { FOCUSED } else { HEADER };
    let tab_style = if active { FOCUSED } else { HEADER };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(format!(" {title} "), title_style))
            .title(Line::from(Span::styled(tab_label.clone(), tab_style)).right_aligned())
            .border_style(border_style)
            .style(CORE),
        area,
    );

    // Hit targets match ratatui's right-aligned title placement (inside side borders).
    if area.width > 2 && tab_width >= 3 {
        let titles_right = area.x + area.width - 1;
        let title_x = titles_right.saturating_sub(tab_width as u16);
        // " [ N/M ] " — '[' at index 1, ']' at len-2
        layout
            .tab_nav
            .push((pane_index, -1, Rect::new(title_x + 1, area.y, 1, 1)));
        layout.tab_nav.push((
            pane_index,
            1,
            Rect::new(title_x + tab_width as u16 - 2, area.y, 1, 1),
        ));
    }

    let inner_width = area.width.saturating_sub(2);
    if inner_width == 0 || area.height < 3 {
        return;
    }
    let header_area = Rect::new(area.x + 1, area.y + 1, inner_width, 1);
    if let Some(source_view) = &pane.source_view {
        frame.render_widget(
            Paragraph::new(source_columns(
                "Provider",
                "Source / profile / context",
                "Endpoint / context",
                "Status",
                inner_width,
            ))
            .style(HEADER),
            header_area,
        );
        for row in 0..layout.pane_rows {
            let index = source_view.offset + row;
            let Some(entry) = source_view.entries.get(index) else {
                break;
            };
            let row_area = Rect::new(area.x + 1, area.y + 2 + row as u16, inner_width, 1);
            let selected = active && index == source_view.selected;
            let style = if selected {
                SELECTED
            } else {
                match entry.status {
                    SourceProbeStatus::Checking => Style::new().fg(Color::Yellow).bg(Color::Blue),
                    SourceProbeStatus::Ready => Style::new().fg(Color::Green).bg(Color::Blue),
                    SourceProbeStatus::Unavailable(_) => {
                        Style::new().fg(Color::LightRed).bg(Color::Blue)
                    }
                }
            };
            let detail = if entry.source.endpoint.is_empty() {
                &entry.source.context
            } else {
                &entry.source.endpoint
            };
            let source_name = if entry.source.context.is_empty()
                || entry.source.name.contains(&entry.source.context)
            {
                entry.source.name.clone()
            } else {
                format!("{} ({})", entry.source.name, entry.source.context)
            };
            frame.render_widget(
                Paragraph::new(source_columns(
                    &entry.source.provider,
                    &source_name,
                    detail,
                    entry.status.label(),
                    inner_width,
                ))
                .style(style),
                row_area,
            );
            layout.rows.push((pane_index, index, row_area));
        }
        return;
    }
    frame.render_widget(
        Paragraph::new(columns("Name", "Size", "Age", inner_width, false)).style(HEADER),
        header_area,
    );

    if let Some(error) = &pane.error {
        let error_area = Rect::new(
            area.x + 1,
            area.y + 2,
            inner_width,
            area.height.saturating_sub(3),
        );
        frame.render_widget(
            Paragraph::new(error.as_str())
                .style(ERROR)
                .wrap(Wrap { trim: true }),
            error_area,
        );
        return;
    }
    if pane.loading && pane.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(" Loading…").style(CORE),
            Rect::new(area.x + 1, area.y + 2, inner_width, 1),
        );
        return;
    }

    for row in 0..layout.pane_rows {
        let index = pane.offset + row;
        let Some(entry) = pane.entries.get(index) else {
            break;
        };
        let row_area = Rect::new(area.x + 1, area.y + 2 + row as u16, inner_width, 1);
        let marked = pane.marks.contains(&entry.name);
        let selected = active && index == pane.selected;
        let style = entry_style(
            entry,
            marked,
            selected,
            app.entry_difference(pane_index, entry),
        );
        let name = entry.name.to_string_lossy();
        let size = match entry.kind {
            BrowserKind::Parent | BrowserKind::Directory => "<DIR>".to_owned(),
            _ => entry.size.map(human_bytes_compact).unwrap_or_default(),
        };
        let age = entry.modified.map(format_age).unwrap_or_default();
        frame.render_widget(
            Paragraph::new(columns(
                &name,
                &size,
                &age,
                inner_width,
                !matches!(entry.kind, BrowserKind::Parent | BrowserKind::Directory),
            ))
            .style(style),
            row_area,
        );
        layout.rows.push((pane_index, index, row_area));
    }
}

pub(crate) fn source_columns(
    provider: &str,
    source: &str,
    detail: &str,
    status: &str,
    width: u16,
) -> String {
    let width = width as usize;
    if width < 44 {
        return fit(&format!("{provider}  {source}  {status}"), width);
    }
    let provider_width = 16.min(width / 4);
    let status_width = 13;
    let detail_width = if width >= 76 { 22 } else { 0 };
    let source_width = width.saturating_sub(provider_width + status_width + detail_width + 3);
    if detail_width == 0 {
        format!(
            "{} {} {}",
            pad_right(&fit(provider, provider_width), provider_width),
            pad_right(&fit(source, source_width), source_width),
            pad_right(&fit(status, status_width), status_width),
        )
    } else {
        format!(
            "{} {} {} {}",
            pad_right(&fit(provider, provider_width), provider_width),
            pad_right(&fit(source, source_width), source_width),
            pad_right(&fit(detail, detail_width), detail_width),
            pad_right(&fit(status, status_width), status_width),
        )
    }
}

pub(crate) fn entry_style(
    entry: &BrowserEntry,
    marked: bool,
    selected: bool,
    difference: Option<Difference>,
) -> Style {
    if selected && marked {
        return MARK_SELECTED;
    }
    if selected {
        return SELECTED;
    }
    if marked {
        return MARKED;
    }
    if difference == Some(Difference::OnlyHere) {
        return Style::new()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD);
    }
    if difference == Some(Difference::Modified) {
        return Style::new().fg(Color::Black).bg(Color::Yellow);
    }
    if entry.name.as_encoded_bytes().starts_with(b"\xE2\x9A\xA0 ") {
        return ERROR.add_modifier(Modifier::BOLD);
    }
    let foreground = match entry.kind {
        BrowserKind::Parent | BrowserKind::Directory => Color::White,
        BrowserKind::Symlink => Color::Gray,
        BrowserKind::File if is_executable(entry) => Color::Green,
        _ => Color::Gray,
    };
    Style::new().fg(foreground).bg(Color::Blue)
}

#[cfg(unix)]
fn is_executable(entry: &BrowserEntry) -> bool {
    entry.mode.is_some_and(|mode| mode & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(entry: &BrowserEntry) -> bool {
    std::path::Path::new(&entry.name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "com" | "bat" | "cmd"
            )
        })
}

pub(crate) fn columns(
    name: &str,
    size: &str,
    age: &str,
    width: u16,
    preserve_extension: bool,
) -> String {
    let width = width as usize;
    if width < 22 {
        return if preserve_extension {
            fit_filename(name, width)
        } else {
            fit(name, width)
        };
    }
    let age_width = 7;
    let size_width = 10;
    let name_width = width.saturating_sub(age_width + size_width + 2);
    let name = if preserve_extension {
        fit_filename(name, name_width)
    } else {
        fit(name, name_width)
    };
    format!(
        "{} {} {}",
        pad_right(&name, name_width),
        pad_left(size, size_width),
        pad_left(age, age_width),
    )
}
