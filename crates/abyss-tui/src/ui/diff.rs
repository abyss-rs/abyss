use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::state::DiffView;
use crate::diff::DiffTag;
use crate::ui::helpers::fit;
use crate::ui::theme::{CORE, HEADER};

/// Fullscreen unified diff, styled the way a diff tool colours one.
pub(crate) fn render_diff(frame: &mut Frame, area: Rect, view: &DiffView) {
    let title = format!(
        " {} ↔ {} [+{} −{}] ",
        view.left_name, view.right_name, view.diff.stats.inserted, view.diff.stats.deleted
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(title, HEADER))
            .border_style(Style::new().fg(Color::White).bg(Color::Blue))
            .style(CORE),
        area,
    );

    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let width = inner.width as usize;

    let lines: Vec<_> = view
        .diff
        .lines
        .iter()
        .skip(view.vertical)
        .take(inner.height as usize)
        .map(|line| {
            let marker = match line.tag {
                DiffTag::Insert => '+',
                DiffTag::Delete => '-',
                DiffTag::Context => ' ',
                DiffTag::Separator => '~',
            };
            // Line numbers make a diff navigable; blank where the line does
            // not exist on that side.
            let number = match line.tag {
                DiffTag::Separator => "    ".to_owned(),
                DiffTag::Insert => format!("{:>4}", line.right.unwrap_or(0)),
                _ => format!("{:>4}", line.left.unwrap_or(0)),
            };
            let text: String = line.text.chars().skip(view.horizontal).collect();
            let rendered = fit(&format!("{number} {marker} {text}"), width);
            let style = match line.tag {
                DiffTag::Insert => Style::new().fg(Color::Black).bg(Color::Green),
                DiffTag::Delete => Style::new().fg(Color::Black).bg(Color::Red),
                DiffTag::Separator => Style::new().fg(Color::DarkGray),
                DiffTag::Context => CORE,
            };
            ratatui::text::Line::from(Span::styled(rendered, style))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines).style(CORE), inner);

    let help = " ↑↓ PgUp/PgDn ←→ scroll   Esc close ";
    frame.render_widget(
        Paragraph::new(fit(help, area.width as usize)).style(HEADER),
        Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
    );
}
