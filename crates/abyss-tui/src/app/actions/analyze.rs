use std::sync::Arc;

use cleaner_tui::{Session, StartOpts};

use crate::app::dialogs::Modal;
use crate::app::state::App;
use crate::storage::Location;

impl App {
    pub(crate) fn open_analyze_clean_confirm(&mut self) {
        let Some(session) = self.analyze.as_ref() else {
            return;
        };
        match session.clean_offer() {
            cleaner_tui::CleanOffer::Unavailable(message) => {
                self.set_status(message);
            }
            cleaner_tui::CleanOffer::Empty { path } => {
                self.modal = Some(Modal::Message {
                    title: "Clean".into(),
                    text: format!("Nothing to clean under:\n\n  {}", path.display()),
                    error: false,
                });
            }
            cleaner_tui::CleanOffer::Ready {
                path,
                dirs,
                files,
                bytes,
            } => {
                self.modal = Some(Modal::ConfirmClean {
                    path,
                    dirs,
                    files,
                    bytes,
                });
            }
        }
    }

    pub(crate) fn open_analyze(&mut self) {
        if self.analyze.is_some() {
            return;
        }
        let pane = &self.panes[self.active];
        if pane.showing_sources() || pane.is_archive() {
            self.set_status("Analyze is local-only");
            return;
        }
        let Location::Local(root) = pane.location.clone() else {
            self.set_status("Analyze is local-only");
            return;
        };
        let config = match cleaner_tui::Config::try_load(None) {
            Ok(config) => Arc::new(config),
            Err(error) => {
                self.set_status(format!("Analyze config error: {error}"));
                return;
            }
        };
        self.app_menu = None;
        self.sort_menu = None;
        self.modal = None;
        self.analyze = Some(Session::start(
            root,
            config,
            StartOpts {
                index_enabled: false,
                rebuild_index: false,
            },
        ));
    }

    pub(crate) fn leave_analyze(&mut self) {
        self.analyze = None;
        self.refresh_all();
        self.set_status("Left Analyze");
    }
}
