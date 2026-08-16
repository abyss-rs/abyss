use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;
use crate::ui::dialogs::render_modal;
use crate::ui::helpers::fit;
use crate::ui::layout::LayoutInfo;
use crate::ui::status::render_buttons;
use crate::ui::theme::{FOCUSED, HEADER};

pub(crate) fn render_analyze(frame: &mut Frame, app: &App, area: Rect) -> LayoutInfo {
    let button_area = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    // Status row only when cleaner has something to say (confirm/progress/message).
    // Idle key captions duplicated the button bar and are omitted.
    let status_text = app
        .analyze
        .as_ref()
        .and_then(|session| session.status_line());
    let status_area = status_text
        .as_ref()
        .map(|_| Rect::new(area.x, area.bottom() - 2, area.width, 1));
    let top_area = Rect::new(area.x, area.y, area.width, 1);
    let content_bottom = status_area.map(|r| r.y).unwrap_or(button_area.y);
    let content = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        content_bottom.saturating_sub(area.y + 1),
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

    frame.render_widget(Block::default().style(HEADER), top_area);
    frame.render_widget(
        Paragraph::new(" Analyze ").style(FOCUSED.add_modifier(Modifier::BOLD)),
        top_area,
    );

    if let Some(session) = &app.analyze {
        session.draw(frame, content, cleaner_tui::Chrome::ContentOnly);
    }

    if let (Some(status_area), Some(status_text)) = (status_area, status_text) {
        let status_style = if status_text.contains("(y/n)") {
            Style::new().fg(Color::Yellow).bg(Color::Blue)
        } else {
            FOCUSED
        };
        frame.render_widget(
            Paragraph::new(format!(
                " {}",
                fit(&status_text, area.width.saturating_sub(1) as usize)
            ))
            .style(status_style),
            status_area,
        );
    }

    render_buttons(frame, app, button_area, &mut layout);

    if let Some(modal) = &app.modal {
        render_modal(frame, area, modal, &mut layout);
    }

    layout
}
