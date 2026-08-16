use std::sync::Arc;

use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use crate::archive::{ArchiveIndex, ArchiveRequest};
use crate::storage::Location;

#[derive(Clone)]
pub(crate) enum InputAction {
    Mkdir,
    SmartJump,
    FindFiles,
    GrepTree,
    BandwidthLimit,
    Copy(Vec<Location>),
    Move(Vec<Location>),
    ArchivePassword(ArchiveRequest),
    Extract {
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        password: Option<Zeroizing<String>>,
        temporary: Option<Arc<NamedTempFile>>,
    },
}

#[derive(Clone)]
pub(crate) struct InputDialog {
    pub(crate) title: String,
    pub(crate) prompt: String,
    pub(crate) value: Vec<char>,
    pub(crate) cursor: usize,
    pub(crate) action: InputAction,
    pub(crate) masked: bool,
}

impl InputDialog {
    pub(crate) fn new(title: &str, prompt: &str, value: String, action: InputAction) -> Self {
        let value: Vec<char> = value.chars().collect();
        let cursor = value.len();
        Self {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            value,
            cursor,
            action,
            masked: false,
        }
    }

    pub(crate) fn password(request: ArchiveRequest, invalid: bool) -> Self {
        let mut dialog = Self::new(
            if invalid {
                "Wrong archive password"
            } else {
                "Encrypted archive"
            },
            "Password:",
            String::new(),
            InputAction::ArchivePassword(request),
        );
        dialog.masked = true;
        dialog
    }

    pub(crate) fn text(&self) -> String {
        self.value.iter().collect()
    }

    pub(crate) fn supports_background(&self) -> bool {
        matches!(
            self.action,
            InputAction::Copy(_) | InputAction::Move(_) | InputAction::Extract { .. }
        )
    }

    pub(crate) fn insert(&mut self, value: char) {
        self.value.insert(self.cursor, value);
        self.cursor += 1;
    }

    pub(crate) fn paste(&mut self, value: &str) {
        for character in value.chars() {
            self.insert(character);
        }
    }
}
