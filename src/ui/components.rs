//! Reusable UI components for the TUI.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, AppMode, Progress, ProgressStage, SyncStatus};

/// Render the help bar with context-sensitive key bindings.
pub fn render_help_bar(f: &mut Frame, area: Rect, app: &App) {
    let help_text = build_help_text(app);
    
    let help = Paragraph::new(Line::from(help_text))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    
    f.render_widget(help, area);
}

/// Build help text based on current app state.
fn build_help_text(app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    // Style helpers
    let key_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let sep_style = Style::default().fg(Color::DarkGray);
    let text_style = Style::default().fg(Color::White);
    
    match app.mode {
        AppMode::ConfirmDelete => {
            spans.push(Span::styled("Y", key_style));
            spans.push(Span::styled(":Confirm ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" N/Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
        AppMode::SelectStorage | AppMode::SelectNamespace | AppMode::SelectPvc | 
        AppMode::SelectPv | AppMode::SelectCloudProvider | AppMode::ConfigureCloud => {
            spans.push(Span::styled("↑↓", key_style));
            spans.push(Span::styled(":Navigate ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Enter", key_style));
            spans.push(Span::styled(":Select ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
        AppMode::DiskAnalyzer => {
            spans.push(Span::styled("↑↓", key_style));
            spans.push(Span::styled(":Navigate ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Enter", key_style));
            spans.push(Span::styled(":Drill ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Bksp", key_style));
            spans.push(Span::styled(":Up ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Esc", key_style));
            spans.push(Span::styled(":Exit", text_style));
        }
        AppMode::Normal => {
            // Function keys
            spans.push(Span::styled("F5", key_style));
            spans.push(Span::styled(":Cp ", text_style));
            spans.push(Span::styled("│", sep_style));
            
            spans.push(Span::styled(" F6", key_style));
            spans.push(Span::styled(":Mv ", text_style));
            spans.push(Span::styled("│", sep_style));
            
            spans.push(Span::styled(" F7", key_style));
            spans.push(Span::styled(":Mk ", text_style));
            spans.push(Span::styled("│", sep_style));
            
            spans.push(Span::styled(" F8", key_style));
            spans.push(Span::styled(":Del ", text_style));
            spans.push(Span::styled("│", sep_style));
            
            // Storage
            spans.push(Span::styled(" ^N", key_style));
            spans.push(Span::styled(":Src ", text_style));
            spans.push(Span::styled("│", sep_style));
            
            // Sync
            if app.sync_enabled {
                spans.push(Span::styled(" ^Y", key_style));
                spans.push(Span::styled(":Sync ", text_style));
                spans.push(Span::styled("│", sep_style));
            } else {
                spans.push(Span::styled(" ^S", key_style));
                spans.push(Span::styled(":Sync ", text_style));
                spans.push(Span::styled("│", sep_style));
            }
            
            // Quit
            spans.push(Span::styled(" q", key_style));
            spans.push(Span::styled(":Quit", text_style));
        }
    }
    
    spans
}

/// Render the status bar with message and sync status.
pub fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let sync_indicator = match &app.sync_status {
        SyncStatus::Disabled => String::new(),
        SyncStatus::Idle => " │ 🔄 Sync: Idle".to_string(),
        SyncStatus::Scanning => " │ 🔄 Scanning...".to_string(),
        SyncStatus::Syncing { current_file, progress } => {
            format!(" │ 🔄 {:.0}% {}", progress * 100.0, truncate_path(current_file, 20))
        }
        SyncStatus::Complete { files_synced } => format!(" │ ✅ Synced {} files", files_synced),
        SyncStatus::Error { message } => format!(" │ ❌ {}", truncate_path(message, 30)),
    };
    
    let text = format!("{}{}", app.message, sync_indicator);
    
    let status = Paragraph::new(text)
        .style(Style::default().bg(Color::Blue).fg(Color::White));
    
    f.render_widget(status, area);
}

/// Render progress bar for file operations.
pub fn render_progress_bar(f: &mut Frame, area: Rect, progress: &Progress) {
    let label = match progress.stage {
        ProgressStage::Counting => format!("Scanning: {}", progress.current_file),
        ProgressStage::Archiving => {
            format!(
                "Archiving: {} ({}/{})",
                truncate_path(&progress.current_file, 30),
                progress.files_done,
                progress.total_files
            )
        }
        ProgressStage::Transferring => {
            if progress.total > 0 {
                format!(
                    "Copying: {} ({}/{})",
                    truncate_path(&progress.current_file, 30),
                    progress.files_done,
                    progress.total_files
                )
            } else {
                format!("Copying: {}", truncate_path(&progress.current_file, 40))
            }
        }
        ProgressStage::Extracting => format!("Extracting: {}", progress.current_file),
        ProgressStage::Complete => "Complete!".to_string(),
    };
    
    let ratio = if progress.total > 0 {
        (progress.current as f64 / progress.total as f64).min(1.0)
    } else {
        0.0
    };
    
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Progress "))
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Black))
        .ratio(ratio)
        .label(label);
    
    f.render_widget(gauge, area);
}

/// Render a centered popup dialog.
pub fn render_popup(f: &mut Frame, title: &str, lines: Vec<Line>, style: Style) {
    let area = f.area();
    
    // Calculate popup dimensions
    let max_line_width = lines.iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(20) as u16;
    
    let popup_width = (max_line_width + 4).max(40).min(area.width - 4);
    let popup_height = (lines.len() as u16 + 4).min(area.height - 2);
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    
    // Clear area and render
    f.render_widget(Clear, popup_area);
    
    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(title)
                .title_style(style.add_modifier(Modifier::BOLD)),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    
    f.render_widget(popup, popup_area);
}

/// Render delete confirmation popup.
pub fn render_delete_confirm(f: &mut Frame, target: &crate::app::DeleteTarget) {
    let type_str = if target.is_dir { "directory" } else { "file" };
    let location = if matches!(
        target.backend.backend_type(),
        crate::fs::BackendType::Local
    ) {
        "LOCAL"
    } else {
        "REMOTE"
    };
    
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Delete ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(type_str),
            Span::raw(" ("),
            Span::styled(location, Style::default().fg(Color::Yellow)),
            Span::raw("):"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                target.display_path.clone(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[Y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" Yes  "),
            Span::styled("[N/Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" No"),
        ]),
    ];
    
    render_popup(f, " ⚠ Confirm Delete ", lines, Style::default().fg(Color::Red));
}

/// Truncate a path for display.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}
