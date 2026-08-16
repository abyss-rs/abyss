use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;
use crate::browser::{BrowserKind, SourceProbeStatus};
use crate::jobs::JobOutcome;
use crate::ui::helpers::{
    display_width, fit, fit_filename, pad_right, take_prefix_columns, take_suffix_columns,
};
use crate::ui::layout::{ActionButton, LayoutInfo};
use crate::ui::theme::{ERROR, FOCUSED};

pub(crate) fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let pane = &app.panes[app.active];
    let marks = if pane.showing_sources() {
        0
    } else {
        pane.marks.len()
    };
    let current = pane
        .selected_source()
        .map(|entry| entry.source.name.clone())
        .or_else(|| {
            pane.current()
                .filter(|entry| entry.kind != BrowserKind::Parent)
                .map(|entry| entry.name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| pane.display_path());
    let selected_source_error = pane
        .selected_source()
        .and_then(|entry| match &entry.status {
            SourceProbeStatus::Unavailable(error) => Some(error.as_str()),
            _ => None,
        });
    let width = area.width as usize;
    let active_notice = app.completion_notice.as_ref().map(|notice| {
        let marker = match notice.outcome {
            JobOutcome::Succeeded => "✓",
            JobOutcome::Failed(_) => "!",
            JobOutcome::Cancelled => "×",
        };
        format!("{marker} {}", notice.text)
    });
    let active_notice =
        active_notice.or_else(|| (app.status != "Ready").then(|| app.status.clone()));
    let text = if let Some(notice) = active_notice {
        notice_line(&notice, width)
    } else {
        let details = status_details(marks, selected_source_error);
        let full = format!(" {current}{details}");
        if display_width(&full) <= width {
            pad_right(&full, width)
        } else {
            pad_right(
                &format!(" {}", fit_filename(&current, width.saturating_sub(1))),
                width,
            )
        }
    };
    let style = match app.completion_notice.as_ref().map(|notice| &notice.outcome) {
        Some(JobOutcome::Succeeded) => Style::new().fg(Color::Black).bg(Color::Green),
        Some(JobOutcome::Failed(_)) => ERROR,
        Some(JobOutcome::Cancelled) => Style::new().fg(Color::Black).bg(Color::Yellow),
        None => FOCUSED,
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

pub(crate) fn notice_line(notice: &str, width: usize) -> String {
    pad_right(&truncate_notice(&format!(" {notice}"), width), width)
}

pub(crate) fn truncate_notice(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width < 5 {
        return fit(value, width);
    }
    // Operation names are short, while the useful result (a destination,
    // error, or optional statistics) tends to be at the end of the notice.
    let left = (width - 1) / 3;
    let right = width - left - 1;
    format!(
        "{}…{}",
        take_prefix_columns(value, left),
        take_suffix_columns(value, right)
    )
}

pub(crate) fn status_details(marks: usize, persistent_status: Option<&str>) -> String {
    match (marks, persistent_status) {
        (0, None) => String::new(),
        (_, None) => format!("  [{marks} marked]"),
        (0, Some(status)) => format!(" — {status}"),
        (_, Some(status)) => format!("  [{marks} marked] — {status}"),
    }
}

pub(crate) fn render_buttons(frame: &mut Frame, app: &App, area: Rect, layout: &mut LayoutInfo) {
    const NORMAL: [ActionButton; 10] = [
        ActionButton::Help,
        ActionButton::View,
        ActionButton::Sync,
        ActionButton::Analyze,
        ActionButton::Copy,
        ActionButton::Move,
        ActionButton::Mkdir,
        ActionButton::Delete,
        ActionButton::Refresh,
        ActionButton::Quit,
    ];
    // Analyze: Esc Quit (leave) before 1, then cleaner-mapped digits; 0 exits the app.
    const ANALYZE: [ActionButton; 11] = [
        ActionButton::EscLeave,
        ActionButton::Help,
        ActionButton::View,
        ActionButton::Sync,
        ActionButton::Analyze,
        ActionButton::Copy,
        ActionButton::Move,
        ActionButton::Mkdir,
        ActionButton::Delete,
        ActionButton::Refresh,
        ActionButton::Quit,
    ];
    // Sync: Esc Exit (leave) before 1, then sync-mapped digits.
    const SYNC: [ActionButton; 11] = [
        ActionButton::EscLeave,
        ActionButton::Help,
        ActionButton::View,
        ActionButton::Sync,
        ActionButton::Analyze,
        ActionButton::Copy,
        ActionButton::Move,
        ActionButton::Mkdir,
        ActionButton::Delete,
        ActionButton::Refresh,
        ActionButton::Quit,
    ];
    frame.render_widget(
        Block::default().style(Style::new().fg(Color::Black).bg(Color::Cyan)),
        area,
    );
    let in_archive = app.panes[app.active].is_archive();
    let in_sources = app.panes[app.active].showing_sources();
    let in_analyze = app.analyze.is_some();
    let in_sync = app.sync.is_some();
    let buttons: &[ActionButton] = if in_analyze {
        &ANALYZE
    } else if in_sync {
        &SYNC
    } else {
        &NORMAL
    };
    let base = area.width / buttons.len() as u16;
    let mut x = area.x;
    for (index, button) in buttons.iter().copied().enumerate() {
        let remaining = area.right().saturating_sub(x);
        let width = if index + 1 == buttons.len() {
            remaining
        } else {
            base.min(remaining)
        };
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let disabled = button_disabled(button, in_archive, in_sources, in_analyze, in_sync);
        let label = button_label(button, in_archive, in_analyze, in_sync, disabled);
        let key = button.key();
        let key_width = display_width(key) as u16;
        let content = Line::from(vec![
            Span::styled(
                key.to_string(),
                if disabled {
                    Style::new().fg(Color::DarkGray).bg(Color::Black)
                } else {
                    Style::new().fg(Color::White).bg(Color::Black)
                },
            ),
            Span::styled(
                fit(label, width.saturating_sub(key_width) as usize),
                if disabled {
                    Style::new().fg(Color::DarkGray).bg(Color::Cyan)
                } else {
                    Style::new().fg(Color::Black).bg(Color::Cyan)
                },
            ),
        ]);
        frame.render_widget(Paragraph::new(content), rect);
        layout.buttons.push((button, rect));
        x = x.saturating_add(width);
    }
}

pub(crate) fn button_disabled(
    button: ActionButton,
    in_archive: bool,
    in_sources: bool,
    in_analyze: bool,
    in_sync: bool,
) -> bool {
    if in_sync {
        return button == ActionButton::Analyze;
    }
    if in_analyze {
        // Esc Quit leaves Analyze; digits: 1 Help, 5 Sort, 6 Clean, 8 Delete, 9 Refresh, 0 Exit
        return matches!(
            button,
            ActionButton::View | ActionButton::Mkdir | ActionButton::Sync | ActionButton::Analyze
        );
    }
    if in_sources {
        return !matches!(
            button,
            ActionButton::Help
                | ActionButton::Refresh
                | ActionButton::Sync
                | ActionButton::Analyze
                | ActionButton::Quit
        );
    }
    if in_archive {
        return matches!(
            button,
            ActionButton::Move | ActionButton::Delete | ActionButton::Refresh
        ) || button == ActionButton::Analyze;
    }
    false
}

pub(crate) fn button_label(
    button: ActionButton,
    in_archive: bool,
    in_analyze: bool,
    in_sync: bool,
    disabled: bool,
) -> &'static str {
    if in_sync {
        return match button {
            ActionButton::EscLeave => "Exit",
            ActionButton::Help => "Help",
            ActionButton::View => "Inspect",
            ActionButton::Sync => "Run Sync",
            ActionButton::Analyze => "",
            ActionButton::Copy => "Direction",
            ActionButton::Move => "Method",
            ActionButton::Mkdir => "Filter",
            ActionButton::Delete => "Strategy",
            ActionButton::Refresh => "Compare",
            ActionButton::Quit => "Exit",
        };
    }
    if in_analyze {
        return match button {
            ActionButton::EscLeave => "Quit",
            ActionButton::Help => "Help",
            ActionButton::View
            | ActionButton::Mkdir
            | ActionButton::Sync
            | ActionButton::Analyze => "",
            ActionButton::Copy => "Sort",
            ActionButton::Move => "Clean",
            ActionButton::Delete => "Delete",
            ActionButton::Refresh => "Refresh",
            ActionButton::Quit => "Exit",
        };
    }
    if in_archive && button == ActionButton::Copy {
        return "Extract";
    }
    if in_archive && button == ActionButton::Mkdir {
        return "Test";
    }
    // Keep slots, but do not advertise move/refresh/delete inside archives.
    if in_archive
        && disabled
        && matches!(
            button,
            ActionButton::Move | ActionButton::Refresh | ActionButton::Delete
        )
    {
        return "";
    }
    button.label()
}
