use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::menu::{BookmarkFocus, MenuCategory};
use crate::app::state::App;
use crate::browser::SortMode;
use crate::ui::helpers::{display_width, fit, fit_filename, pad_right};
use crate::ui::layout::LayoutInfo;
use crate::ui::theme::{DIALOG, FOCUSED};

pub(crate) fn render_top_bar(frame: &mut Frame, app: &App, area: Rect, layout: &mut LayoutInfo) {
    let mut x = area.x;
    for category in MenuCategory::ALL {
        let width = (display_width(category.label()) + 2) as u16;
        let rect = Rect::new(x, area.y, width, 1);
        if rect.width == 0 {
            break;
        }
        let selected = app
            .app_menu
            .as_ref()
            .is_some_and(|menu| menu.category == category);
        frame.render_widget(
            Paragraph::new(format!(" {} ", category.label())).style(if selected {
                FOCUSED
            } else {
                DIALOG
            }),
            rect,
        );
        layout.menu_headings.push((category, rect));
        x = x.saturating_add(width);
    }
    let sort_label = format!(" Sort: {} ", app.panes[app.active].sort.mode.label());
    let sort_width = (display_width(&sort_label)) as u16;
    let sort_x = area.right().saturating_sub(sort_width);
    let sort_rect = Rect::new(sort_x, area.y, sort_width, 1);
    frame.render_widget(
        Paragraph::new(sort_label).style(if app.sort_menu.is_some() {
            FOCUSED
        } else {
            DIALOG
        }),
        sort_rect,
    );
    layout.sort_menu = sort_rect;
}

pub(crate) fn render_app_menu(frame: &mut Frame, app: &App, layout: &mut LayoutInfo) {
    let Some(menu) = app.app_menu else {
        return;
    };
    let Some(anchor) = layout
        .menu_headings
        .iter()
        .find(|(category, _)| *category == menu.category)
        .map(|(_, rect)| *rect)
    else {
        return;
    };
    if menu.category == MenuCategory::Bookmarks {
        render_bookmark_menu(frame, app, anchor, layout);
        return;
    }

    let actions = app.visible_menu_actions(menu.category);
    let content_width = actions
        .iter()
        .map(|action| display_width(action.label()) + display_width(action.shortcut()) + 7)
        .max()
        .unwrap_or(24)
        .max(display_width(menu.category.label()) + 4);
    let width = (content_width as u16).min(frame.area().width).max(1);
    let available_height = frame.area().bottom().saturating_sub(anchor.y + 1).max(1);
    let wanted_rows = actions.len().max(1) as u16;
    let height = wanted_rows.saturating_add(2).min(available_height);
    let x = anchor.x.min(frame.area().right().saturating_sub(width));
    let area = Rect::new(x, anchor.y + 1, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", menu.category.label()))
            .style(DIALOG),
        area,
    );

    let visible_rows = area.height.saturating_sub(2) as usize;
    if actions.is_empty() {
        let rect = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(" No actions available ")
                .style(Style::new().fg(Color::DarkGray).bg(Color::Gray)),
            rect,
        );
        return;
    }
    let start = menu
        .selected
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(actions.len().saturating_sub(visible_rows));
    for (row, action) in actions.iter().skip(start).take(visible_rows).enumerate() {
        let index = start + row;
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + row as u16,
            area.width.saturating_sub(2),
            1,
        );
        let marker = if app.menu_action_checked(*action) {
            "✓"
        } else {
            " "
        };
        let shortcut_width = display_width(action.shortcut());
        let label_width = rect.width.saturating_sub(shortcut_width as u16 + 4) as usize;
        let text = format!(
            " {marker} {} {} ",
            pad_right(&fit(action.label(), label_width), label_width),
            action.shortcut()
        );
        frame.render_widget(
            Paragraph::new(text).style(if menu.selected == index {
                FOCUSED
            } else {
                DIALOG
            }),
            rect,
        );
        layout.menu_items.push((*action, rect));
    }
}

pub(crate) fn render_bookmark_menu(
    frame: &mut Frame,
    app: &App,
    anchor: Rect,
    layout: &mut LayoutInfo,
) {
    let Some(menu) = app.app_menu else {
        return;
    };
    let width = 64_u16.min(frame.area().width).max(1);
    let available_height = frame.area().bottom().saturating_sub(anchor.y + 1).max(1);
    let height = 11_u16.min(available_height);
    let x = anchor.x.min(frame.area().right().saturating_sub(width));
    let area = Rect::new(x, anchor.y + 1, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Bookmarks · ↵ Jump · S Set ")
            .style(DIALOG),
        area,
    );

    let visible_rows = area.height.saturating_sub(2) as usize;
    let start = menu
        .selected
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(9_usize.saturating_sub(visible_rows));
    for (row, index) in (start..9).take(visible_rows).enumerate() {
        let row_rect = Rect::new(
            area.x + 1,
            area.y + 1 + row as u16,
            area.width.saturating_sub(2),
            1,
        );
        let set_width = 7_u16.min(row_rect.width);
        let jump_rect = Rect::new(
            row_rect.x,
            row_rect.y,
            row_rect.width.saturating_sub(set_width),
            1,
        );
        let set_rect = Rect::new(jump_rect.right(), row_rect.y, set_width, 1);
        let bookmark = app.bookmark_display(index);
        let selected = menu.selected == index;
        let jump_selected =
            selected && menu.bookmark_focus == BookmarkFocus::Jump && bookmark.is_some();
        let set_selected = selected && menu.bookmark_focus == BookmarkFocus::Set;
        let path_width = jump_rect.width.saturating_sub(4) as usize;
        let path = bookmark
            .as_deref()
            .map(|path| fit_filename(path, path_width))
            .unwrap_or_else(|| "Empty".to_owned());
        frame.render_widget(
            Paragraph::new(format!(" {} {}", index + 1, pad_right(&path, path_width))).style(
                if jump_selected {
                    FOCUSED
                } else if bookmark.is_none() {
                    Style::new().fg(Color::DarkGray).bg(Color::Gray)
                } else {
                    DIALOG
                },
            ),
            jump_rect,
        );
        frame.render_widget(
            Paragraph::new(" Set ").style(if set_selected { FOCUSED } else { DIALOG }),
            set_rect,
        );
        if bookmark.is_some() {
            layout.bookmark_rows.push((index, jump_rect));
        }
        layout.bookmark_sets.push((index, set_rect));
    }
}

pub(crate) fn render_sort_menu(frame: &mut Frame, app: &App, layout: &mut LayoutInfo) {
    let Some(menu) = app.sort_menu else {
        return;
    };
    let anchor = layout.sort_menu;
    let width = 25_u16.min(frame.area().width).max(1);
    let height = 10_u16;
    let x = anchor.right().saturating_sub(width);
    let area = Rect::new(x, anchor.y + 1, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(if menu.pane == 0 {
                " Left sort "
            } else {
                " Right sort "
            })
            .style(DIALOG),
        area,
    );
    let spec = app.panes[menu.pane].sort;
    for index in 0..8 {
        let rect = Rect::new(
            area.x + 1,
            area.y + 1 + index as u16,
            area.width.saturating_sub(2),
            1,
        );
        let (checked, label) = match index {
            0..=5 => (
                spec.mode == SortMode::ALL[index],
                SortMode::ALL[index].label(),
            ),
            6 => (spec.reverse, "Reverse"),
            7 => (spec.directories_first, "Directories first"),
            _ => unreachable!(),
        };
        let marker = if checked { "●" } else { " " };
        frame.render_widget(
            Paragraph::new(format!(" {marker} {label}")).style(if menu.selected == index {
                FOCUSED
            } else {
                DIALOG
            }),
            rect,
        );
        layout.sort_items.push((index, rect));
    }
}
