use crate::app::menu::{AppMenu, BookmarkFocus, MenuAction, MenuCategory};
use crate::app::state::App;
use crate::storage::Location;
use crate::workspace::StoredLocation;

impl App {
    pub(crate) fn open_app_menu(&mut self) {
        self.open_menu_category(MenuCategory::Navigate);
    }

    pub(crate) fn open_menu_category(&mut self, category: MenuCategory) {
        let selected = self
            .app_menu
            .filter(|menu| menu.category == category)
            .map_or(0, |menu| menu.selected);
        let mut menu = AppMenu {
            category,
            selected,
            bookmark_focus: BookmarkFocus::Jump,
        };
        self.normalize_menu_selection(&mut menu);
        self.sort_menu = None;
        self.app_menu = Some(menu);
    }

    pub(crate) fn visible_menu_actions(&self, category: MenuCategory) -> Vec<MenuAction> {
        MenuAction::for_category(category)
            .iter()
            .copied()
            .filter(|action| self.menu_action_available(*action))
            .collect()
    }

    pub(crate) fn menu_action_checked(&self, action: MenuAction) -> bool {
        match action {
            MenuAction::SynchronizedScrolling => self.synchronized_scrolling,
            MenuAction::DirectoryComparison => self.comparison,
            _ => false,
        }
    }

    pub(crate) fn bookmark_display(&self, index: usize) -> Option<String> {
        self.workspace.bookmark(index).map(StoredLocation::display)
    }

    pub(crate) fn normalize_menu_selection(&self, menu: &mut AppMenu) {
        if menu.category == MenuCategory::Bookmarks {
            menu.selected = menu.selected.min(8);
            if self.workspace.bookmark(menu.selected).is_none() {
                menu.bookmark_focus = BookmarkFocus::Set;
            }
            return;
        }
        let count = self.visible_menu_actions(menu.category).len();
        menu.selected = menu.selected.min(count.saturating_sub(1));
    }

    pub(crate) fn menu_action_available(&self, action: MenuAction) -> bool {
        let pane = &self.panes[self.active];
        let in_sources = pane.showing_sources();
        match action {
            MenuAction::DirectoryHistory => !in_sources && !self.workspace.history.is_empty(),
            MenuAction::SmartJump => !in_sources,
            MenuAction::NewTab => !in_sources,
            MenuAction::CloseTab => !in_sources && pane.tab_count() > 1,
            MenuAction::SynchronizedScrolling | MenuAction::DirectoryComparison => {
                !self.panes[0].showing_sources() && !self.panes[1].showing_sources()
            }
            MenuAction::Jobs => !self.jobs.history().is_empty(),
            MenuAction::OpenDefaultApp | MenuAction::OpenEditor => {
                !in_sources && matches!(pane.current_location(), Some(Location::Local(_)))
            }
            MenuAction::OpenSubshell => !in_sources && pane.location.is_local(),
            MenuAction::Inspect => !in_sources,
            // Always available: it describes the machine, not the panes.
            MenuAction::SystemMonitor => true,
            // Comparing needs a real file selected on this side.
            MenuAction::DiffPanes => {
                !in_sources && matches!(pane.current_location(), Some(Location::Local(_)))
            }
            // Both walk a real directory tree, so a remote or archive pane has
            // nothing for them to search.
            MenuAction::FindFiles | MenuAction::GrepTree => {
                !in_sources && !pane.is_archive() && pane.location.is_local()
            }
            MenuAction::CreateArchive | MenuAction::Hashes => {
                if in_sources || pane.is_archive() {
                    return false;
                }
                let selected = pane.selected_locations();
                !selected.is_empty() && selected.iter().all(Location::is_local)
            }
            MenuAction::DifferentialSync => {
                !in_sources
                    && !self.panes[0].showing_sources()
                    && !self.panes[1].showing_sources()
                    && !self.panes[0].is_archive()
                    && !self.panes[1].is_archive()
                    && !self.location_read_only(&self.panes[1 - self.active].location)
            }
            #[cfg(feature = "kubernetes")]
            MenuAction::VolumeSnapshot => {
                if in_sources || self.snapshot_load.is_some() {
                    return false;
                }
                let Some(Location::Remote(location)) = pane.current_location() else {
                    return false;
                };
                self.browser
                    .storage()
                    .backend(&location)
                    .is_ok_and(|backend| backend.capabilities().volume_snapshot)
            }
        }
    }

    pub(crate) fn perform_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::DirectoryHistory => self.open_directory_history(),
            MenuAction::SmartJump => self.open_smart_jump(),
            MenuAction::NewTab => self.open_new_tab(),
            MenuAction::CloseTab => self.close_active_tab(),
            MenuAction::SynchronizedScrolling => self.toggle_synchronized_scrolling(),
            MenuAction::DirectoryComparison => self.toggle_directory_comparison(),
            MenuAction::FindFiles => self.prompt_find_files(),
            MenuAction::GrepTree => self.prompt_grep_tree(),
            MenuAction::DiffPanes => self.diff_with_other_pane(),
            MenuAction::SystemMonitor => self.open_monitor(),
            MenuAction::Jobs => self.open_jobs_panel(None),
            MenuAction::OpenDefaultApp => self.open_with_default_app(),
            MenuAction::OpenEditor => self.open_in_editor(),
            MenuAction::OpenSubshell => self.spawn_subshell(),
            MenuAction::Inspect => self.open_inspect(),
            MenuAction::CreateArchive => self.open_archive_create(),
            MenuAction::Hashes => self.open_hash_action(),
            MenuAction::DifferentialSync => self.open_sync_session(),
            #[cfg(feature = "kubernetes")]
            MenuAction::VolumeSnapshot => self.create_pvc_snapshot(),
        }
    }
}
