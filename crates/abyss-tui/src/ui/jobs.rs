use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph};

use crate::app::App;
use crate::highlight::Piece;
use crate::jobs::{Job, JobState};
use crate::progress::human_bytes;
use crate::ui::helpers::{
    background_job_label, background_job_state, centered, compact_job_state, fit, format_percent,
    inset, job_eta, operation_body, operation_speed_line, phase_name, truncate_middle,
};
use crate::ui::layout::LayoutInfo;
use crate::ui::theme::{
    CORE, DIALOG, FOCUSED, HEADER, OPERATION_DIALOG_HEIGHT, OPERATION_DIALOG_WIDTH,
};
use crate::viewer::{Viewer, ViewerMode};

/// Slice one highlighted line to the visible window, keeping each run's style.
///
/// `skip` and `width` are counted in characters, matching how the plain-text
/// path scrolls, so both stay in step when highlighting is unavailable.
fn highlighted_line(pieces: &[Piece], skip: usize, width: usize) -> Line<'static> {
    let mut remaining_skip = skip;
    let mut remaining_width = width;
    let mut spans = Vec::new();
    for piece in pieces {
        if remaining_width == 0 {
            break;
        }
        let length = piece.text.chars().count();
        if remaining_skip >= length {
            remaining_skip -= length;
            continue;
        }
        let visible: String = piece
            .text
            .chars()
            .skip(remaining_skip)
            .take(remaining_width)
            .collect();
        remaining_skip = 0;
        remaining_width -= visible.chars().count();
        let mut style = Style::new().fg(piece.color);
        if piece.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if piece.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        spans.push(Span::styled(visible, style));
    }
    Line::from(spans)
}

pub(crate) fn render_viewer(
    frame: &mut Frame,
    area: Rect,
    viewer: &Viewer,
    highlight: Option<&Vec<Vec<Piece>>>,
) {
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2));
    let mode = match viewer.mode {
        ViewerMode::Text => "text",
        ViewerMode::Hex => "hex",
    };
    let suffix = if viewer.truncated {
        " — first 8 MiB"
    } else {
        ""
    };
    let title = format!(" {} [{mode}{suffix}] ", viewer.path.display());
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(title, HEADER))
            .border_style(Style::new().fg(Color::White).bg(Color::Blue))
            .style(CORE),
        body,
    );
    let rows = body.height.saturating_sub(2) as usize;
    let width = body.width.saturating_sub(2) as usize;
    let lines = viewer
        .lines
        .iter()
        .enumerate()
        .skip(viewer.vertical)
        .take(rows)
        .map(
            |(index, line)| match highlight.and_then(|all| all.get(index)) {
                Some(pieces) => highlighted_line(pieces, viewer.horizontal, width),
                None => {
                    let visible: String =
                        line.chars().skip(viewer.horizontal).take(width).collect();
                    Line::from(visible)
                }
            },
        )
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(CORE),
        Rect::new(
            body.x + 1,
            body.y + 1,
            body.width.saturating_sub(2),
            body.height.saturating_sub(2),
        ),
    );
    frame.render_widget(
        Paragraph::new(format!(
            " Line {}/{}  Col {}",
            viewer.vertical.saturating_add(1),
            viewer.lines.len(),
            viewer.horizontal.saturating_add(1)
        ))
        .style(FOCUSED),
        Rect::new(area.x, area.bottom() - 2, area.width, 1),
    );
    frame.render_widget(
        Paragraph::new("2 View/Close   ↑↓ PgUp/PgDn Scroll   ←→ Horizontal   0/q Quit viewer")
            .style(Style::new().fg(Color::Black).bg(Color::Cyan)),
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
}

pub(crate) fn render_background_jobs(
    frame: &mut Frame,
    area: Rect,
    jobs: &[&Job],
    layout: &mut LayoutInfo,
) {
    for (row, job) in jobs.iter().enumerate() {
        let rect = Rect::new(area.x, area.y + row as u16, area.width, 1);
        let snapshot = job.snapshot();
        let current = snapshot
            .current_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let speed = job.speed.map(|speed| speed.current);
        let label = background_job_label(
            job.id,
            job.kind,
            background_job_state(job, &snapshot),
            &current,
            snapshot.ratio(),
            speed,
            job_eta(job, &snapshot),
            rect.width as usize,
        );
        let color = match job.state {
            JobState::WaitingConflict(_) => Color::Yellow,
            JobState::Paused => Color::Magenta,
            JobState::Cancelling => Color::Red,
            _ => Color::Cyan,
        };
        frame.render_widget(
            Gauge::default()
                .ratio(snapshot.ratio())
                .gauge_style(
                    Style::new()
                        .fg(color)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
                .label(label),
            rect,
        );
        layout.job_rows.push((job.id, rect));
    }
}

pub(crate) fn render_operation(frame: &mut Frame, area: Rect, job: &Job) {
    let snapshot = job.snapshot();
    let phase = phase_name(snapshot.phase);
    let box_area = centered(area, OPERATION_DIALOG_WIDTH, OPERATION_DIALOG_HEIGHT);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" #{} {} ", job.id, job.kind.title()))
            .style(DIALOG)
            .border_style(Style::new().fg(Color::Black).bg(Color::Gray)),
        box_area,
    );
    let inside = inset(box_area, 2, 1);
    let current = snapshot
        .current_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let queued_status =
        matches!(job.state, JobState::Queued(_)).then(|| compact_job_state(job, &snapshot));
    let body = operation_body(
        phase,
        &current,
        queued_status.as_deref(),
        &snapshot,
        inside.width as usize,
    );
    frame.render_widget(
        Paragraph::new(body).style(DIALOG),
        Rect::new(inside.x, inside.y, inside.width, 5),
    );
    let gauge_area = Rect::new(inside.x, inside.y + 5, inside.width, 1);
    frame.render_widget(
        Gauge::default()
            .ratio(snapshot.ratio())
            .gauge_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .label(format_percent(snapshot.ratio())),
        gauge_area,
    );
    let speed = operation_speed_line(job.speed, inside.width as usize);
    frame.render_widget(
        Paragraph::new(format!("{speed}\nEsc  Cancel")).style(DIALOG),
        Rect::new(inside.x, inside.y + 7, inside.width, 2),
    );
}

pub(crate) fn render_jobs_panel(
    frame: &mut Frame,
    outer: Rect,
    app: &App,
    layout: &mut LayoutInfo,
) {
    let history = app.jobs.history();
    let height = (history.len().min(15) as u16 + 6).min(outer.height.saturating_sub(2));
    let area = centered(outer, 92, height.max(8));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Jobs — ↑↓ Select  P Pause/resume  Ctrl+↑↓ Reorder  B Limit  C Cancel ")
            .style(DIALOG),
        area,
    );
    let selected = app.jobs_panel.and_then(|panel| panel.selected);
    let rows = area.height.saturating_sub(5) as usize;
    for (row, job) in history.iter().take(rows).enumerate() {
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + row as u16,
            area.width.saturating_sub(2),
            1,
        );
        let snapshot = job.snapshot();
        let state = compact_job_state(job, &snapshot);
        let speed = job
            .speed
            .map(|speed| format!("{}/s", human_bytes(speed.current)))
            .unwrap_or_default();
        let text = format!(
            " #{:<3} {:<13} {:<18} {:>5.1}%  {:>12}",
            job.id,
            job.kind.title(),
            state,
            snapshot.ratio() * 100.0,
            speed
        );
        frame.render_widget(
            Paragraph::new(fit(&text, rect.width as usize)).style(if selected == Some(job.id) {
                FOCUSED
            } else {
                DIALOG
            }),
            rect,
        );
        layout.job_rows.push((job.id, rect));
    }
    if let Some(job) = selected.and_then(|id| app.jobs.job(id)) {
        let snapshot = job.snapshot();
        let path = snapshot.current_path.to_string_lossy();
        let detail = format!(
            "{}  Items {}/{}  Logical {}/{}",
            truncate_middle(&path, area.width.saturating_sub(4) as usize),
            snapshot.objects_done,
            snapshot.total_objects,
            human_bytes(snapshot.logical_done),
            human_bytes(snapshot.total_bytes)
        );
        frame.render_widget(
            Paragraph::new(fit(&detail, area.width.saturating_sub(4) as usize)).style(HEADER),
            Rect::new(
                area.x + 2,
                area.bottom().saturating_sub(2),
                area.width.saturating_sub(4),
                1,
            ),
        );
    }
}
