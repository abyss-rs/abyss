pub(crate) mod archive;
pub(crate) mod find;
pub(crate) mod hash;
pub(crate) mod history;
pub(crate) mod input;

use std::path::PathBuf;

use crate::inspect::InspectDialog;
use crate::jobs::JobId;
use crate::storage::Location;
use crate::sync::SyncPlan;

pub(crate) use self::archive::{ArchiveCreateDialog, ArchiveCreateField};
pub(crate) use self::find::FindDialog;
pub(crate) use self::hash::{HashCreateDialog, HashCreateField};
pub(crate) use self::history::HistoryDialog;
pub(crate) use self::input::{InputAction, InputDialog};

#[derive(Clone)]
pub(crate) enum Modal {
    Help,
    Input(InputDialog),
    History(HistoryDialog),
    Find(FindDialog),
    ArchiveCreate(ArchiveCreateDialog),
    HashCreate(HashCreateDialog),
    VerifyHash(PathBuf),
    ConfirmSync(SyncPlan),
    ConfirmDelete {
        paths: Vec<Location>,
        trash_available: bool,
    },
    ConfirmClean {
        path: PathBuf,
        dirs: usize,
        files: usize,
        bytes: u64,
    },
    Conflict {
        job_id: JobId,
        path: PathBuf,
    },
    Message {
        title: String,
        text: String,
        error: bool,
    },
    QuitJobs,
    Inspect(InspectDialog),
}
