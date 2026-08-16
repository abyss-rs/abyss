use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::Error;
use crate::copy::{ConflictDecision, ConflictResolver};
use crate::progress::CopyStats;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Copy,
    Move,
    Delete,
    Trash,
    Sync,
    Archive,
    Hash,
    Verify,
    Extract,
    Test,
}

impl OperationKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Move => "Move / Rename",
            Self::Delete => "Delete",
            Self::Trash => "Trash",
            Self::Sync => "Sync",
            Self::Archive => "Archive",
            Self::Hash => "Hash",
            Self::Verify => "Verify hashes",
            Self::Extract => "Extract",
            Self::Test => "Test",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictChoice {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    Cancel,
}

#[derive(Debug)]
pub enum OperationEvent {
    Conflict(PathBuf),
    Finished(Result<(), Error>),
}

pub struct OperationHandle {
    pub stats: Arc<CopyStats>,
    pub cancelled: Arc<AtomicBool>,
    pub(super) events: Receiver<OperationEvent>,
    pub(super) conflict_response: Sender<ConflictChoice>,
}
pub struct ChannelConflictResolver {
    pub(super) events: Sender<OperationEvent>,
    responses: Mutex<Receiver<ConflictChoice>>,
    mode: Mutex<Option<ConflictDecision>>,
    cancelled: Arc<AtomicBool>,
}

impl ChannelConflictResolver {
    pub(crate) fn new(
        events: Sender<OperationEvent>,
        responses: Receiver<ConflictChoice>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            events,
            responses: Mutex::new(responses),
            mode: Mutex::new(None),
            cancelled,
        }
    }

    pub(crate) fn with_mode(
        events: Sender<OperationEvent>,
        responses: Receiver<ConflictChoice>,
        cancelled: Arc<AtomicBool>,
        mode: ConflictDecision,
    ) -> Self {
        Self {
            events,
            responses: Mutex::new(responses),
            mode: Mutex::new(Some(mode)),
            cancelled,
        }
    }
}

impl ConflictResolver for ChannelConflictResolver {
    fn resolve(&self, destination: &Path) -> Result<ConflictDecision, Error> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Ok(ConflictDecision::Cancel);
        }
        if let Some(mode) = *self
            .mode
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            return Ok(mode);
        }

        self.events
            .send(OperationEvent::Conflict(destination.to_owned()))
            .map_err(|_| Error::message("operation UI closed during conflict prompt"))?;
        let choice = self
            .responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recv()
            .map_err(|_| Error::Cancelled)?;
        let decision = match choice {
            ConflictChoice::Overwrite => ConflictDecision::Overwrite,
            ConflictChoice::OverwriteAll => {
                *self
                    .mode
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(ConflictDecision::Overwrite);
                ConflictDecision::Overwrite
            }
            ConflictChoice::Skip => ConflictDecision::Skip,
            ConflictChoice::SkipAll => {
                *self
                    .mode
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(ConflictDecision::Skip);
                ConflictDecision::Skip
            }
            ConflictChoice::Cancel => ConflictDecision::Cancel,
        };
        Ok(decision)
    }
}
