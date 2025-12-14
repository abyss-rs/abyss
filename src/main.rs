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
    Terminal,
};
use std::io;

use app::App;
use events::handle_events;
use ui::{render_delete_confirm, render_help_bar, render_progress_bar, render_status_bar, Pane};

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

            // Only render panes if NOT in editor mode - otherwise editor covers this area
            if !matches!(app.mode, app::AppMode::EditFile | app::AppMode::EditorSearch) {
                app.left_pane.render(f, panes[0]);
                app.right_pane.render(f, panes[1]);
            }

            // Render delete confirmation popup if in ConfirmDelete mode
            if matches!(app.mode, app::AppMode::ConfirmDelete) {
                if let Some(ref target) = app.delete_target {
                    render_delete_confirm(f, target);
                }
            }
            
            // Render rename popup
            if matches!(app.mode, app::AppMode::Rename) {
                ui::components::render_rename_popup(f, &app.text_input);
            }
            
            // Render file viewer
            if matches!(app.mode, app::AppMode::ViewFile) {
                let filename = app.active_pane().selected_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                // Use main content area (chunks[0])
                ui::components::render_file_viewer(f, &app.view_content, app.view_scroll, &filename, chunks[0]);
            }
            
            // Render search popup
            if matches!(app.mode, app::AppMode::Search) {
                ui::components::render_search_popup(f, &app.text_input, " Search ");
            }
            
            // Render editor search popup
            if matches!(app.mode, app::AppMode::EditorSearch) {
                // Ensure editor is rendered first (below) or render it here?
                // Logic below renders editor for EditFile. Let's include EditorSearch there.
                // Or duplicate render_file_editor call here.
                // Best structure:
            }
            
            // Render file editor
            if matches!(app.mode, app::AppMode::EditFile | app::AppMode::EditorSearch) {
                // Use main content area (chunks[0]) to avoid overlapping status/help bar
                ui::components::render_file_editor(f, &mut app.editor, chunks[0]);
            }

            if matches!(app.mode, app::AppMode::EditorSearch) {
                ui::components::render_search_popup(f, &app.text_input, " Where Is ");
            }
            
            // Render large file confirmation
            if matches!(app.mode, app::AppMode::ConfirmLargeLoad) {
                ui::components::render_confirm_large_load_popup(f, app);
            }

            if show_progress {
                if let Some(ref progress) = app.progress {
                    render_progress_bar(f, chunks[1], progress);
                }
                render_status_bar(f, chunks[2], app);
                render_help_bar(f, chunks[3], app);
            } else {
                render_status_bar(f, chunks[1], app);
                render_help_bar(f, chunks[2], app);
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
