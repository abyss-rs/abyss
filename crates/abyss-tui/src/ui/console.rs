use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::app::App;
use crate::ui::helpers::fit;
use crate::ui::layout::LayoutInfo;
use crate::ui::theme::{CORE, FOCUSED};

/// Draw the shell, returning the interior the emulator should be sized to.
///
/// `area` includes the border row.
pub(crate) fn render_console(frame: &mut Frame, app: &App, area: Rect, layout: &mut LayoutInfo) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(console) = &app.console else {
        return;
    };
    let focused = console.focused;
    let title = title_text(area.width as usize, focused);
    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .style(if focused { FOCUSED } else { CORE });
    let interior = block.inner(area);
    layout.console = interior;

    let cursor = if focused {
        Cursor::default()
    } else {
        let mut cursor = Cursor::default();
        cursor.hide();
        cursor
    };
    frame.render_widget(
        PseudoTerminal::new(console.screen())
            .block(block)
            .cursor(cursor),
        area,
    );
}

fn title_text(width: usize, focused: bool) -> String {
    let hint = if focused {
        "Esc pane  ^X size"
    } else {
        "c focus  ^X size"
    };
    let label = format!(" Console — {hint} ");
    fit(&label, width)
}
