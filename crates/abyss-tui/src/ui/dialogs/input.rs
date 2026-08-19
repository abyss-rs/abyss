use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::InputDialog;
use crate::ui::dialogs::render_dialog_buttons;
use crate::ui::helpers::centered;
use crate::ui::layout::{DialogButton, LayoutInfo};
use crate::ui::theme::DIALOG;

pub(crate) fn render_input_dialog(
    frame: &mut Frame,
    outer: Rect,
    input: &InputDialog,
    text: &str,
    layout: &mut LayoutInfo,
) {
    let area = centered(outer, 78, 10);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", input.title))
            .style(DIALOG),
        area,
    );
    frame.render_widget(
        Paragraph::new(format!("{}\n\n{}", input.prompt, text))
            .style(DIALOG)
            .wrap(Wrap { trim: false }),
        Rect::new(
            area.x + 2,
            area.y + 1,
            area.width.saturating_sub(4),
            area.height.saturating_sub(4),
        ),
    );
    let buttons = if input.supports_background() {
        vec![
            (DialogButton::Start, "[ Enter  Start ]"),
            (DialogButton::Background, "[ Ctrl+B  Background ]"),
            (DialogButton::Cancel, "[ Esc  Cancel ]"),
        ]
    } else {
        vec![
            (DialogButton::Start, "[ Enter  Confirm ]"),
            (DialogButton::Cancel, "[ Esc  Cancel ]"),
        ]
    };
    render_dialog_buttons(frame, area, &buttons, layout);
}
