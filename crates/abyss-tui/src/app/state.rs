use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use cleaner_tui::Session;
use tempfile::NamedTempFile;

use crate::app::dialogs::Modal;
use crate::app::menu::{AppMenu, SortMenu};
use crate::app::status::{CompletionNotice, JobsPanel};
use crate::app::sync::SyncSession;
use crate::archive::ArchiveLoad;
use crate::browser::{BrowserEntry, BrowserKind, BrowserService, Pane};
use crate::console::{Console, ConsoleView};
use crate::diff::FileDiff;
use crate::frontend::NOTICE_LIFETIME;
use crate::highlight::Piece;
use crate::jobs::{JobId, JobManager};
use crate::monitor::Monitor;
use crate::search::SearchLoad;
use crate::storage::Location;
#[cfg(feature = "kubernetes")]
use crate::tasks::SnapshotLoad;
use crate::tasks::{RemoteDownload, SyncLoad};
use crate::ui::LayoutInfo;
use crate::viewer::{Viewer, ViewerLoad};
use crate::workspace::{PaneTabs, WorkspaceState, fallback_home_location};

#[derive(Clone)]
pub(crate) struct PendingResolve {
    pub(crate) token: u64,
    pub(crate) pane: usize,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Copy)]
pub(crate) struct LastClick {
    pub(crate) pane: usize,
    pub(crate) index: usize,
    pub(crate) at: Instant,
}

pub(crate) enum ExternalAction {
    Shell(PathBuf),
    Open(PathBuf),
    Edit(PathBuf),
}

/// A rendered file comparison, shown fullscreen like the viewer.
pub(crate) struct DiffView {
    pub(crate) left_name: String,
    pub(crate) right_name: String,
    pub(crate) diff: FileDiff,
    pub(crate) vertical: usize,
    pub(crate) horizontal: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Difference {
    OnlyHere,
    Modified,
}

pub(crate) struct App {
    pub(crate) panes: [PaneTabs; 2],
    pub(crate) active: usize,
    pub(crate) modal: Option<Modal>,
    pub(crate) viewer: Option<Viewer>,
    /// Syntax-highlighted form of `viewer`, when the language is known.
    pub(crate) viewer_highlight: Option<Vec<Vec<Piece>>>,
    pub(crate) viewer_loading: Option<PathBuf>,
    pub(crate) jobs: JobManager,
    pub(crate) foreground_job: Option<JobId>,
    pub(crate) jobs_panel: Option<JobsPanel>,
    pub(crate) completion_notice: Option<CompletionNotice>,
    pub(crate) app_menu: Option<AppMenu>,
    pub(crate) sort_menu: Option<SortMenu>,
    pub(crate) status: String,
    pub(crate) status_until: Option<Instant>,
    pub(crate) pane_rows: usize,
    pub(crate) browser: BrowserService,
    pub(crate) viewer_load: Option<ViewerLoad>,
    pub(crate) archive_load: Option<ArchiveLoad>,
    pub(crate) viewer_temp: Option<NamedTempFile>,
    pub(crate) remote_load: Option<RemoteDownload>,
    pub(crate) remote_temp: Option<NamedTempFile>,
    pub(crate) remote_archive_display: Option<String>,
    pub(crate) pending_resolve: Option<PendingResolve>,
    pub(crate) pending_conflicts: VecDeque<(JobId, PathBuf)>,
    pub(crate) next_token: u64,
    pub(crate) layout: LayoutInfo,
    pub(crate) last_click: Option<LastClick>,
    pub(crate) should_quit: bool,
    pub(crate) quit_after_jobs: bool,
    pub(crate) workspace: WorkspaceState,
    pub(crate) synchronized_scrolling: bool,
    pub(crate) comparison: bool,
    pub(crate) pending_external: Option<ExternalAction>,
    pub(crate) sync_load: Option<SyncLoad>,
    /// Find Files or Grep in Tree running on a background thread.
    pub(crate) search_load: Option<SearchLoad>,
    #[cfg(feature = "kubernetes")]
    pub(crate) snapshot_load: Option<SnapshotLoad>,
    /// Fullscreen cleaner analyze session (replaces dual-pane UI while active).
    pub(crate) analyze: Option<Session>,
    /// Dedicated interactive sync session.
    pub(crate) sync: Option<SyncSession>,
    /// Persistent shell on a pty, spawned the first time the console opens.
    pub(crate) console: Option<Console>,
    /// How much of the screen the console currently occupies.
    pub(crate) console_view: ConsoleView,
    /// Fullscreen file comparison.
    pub(crate) diff: Option<DiffView>,
    /// Fullscreen system monitor.
    pub(crate) monitor: Option<Monitor>,
}

impl App {
    #[cfg(test)]
    pub(crate) fn new(left: impl Into<Location>, right: impl Into<Location>) -> Self {
        let left = left.into();
        let right = right.into();
        Self::build(
            [
                PaneTabs::new(Pane::new(left)),
                PaneTabs::new(Pane::new(right)),
            ],
            WorkspaceState::default(),
            None,
            0,
            false,
            false,
        )
    }

    pub(crate) fn from_workspace(
        left: Option<Location>,
        right: Option<Location>,
        mut workspace: WorkspaceState,
        warning: Option<String>,
    ) -> Self {
        let fallback = fallback_home_location();
        let restored = workspace.session.as_ref();
        let mut panes = [
            PaneTabs::from_session(restored.map(|session| &session.panes[0]), fallback.clone()),
            PaneTabs::from_session(restored.map(|session| &session.panes[1]), fallback.clone()),
        ];
        if let Some(left) = left {
            let left_loc = match &left {
                Location::Local(path) if !path.exists() => fallback.clone(),
                _ => left,
            };
            panes[0] = PaneTabs::new(Pane::new(left_loc));
        }
        if let Some(right) = right {
            let right_loc = match &right {
                Location::Local(path) if !path.exists() => fallback.clone(),
                _ => right,
            };
            panes[1] = PaneTabs::new(Pane::new(right_loc));
        }
        let active = restored.map_or(0, |session| session.active_pane.min(1));
        let synchronized = restored.is_some_and(|session| session.synchronized_scrolling);
        let comparison = restored.is_some_and(|session| session.comparison);
        let console_view = restored.map_or(ConsoleView::default(), |session| {
            ConsoleView::from(session.console_view)
        });
        workspace.session = None;
        let mut app = Self::build(panes, workspace, warning, active, synchronized, comparison);
        app.console_view = console_view;
        app
    }

    pub(crate) fn build(
        panes: [PaneTabs; 2],
        workspace: WorkspaceState,
        warning: Option<String>,
        active: usize,
        synchronized_scrolling: bool,
        comparison: bool,
    ) -> Self {
        let browser = BrowserService::new();
        let mut jobs = JobManager::new();
        jobs.set_bandwidth_limit(workspace.bandwidth_limit);
        let mut app = Self {
            panes,
            active,
            modal: None,
            viewer: None,
            viewer_highlight: None,
            viewer_loading: None,
            jobs,
            foreground_job: None,
            jobs_panel: None,
            completion_notice: None,
            app_menu: None,
            sort_menu: None,
            status: warning.clone().unwrap_or_else(|| "Ready".to_owned()),
            status_until: warning.map(|_| Instant::now() + NOTICE_LIFETIME),
            pane_rows: 1,
            browser,
            viewer_load: None,
            archive_load: None,
            viewer_temp: None,
            remote_load: None,
            remote_temp: None,
            remote_archive_display: None,
            pending_resolve: None,
            pending_conflicts: VecDeque::new(),
            next_token: 1,
            layout: LayoutInfo::default(),
            last_click: None,
            should_quit: false,
            quit_after_jobs: false,
            workspace,
            synchronized_scrolling,
            comparison,
            pending_external: None,
            sync_load: None,
            search_load: None,
            #[cfg(feature = "kubernetes")]
            snapshot_load: None,
            analyze: None,
            sync: None,
            console: None,
            console_view: ConsoleView::default(),
            diff: None,
            monitor: None,
        };
        app.panes[0].reload(0, &app.browser);
        app.panes[1].reload(1, &app.browser);
        app
    }

    pub(crate) fn entry_difference(
        &self,
        pane_index: usize,
        entry: &BrowserEntry,
    ) -> Option<Difference> {
        if !self.comparison || entry.kind == BrowserKind::Parent {
            return None;
        }
        let opposite = &self.panes[1 - pane_index];
        let Some(other) = opposite.find_entry(&entry.name) else {
            return Some(Difference::OnlyHere);
        };
        if other.kind == BrowserKind::Parent {
            return Some(Difference::OnlyHere);
        }
        if other.kind != entry.kind || other.size != entry.size || other.modified != entry.modified
        {
            Some(Difference::Modified)
        } else {
            None
        }
    }

    pub(crate) fn active_pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    pub(crate) fn location_read_only(&self, location: &Location) -> bool {
        let Location::Remote(location) = location else {
            return false;
        };
        self.browser
            .storage()
            .backend(location)
            .is_ok_and(|backend| backend.capabilities().read_only)
    }
}
