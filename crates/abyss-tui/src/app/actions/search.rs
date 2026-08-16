use std::path::{Path, PathBuf};

use crate::app::dialogs::{FindDialog, InputAction, InputDialog, Modal};
use crate::app::runner::os_string_from_external;
use crate::app::state::App;
use crate::search::{RESULT_LIMIT, SearchKind, SearchLoad, SearchRequest};
use crate::storage::Location;

impl App {
    /// Root a search at the active pane, which must be local.
    fn search_root(&mut self, label: &str) -> Option<PathBuf> {
        match &self.panes[self.active].location {
            Location::Local(path) => Some(path.clone()),
            Location::Remote(_) => {
                self.set_status(format!("{label} needs a local directory"));
                None
            }
        }
    }

    pub(crate) fn prompt_find_files(&mut self) {
        if self.search_root("Find Files").is_some() {
            self.modal = Some(Modal::Input(InputDialog::new(
                "Find Files",
                "Name contains:",
                String::new(),
                InputAction::FindFiles,
            )));
        }
    }

    pub(crate) fn prompt_grep_tree(&mut self) {
        if self.search_root("Grep in Tree").is_some() {
            self.modal = Some(Modal::Input(InputDialog::new(
                "Grep in Tree",
                "Pattern (regex):",
                String::new(),
                InputAction::GrepTree,
            )));
        }
    }

    /// Kick off a search on a background thread.
    pub(crate) fn start_search(&mut self, query: String, kind: SearchKind) {
        let label = match kind {
            SearchKind::Files => "Find Files",
            SearchKind::Contents => "Grep in Tree",
        };
        let Some(root) = self.search_root(label) else {
            return;
        };
        self.search_load = Some(SearchLoad::start(SearchRequest {
            root,
            query: query.clone(),
            kind,
            // Matching fd and rg: ignore rules on by default, so results are
            // about the user's own files rather than build output.
            respect_ignore: true,
        }));
        self.set_status(format!("{label}: searching for {query}…"));
    }

    /// Collect a finished search and show its results.
    pub(crate) fn poll_search(&mut self) -> bool {
        let Some(result) = self.search_load.as_ref().and_then(SearchLoad::try_recv) else {
            return false;
        };
        let request = self
            .search_load
            .take()
            .expect("loader exists")
            .request
            .clone();
        match result {
            Ok(hits) if hits.is_empty() => {
                self.set_status(format!("No matches for {}", request.query));
            }
            Ok(hits) => {
                let truncated = hits.len() >= RESULT_LIMIT;
                let count = hits.len();
                self.set_status(format!("{count} matches for {}", request.query));
                self.modal = Some(Modal::Find(FindDialog::new(
                    request.query,
                    request.kind,
                    hits,
                    truncated,
                )));
            }
            Err(error) => self.show_error("Search", error),
        }
        true
    }

    /// Open whatever the find dialog has selected.
    ///
    /// Directories move the pane; files move the pane to the parent and select
    /// the file, so the result lands in context rather than opening blind.
    pub(crate) fn open_find_selection(&mut self, dialog: &FindDialog) {
        let Some(hit) = dialog.current() else {
            return;
        };
        let path = hit.path.clone();
        if path.is_dir() {
            self.navigate_active_to(Location::Local(path));
            return;
        }
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let name = path
            .file_name()
            .map(|name| os_string_from_external(name.as_encoded_bytes().to_vec()));
        self.navigate_active_to(Location::Local(parent));
        if let Some(name) = name {
            let pane = self.active;
            self.panes[pane].reload_selecting(pane, name, &self.browser);
        }
    }
}
