use ratatui::style::{Color, Modifier, Style};

pub(crate) const CORE: Style = Style::new().fg(Color::Gray).bg(Color::Blue);
pub(crate) const HEADER: Style = Style::new().fg(Color::Yellow).bg(Color::Blue);
pub(crate) const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);
pub(crate) const MARKED: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Yellow)
    .add_modifier(Modifier::BOLD);
pub(crate) const MARK_SELECTED: Style = Style::new()
    .fg(Color::White)
    .bg(Color::Magenta)
    .add_modifier(Modifier::BOLD);
pub(crate) const DIALOG: Style = Style::new().fg(Color::Black).bg(Color::Gray);
pub(crate) const FOCUSED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);
pub(crate) const ERROR: Style = Style::new().fg(Color::White).bg(Color::Red);

pub(crate) const OPERATION_DIALOG_WIDTH: u16 = 76;
pub(crate) const OPERATION_DIALOG_HEIGHT: u16 = 12;
pub(crate) const BACKGROUND_ETA_BREAKPOINT: usize = 72;
pub(crate) const BACKGROUND_ID_WIDTH: usize = 6;
pub(crate) const BACKGROUND_KIND_WIDTH: usize = 8;
pub(crate) const BACKGROUND_STATE_WIDTH: usize = 10;
pub(crate) const BACKGROUND_PERCENT_WIDTH: usize = 7;
pub(crate) const BACKGROUND_SPEED_WIDTH: usize = 13;
pub(crate) const BACKGROUND_ETA_WIDTH: usize = 14;
