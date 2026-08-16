mod analyze;
mod console;
mod dialogs;
mod diff;
pub(crate) mod helpers;
mod jobs;
mod layout;
pub(crate) mod menu;
mod monitor;
pub(crate) mod panes;
pub(crate) mod status;
mod sync;
mod theme;

#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph, Wrap};

pub(crate) use self::dialogs::render_box;
pub(crate) use self::layout::{ActionButton, DialogButton, LayoutInfo};
pub(crate) use self::theme::*;
use crate::app::App;

pub(crate) fn render(frame: &mut Frame, app: &App) -> LayoutInfo {
    let area = frame.area();
    frame.render_widget(Block::default().style(CORE), area);

    if area.width < 52 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Terminal is too small (minimum 52 × 10)")
                .style(ERROR)
                .wrap(Wrap { trim: true }),
            area,
        );
        return LayoutInfo::default();
    }

    if app.analyze.is_some() {
        return analyze::render_analyze(frame, app, area);
    }

    if app.sync.is_some() {
        return sync::render_sync(frame, app, area);
    }

    if let Some(monitor) = &app.monitor {
        monitor::render_monitor(frame, area, monitor);
        return LayoutInfo::default();
    }

    if let Some(view) = &app.diff {
        diff::render_diff(frame, area, view);
        return LayoutInfo::default();
    }

    if let Some(viewer) = &app.viewer {
        jobs::render_viewer(frame, area, viewer, app.viewer_highlight.as_ref());
        return LayoutInfo::default();
    }

    let background_jobs = app.jobs.visible_background();
    let job_rows = background_jobs.len() as u16;
    let button_area = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let status_area = Rect::new(area.x, area.bottom() - 2, area.width, 1);
    let jobs_area = Rect::new(
        area.x,
        status_area.y.saturating_sub(job_rows),
        area.width,
        job_rows,
    );
    let top_area = Rect::new(area.x, area.y, area.width, 1);
    let body_area = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        jobs_area.y.saturating_sub(area.y + 1),
    );
    // The console eats into the panes from the bottom; in the full view it
    // takes the lot and the panes are not drawn at all.
    let console_rows = app.console_view.rows(body_area.height);
    let panes_area = Rect::new(
        body_area.x,
        body_area.y,
        body_area.width,
        body_area.height.saturating_sub(console_rows),
    );
    let console_area = Rect::new(
        body_area.x,
        panes_area.bottom(),
        body_area.width,
        console_rows,
    );
    let left_width = panes_area.width / 2;
    let pane_rects = [
        Rect::new(panes_area.x, panes_area.y, left_width, panes_area.height),
        Rect::new(
            panes_area.x + left_width,
            panes_area.y,
            panes_area.width - left_width,
            panes_area.height,
        ),
    ];
    let pane_rows = panes_area.height.saturating_sub(3) as usize;
    let mut layout = LayoutInfo {
        pane_rects,
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
        pane_rows,
    };

    menu::render_top_bar(frame, app, top_area, &mut layout);
    if panes_area.height > 0 {
        for (pane, &pane_rect) in pane_rects.iter().enumerate() {
            panes::render_pane(frame, app, pane, pane_rect, &mut layout);
        }
    }
    console::render_console(frame, app, console_area, &mut layout);
    jobs::render_background_jobs(frame, jobs_area, &background_jobs, &mut layout);
    status::render_status(frame, app, status_area);
    status::render_buttons(frame, app, button_area, &mut layout);

    if let Some(path) = &app.viewer_loading {
        render_box(
            frame,
            area,
            "Viewer",
            &format!("Reading {}…\n\nEsc  Cancel", path.display()),
            false,
            66,
            7,
        );
    } else if let Some(id) = app.foreground_job
        && let Some(job) = app.jobs.job(id)
    {
        jobs::render_operation(frame, area, job);
    }

    if let Some(modal) = &app.modal {
        dialogs::render_modal(frame, area, modal, &mut layout);
    } else if app.jobs_panel.is_some() {
        jobs::render_jobs_panel(frame, area, app, &mut layout);
    }
    if app.modal.is_none() && app.foreground_job.is_none() && app.jobs_panel.is_none() {
        menu::render_app_menu(frame, app, &mut layout);
        menu::render_sort_menu(frame, app, &mut layout);
    }

    layout
}
