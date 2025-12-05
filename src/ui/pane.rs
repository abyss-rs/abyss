use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::fs::types::FileEntry;

pub struct Pane {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub state: ListState,
    pub is_active: bool,
}

impl Pane {
    pub fn new(path: String) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        Self {
            path,
            entries: Vec::new(),
            state,
            is_active: false,
        }
    }

    pub fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.entries.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.entries.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.state.selected().and_then(|i| self.entries.get(i))
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                let icon = if entry.is_dir { "📁" } else { "📄" };
                let size = entry.format_size();

                let content = format!("{} {:<40} {:>10}", icon, entry.name, size);

                ListItem::new(Line::from(content))
            })
            .collect();

        let border_style = if self.is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        let title = if self.path.is_empty() {
            "Select PVC".to_string()
        } else {
            self.path.clone()
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");

        f.render_stateful_widget(list, area, &mut self.state);
    }
}

pub fn render_status_bar(f: &mut Frame, area: Rect, message: &str) {
    let status =
        Paragraph::new(message).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    f.render_widget(status, area);
}

pub fn render_help_bar(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Span::styled("F2", Style::default().fg(Color::Yellow)),
        Span::raw(" Info | "),
        Span::styled("F3", Style::default().fg(Color::Yellow)),
        Span::raw(" Analyze | "),
        Span::styled("F5", Style::default().fg(Color::Yellow)),
        Span::raw(" Copy | "),
        Span::styled("F7", Style::default().fg(Color::Yellow)),
        Span::raw(" MkDir | "),
        Span::styled("F8", Style::default().fg(Color::Yellow)),
        Span::raw(" Delete | "),
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" Switch | "),
        Span::styled("F10", Style::default().fg(Color::Yellow)),
        Span::raw(" Quit"),
    ];

    let help = Paragraph::new(Line::from(help_text))
        .style(Style::default().bg(Color::Blue).fg(Color::White));

    f.render_widget(help, area);
}
