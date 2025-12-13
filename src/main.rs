mod app;
mod events;
mod fs;
mod k8s;
mod sync;
mod ui;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Terminal,
};
use std::io;

use app::App;
use events::handle_events;
use ui::{render_help_bar, render_status_bar};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new().await?;

    // Main loop
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            // Determine if we need progress bar
            let show_progress = app.progress.is_some();

            let chunks = if show_progress {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),    // Main area
                        Constraint::Length(3), // Progress bar
                        Constraint::Length(1), // Status bar
                        Constraint::Length(1), // Help bar
                    ])
                    .split(f.area())
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),    // Main area
                        Constraint::Length(1), // Status bar
                        Constraint::Length(1), // Help bar
                    ])
                    .split(f.area())
            };

            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);

            app.left_pane.render(f, panes[0]);
            app.right_pane.render(f, panes[1]);

            // Render delete confirmation popup if in ConfirmDelete mode
            if matches!(app.mode, app::AppMode::ConfirmDelete) {
                if let Some(ref target) = app.delete_target {
                    // Calculate popup area (centered, 60% width, 7 lines height)
                    let area = f.area();
                    let popup_width = (area.width * 60 / 100).max(40).min(area.width - 4);
                    let popup_height = 7u16;
                    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
                    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
                    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

                    // Clear the area behind the popup
                    f.render_widget(Clear, popup_area);

                    // Create popup content
                    let type_str = if target.is_dir { "directory" } else { "file" };
                    let location = if matches!(
                        target.backend.backend_type(),
                        crate::fs::BackendType::Local
                    ) {
                        "LOCAL"
                    } else {
                        "REMOTE"
                    };

                    let text = vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                "Delete ",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(type_str),
                            Span::raw(" ("),
                            Span::styled(location, Style::default().fg(Color::Yellow)),
                            Span::raw("):"),
                        ]),
                        Line::from(""),
                        Line::from(vec![Span::styled(
                            &target.display_path,
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )]),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                "[Y]",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(" Yes  "),
                            Span::styled(
                                "[N/Esc]",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(" No"),
                        ]),
                    ];

                    let popup = Paragraph::new(text)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(Color::Red))
                                .title(" ⚠ Confirm Delete ")
                                .title_style(
                                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                                ),
                        )
                        .alignment(ratatui::layout::Alignment::Center)
                        .wrap(Wrap { trim: true });

                    f.render_widget(popup, popup_area);
                }
            }

            if show_progress {
                if let Some(ref progress) = app.progress {
                    use app::ProgressStage;

                    // Calculate percent based on stage
                    let (percent, label, color) = match progress.stage {
                        ProgressStage::Counting => {
                            // Indeterminate - show spinner-like animated bar
                            let tick = (std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                                / 200)
                                % 100;
                            (
                                tick as u16,
                                format!("📊 Counting files: {}...", progress.current_file),
                                Color::Yellow,
                            )
                        }
                        ProgressStage::Archiving => {
                            let pct = if progress.total_files > 0 {
                                (progress.files_done as f64 / progress.total_files as f64 * 100.0)
                                    as u16
                            } else {
                                50
                            };
                            (
                                pct,
                                format!(
                                    "📦 Archiving: {} ({}/{})",
                                    progress.current_file,
                                    progress.files_done,
                                    progress.total_files
                                ),
                                Color::Blue,
                            )
                        }
                        ProgressStage::Transferring => {
                            let pct = if progress.total > 0 {
                                (progress.current as f64 / progress.total as f64 * 100.0) as u16
                            } else {
                                // Indeterminate
                                let tick = (std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    / 200)
                                    % 100;
                                tick as u16
                            };
                            (
                                pct,
                                format!("📤 Transferring: {}", progress.current_file),
                                Color::Cyan,
                            )
                        }
                        ProgressStage::Extracting => (
                            90,
                            format!("📂 Extracting: {}", progress.current_file),
                            Color::Magenta,
                        ),
                        ProgressStage::Complete => (
                            100,
                            format!("✓ Complete: {}", progress.current_file),
                            Color::Green,
                        ),
                    };

                    let gauge = Gauge::default()
                        .block(Block::default().borders(Borders::ALL).title("Progress"))
                        .gauge_style(Style::default().fg(color).bg(Color::Black))
                        .percent(percent.min(100))
                        .label(label);

                    f.render_widget(gauge, chunks[1]);
                }
                render_status_bar(f, chunks[2], &app.message);
                render_help_bar(f, chunks[3]);
            } else {
                render_status_bar(f, chunks[1], &app.message);
                render_help_bar(f, chunks[2]);
            }
        })?;

        // Poll background tasks for progress updates
        app.poll_background_task().await;

        handle_events(app).await?;

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
