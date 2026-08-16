use ratatui::layout::Rect;

use crate::app::{
    ArchiveCreateField, HashCreateField, MenuAction, MenuCategory, SyncMenuAction, SyncMenuCategory,
};
use crate::jobs::JobId;
use crate::ui::helpers::contains;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionButton {
    /// Leave Analyze or Sync mode (shown as Esc before digit 1).
    EscLeave,
    Help,
    View,
    Mkdir,
    Copy,
    Move,
    Delete,
    Refresh,
    Sync,
    Analyze,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DialogButton {
    Start,
    Background,
    Cancel,
}

impl ActionButton {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::EscLeave => "Esc",
            Self::Help => "1",
            Self::View => "2",
            Self::Sync => "3",
            Self::Analyze => "4",
            Self::Copy => "5",
            Self::Move => "6",
            Self::Mkdir => "7",
            Self::Delete => "8",
            Self::Refresh => "9",
            Self::Quit => "0",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::EscLeave => "Quit",
            Self::Help => "Help",
            Self::View => "View",
            Self::Mkdir => "Mkdir",
            Self::Copy => "Copy",
            Self::Move => "Move",
            Self::Delete => "Delete",
            Self::Refresh => "Refresh",
            Self::Sync => "Sync",
            Self::Analyze => "Analyze",
            Self::Quit => "Quit",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LayoutInfo {
    pub(crate) pane_rects: [Rect; 2],
    /// Interior of the console (inside its border); empty when hidden.
    pub(crate) console: Rect,
    pub(crate) menu_headings: Vec<(MenuCategory, Rect)>,
    pub(crate) menu_items: Vec<(MenuAction, Rect)>,
    pub(crate) sync_menu_headings: Vec<(SyncMenuCategory, Rect)>,
    pub(crate) sync_menu_items: Vec<(SyncMenuAction, Rect)>,
    pub(crate) bookmark_rows: Vec<(usize, Rect)>,
    pub(crate) bookmark_sets: Vec<(usize, Rect)>,
    pub(crate) sort_menu: Rect,
    pub(crate) sort_items: Vec<(usize, Rect)>,
    /// Clickable `[` / `]` on each pane title: `(pane, delta, rect)`.
    pub(crate) tab_nav: Vec<(usize, isize, Rect)>,
    pub(crate) rows: Vec<(usize, usize, Rect)>,
    pub(crate) buttons: Vec<(ActionButton, Rect)>,
    pub(crate) job_rows: Vec<(JobId, Rect)>,
    pub(crate) dialog_buttons: Vec<(DialogButton, Rect)>,
    pub(crate) archive_fields: Vec<(ArchiveCreateField, Rect)>,
    pub(crate) hash_fields: Vec<(HashCreateField, Rect)>,
    pub(crate) pane_rows: usize,
}

impl LayoutInfo {
    pub(crate) fn pane_at(&self, column: u16, row: u16) -> Option<usize> {
        self.pane_rects
            .iter()
            .position(|rect| contains(*rect, column, row))
    }

    pub(crate) fn row_at(&self, column: u16, row: u16) -> Option<(usize, usize)> {
        self.rows
            .iter()
            .find(|(_, _, rect)| contains(*rect, column, row))
            .map(|(pane, index, _)| (*pane, *index))
    }

    pub(crate) fn button_at(&self, column: u16, row: u16) -> Option<ActionButton> {
        self.buttons
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(button, _)| *button)
    }

    pub(crate) fn menu_heading_at(&self, column: u16, row: u16) -> Option<MenuCategory> {
        self.menu_headings
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(category, _)| *category)
    }

    pub(crate) fn sync_menu_heading_at(&self, column: u16, row: u16) -> Option<SyncMenuCategory> {
        self.sync_menu_headings
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(category, _)| *category)
    }

    pub(crate) fn sync_menu_item_at(&self, column: u16, row: u16) -> Option<SyncMenuAction> {
        self.sync_menu_items
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(action, _)| *action)
    }

    pub(crate) fn menu_item_at(&self, column: u16, row: u16) -> Option<MenuAction> {
        self.menu_items
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(action, _)| *action)
    }

    pub(crate) fn bookmark_row_at(&self, column: u16, row: u16) -> Option<usize> {
        self.bookmark_rows
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn bookmark_set_at(&self, column: u16, row: u16) -> Option<usize> {
        self.bookmark_sets
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(index, _)| *index)
    }

    pub(crate) fn sort_menu_at(&self, column: u16, row: u16) -> bool {
        contains(self.sort_menu, column, row)
    }

    pub(crate) fn tab_nav_at(&self, column: u16, row: u16) -> Option<(usize, isize)> {
        self.tab_nav
            .iter()
            .find(|(_, _, rect)| contains(*rect, column, row))
            .map(|(pane, delta, _)| (*pane, *delta))
    }

    pub(crate) fn job_at(&self, column: u16, row: u16) -> Option<JobId> {
        self.job_rows
            .iter()
            .rev()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(id, _)| *id)
    }

    pub(crate) fn dialog_button_at(&self, column: u16, row: u16) -> Option<DialogButton> {
        self.dialog_buttons
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(button, _)| *button)
    }

    pub(crate) fn archive_field_at(&self, column: u16, row: u16) -> Option<ArchiveCreateField> {
        self.archive_fields
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(field, _)| *field)
    }

    pub(crate) fn hash_field_at(&self, column: u16, row: u16) -> Option<HashCreateField> {
        self.hash_fields
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(field, _)| *field)
    }

    pub(crate) fn sort_item_at(&self, column: u16, row: u16) -> Option<usize> {
        self.sort_items
            .iter()
            .find(|(_, rect)| contains(*rect, column, row))
            .map(|(index, _)| *index)
    }
}
