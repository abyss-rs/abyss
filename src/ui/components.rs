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
        AppMode::ConfirmDelete | AppMode::ConfirmLargeLoad => {
            spans.push(Span::styled("Y", key_style));
            spans.push(Span::styled(":Confirm ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" N/Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
        AppMode::EditorSearch => {
             spans.push(Span::styled("Enter", key_style));
             spans.push(Span::styled(":Find ", text_style));
             spans.push(Span::styled("│", sep_style));
             spans.push(Span::styled(" Esc", key_style));
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
            // Function keys sorted
            spans.push(Span::styled("F3", key_style));
            spans.push(Span::styled(":View ", text_style));
            spans.push(Span::styled("│", sep_style));

            spans.push(Span::styled(" F4", key_style));
            spans.push(Span::styled(":Edit ", text_style));
            spans.push(Span::styled("│", sep_style));

            spans.push(Span::styled(" F5", key_style));
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

            spans.push(Span::styled(" F9", key_style));
            spans.push(Span::styled(":Analyz ", text_style));
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
        AppMode::Rename => {
            spans.push(Span::styled("Type", key_style));
            spans.push(Span::styled(":New name ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Enter", key_style));
            spans.push(Span::styled(":Confirm ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
        AppMode::ViewFile => {
            spans.push(Span::styled("j/k", key_style));
            spans.push(Span::styled(":Scroll ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" PgUp/PgDn", key_style));
            spans.push(Span::styled(":Page ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Home/End", key_style));
            spans.push(Span::styled(":Top/Bot ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" q/Esc", key_style));
            spans.push(Span::styled(":Close", text_style));
        }
        AppMode::EditFile => {
            spans.push(Span::styled("^O", key_style));
            spans.push(Span::styled(":Write ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" ^K", key_style));
            spans.push(Span::styled(":Cut ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" ^U", key_style));
            spans.push(Span::styled(":Uncut ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" ^X", key_style));
            spans.push(Span::styled(":Exit", text_style));
        }
        AppMode::Search => {
            spans.push(Span::styled("Type", key_style));
            spans.push(Span::styled(":Pattern ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Enter", key_style));
            spans.push(Span::styled(":Find ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
        AppMode::HashMenu => {
            spans.push(Span::styled("↑↓", key_style));
            spans.push(Span::styled(":Navigate ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Enter", key_style));
            spans.push(Span::styled(":Select ", text_style));
            spans.push(Span::styled("│", sep_style));
            spans.push(Span::styled(" Esc", key_style));
            spans.push(Span::styled(":Cancel", text_style));
        }
    }
    
    spans
}

/// Render file editor.
pub fn render_file_editor(f: &mut Frame, editor: &mut crate::app::TextEditor, area: Rect) {
    // Clear the entire area first
    f.render_widget(Clear, area);
    
    // Editor styling - different colors for edit vs readonly mode
    let bg_color = Color::Black;
    let border_style = if editor.modified {
         Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if editor.readonly {
         Style::default().fg(Color::Cyan)  // Readonly mode: cyan border
    } else {
         Style::default().fg(Color::Green)  // Edit mode: green border
    };
    
    let title = if editor.modified {
        format!(" Editing: {} (Modified) ", editor.filename)
    } else if editor.readonly {
        format!(" Viewing: {} (readonly) ", editor.filename)
    } else {
        format!(" Editing: {} ", editor.filename)
    };

    // Get file extension for syntax highlighting
    let extension = std::path::Path::new(&editor.filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt");

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .style(Style::default().bg(bg_color));
    
    let inner_area = block.inner(area);
    
    // Render the block (border) first
    f.render_widget(block, area);
    
    // Calculate visible lines and store for event handler scroll logic
    let visible_height = inner_area.height as usize;
    editor.visible_height = visible_height;
    
    let start_line = editor.scroll_offset;
    let end_line = (start_line + visible_height).min(editor.content.len());

    // Render each line individually to its own row
    for (i, line_idx) in (start_line..end_line).enumerate() {
        if i >= visible_height {
            break;
        }
        
        let line_content = &editor.content[line_idx];
        // Replace tabs with spaces to avoid width calculation issues
        let clean_content = line_content.replace('\t', "    ");
        
        let highlighted = crate::ui::syntax::highlight_line(&clean_content, extension);
        
        let line_area = Rect::new(
            inner_area.x,
            inner_area.y + i as u16,
            inner_area.width,
            1,
        );
        
        // Render this line
        f.render_widget(
            Paragraph::new(highlighted)
                .style(Style::default().bg(bg_color)),
            line_area,
        );
    }
    
    // Set cursor
    let cursor_y = editor.cursor_row as i32 - editor.scroll_offset as i32;
    if cursor_y >= 0 && cursor_y < inner_area.height as i32 {
        // Also account for tab expansion in cursor position
        let current_line = &editor.content[editor.cursor_row];
        let chars_before_cursor: String = current_line.chars().take(editor.cursor_col).collect();
        let visual_col = chars_before_cursor.replace('\t', "    ").len();
        
        f.set_cursor_position(
            (inner_area.x + visual_col as u16,
            inner_area.y + cursor_y as u16)
        );
    }
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
    
    // Use get_status_message which includes full filename for truncated entries
    let status_msg = app.get_status_message();
    let text = format!("{}{}", status_msg, sync_indicator);
    
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

/// Render rename input popup.
pub fn render_rename_popup(f: &mut Frame, text_input: &crate::app::TextInput) {
    let area = f.area();
    
    let popup_width = 50u16.min(area.width - 4);
    let popup_height = 5;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    
    f.render_widget(Clear, popup_area);
    
    // Show input with cursor
    let input_display = format!(
        "{}|{}",
        &text_input.value[..text_input.cursor],
        &text_input.value[text_input.cursor..]
    );
    
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(input_display, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(": Confirm  "),
            Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(": Cancel"),
        ]),
    ];
    
    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Rename ")
                .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .alignment(Alignment::Center);
    
    f.render_widget(popup, popup_area);
}

/// Render file viewer overlay.
pub fn render_file_viewer(f: &mut Frame, content: &[String], scroll: usize, filename: &str, area: Rect) {
    // Use most of the screen (passed area)
    let margin = 2;
    let popup_area = Rect::new(
        area.x + margin,
        area.y + margin,
        area.width.saturating_sub(margin * 2),
        area.height.saturating_sub(margin * 2),
    );
    
    f.render_widget(Clear, popup_area);
    
    // Get visible lines
    let visible_height = popup_area.height.saturating_sub(2) as usize;
    // Get file extension
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt");

    let visible_lines: Vec<Line> = content
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, line)| {
            let line_num = scroll + i + 1;
            let mut spans = vec![
                Span::styled(
                    format!("{:4} ", line_num),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            
            // Highlight the code content
            let highlighted = crate::ui::syntax::highlight_line(line, extension);
            spans.extend(highlighted.spans);
            
            Line::from(spans)
        })
        .collect();
    
    let title = format!(" {} (line {}/{}) ", filename, scroll + 1, content.len());
    
    let popup = Paragraph::new(visible_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(title)
                .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        )
        .wrap(Wrap { trim: false });
    
    f.render_widget(popup, popup_area);
}

/// Render search input popup.
pub fn render_search_popup(f: &mut Frame, text_input: &crate::app::TextInput, title: &str) {
    let area = f.area();
    
    let popup_width = 50u16.min(area.width - 4);
    let popup_height = 5;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    
    f.render_widget(Clear, popup_area);
    
    // Show input with cursor
    let input_display = format!(
        "{}|{}",
        &text_input.value[..text_input.cursor],
        &text_input.value[text_input.cursor..]
    );
    
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Find: "),
            Span::styled(input_display, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(": Search  "),
            Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(": Cancel"),
        ]),
    ];
    
    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(title)
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        )
        .alignment(Alignment::Center);
    
    f.render_widget(popup, popup_area);
}

/// Render large file confirmation popup.
pub fn render_confirm_large_load_popup(f: &mut Frame, app: &crate::app::App) {
    let area = f.area();
    
    // Check if View or Edit
    let action_str = if matches!(app.pending_large_action, Some(crate::app::LargeFileAction::Edit)) { "Edit" } else { "View" };
    let size_mb = app.view_file_size / 1024 / 1024;
    
    let blocks = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("Remote file is large ({} MB)!", size_mb), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(format!("Download and {}? This may take time.", action_str)),
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter/Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(": Confirm  "),
            Span::styled("Esc/N", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(": Cancel"),
        ]),
    ];
    
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 7;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    
    if popup_x >= area.width || popup_y >= area.height {
        return;
    }
    
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    
    f.render_widget(Clear, popup_area);
    
    let popup = Paragraph::new(blocks)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title(" ⚠ Large File Warning ")
                .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        )
        .alignment(Alignment::Center);

    f.render_widget(popup, popup_area);
}
