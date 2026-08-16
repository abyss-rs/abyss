use std::time::Instant;

use crate::app::dialogs::Modal;
use crate::app::state::App;
use crate::browser::SourceProbeStatus;
use crate::frontend::NOTICE_LIFETIME;
use crate::jobs::{JobId, JobOutcome};

#[derive(Clone, Copy)]
pub(crate) struct JobsPanel {
    pub(crate) selected: Option<JobId>,
}

pub(crate) struct CompletionNotice {
    pub(crate) text: String,
    pub(crate) outcome: JobOutcome,
    pub(crate) until: Instant,
}

impl App {
    pub(crate) fn expire_completion_notice(&mut self, now: Instant) -> bool {
        self.completion_notice
            .take_if(|notice| now >= notice.until)
            .is_some()
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        let status = status.into();
        if status == "Ready" {
            self.clear_status();
            return;
        }
        self.completion_notice = None;
        self.status = status;
        self.status_until = Some(Instant::now() + NOTICE_LIFETIME);
    }

    pub(crate) fn set_completion_notice(&mut self, text: String, outcome: JobOutcome) {
        self.status = "Ready".to_owned();
        self.status_until = None;
        self.completion_notice = Some(CompletionNotice {
            text,
            outcome,
            until: Instant::now() + NOTICE_LIFETIME,
        });
    }

    pub(crate) fn clear_status(&mut self) {
        self.status = "Ready".to_owned();
        self.status_until = None;
    }

    pub(crate) fn expire_status(&mut self, now: Instant) -> bool {
        if self.status_until.is_some_and(|deadline| now >= deadline) {
            self.clear_status();
            true
        } else {
            false
        }
    }

    pub(crate) fn show_selected_source_error(&mut self) {
        let error =
            self.panes[self.active]
                .selected_source()
                .and_then(|entry| match &entry.status {
                    SourceProbeStatus::Unavailable(error) => Some(error.clone()),
                    _ => None,
                });
        if let Some(error) = error {
            self.set_status(error);
        } else {
            self.clear_status();
        }
    }

    pub(crate) fn show_error(&mut self, title: impl Into<String>, text: impl Into<String>) {
        let title = title.into();
        let text = text.into();
        self.set_status(format!("{title} failed: {text}"));
        self.modal = Some(Modal::Message {
            title,
            text,
            error: true,
        });
    }
}
