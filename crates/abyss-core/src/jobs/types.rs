use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use zeroize::Zeroizing;

use crate::archive::{ArchiveCreateOptions, ArchiveIndex};
use crate::hashing::HashCreateOptions;
use crate::operation::{OperationHandle, OperationKind};
use crate::progress::{ProgressSnapshot, SpeedSnapshot, Speedometer};
use crate::storage::{Location, StorageRuntime};

pub type JobId = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchMode {
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitReason {
    Capacity,
    Overlap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobOutcome {
    Succeeded,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued(WaitReason),
    Running,
    Paused,
    WaitingConflict(PathBuf),
    Cancelling,
    Finished { outcome: JobOutcome, at: Instant },
}

pub enum JobRequest {
    Copy {
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: OperationKind,
    },
    Delete {
        sources: Vec<PathBuf>,
    },
    Trash {
        sources: Vec<PathBuf>,
    },
    SyncFile {
        storage: Arc<StorageRuntime>,
        source: Location,
        destination: Location,
    },
    CreateArchive {
        options: ArchiveCreateOptions,
    },
    CreateHash {
        options: HashCreateOptions,
    },
    VerifyHash {
        database: PathBuf,
        root: PathBuf,
    },
    StorageCopy {
        storage: Arc<StorageRuntime>,
        sources: Vec<Location>,
        destination: Location,
        kind: OperationKind,
    },
    StorageDelete {
        storage: Arc<StorageRuntime>,
        sources: Vec<Location>,
    },
    StorageExtract {
        storage: Arc<StorageRuntime>,
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        destination: Location,
        password: Option<Zeroizing<String>>,
        temporary: Option<Arc<tempfile::NamedTempFile>>,
    },
    Extract {
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        destination: PathBuf,
        password: Option<Zeroizing<String>>,
        temporary: Option<Arc<tempfile::NamedTempFile>>,
    },
    TestArchive {
        index: Arc<ArchiveIndex>,
        password: Option<Zeroizing<String>>,
        temporary: Option<Arc<tempfile::NamedTempFile>>,
    },
}

impl JobRequest {
    pub(crate) fn kind(&self) -> OperationKind {
        match self {
            Self::Copy { kind, .. } => *kind,
            Self::Delete { .. } => OperationKind::Delete,
            Self::Trash { .. } => OperationKind::Trash,
            Self::SyncFile { .. } => OperationKind::Sync,
            Self::CreateArchive { .. } => OperationKind::Archive,
            Self::CreateHash { .. } => OperationKind::Hash,
            Self::VerifyHash { .. } => OperationKind::Verify,
            Self::StorageCopy { kind, .. } => *kind,
            Self::StorageDelete { .. } => OperationKind::Delete,
            Self::StorageExtract { .. } => OperationKind::Extract,
            Self::Extract { .. } => OperationKind::Extract,
            Self::TestArchive { .. } => OperationKind::Test,
        }
    }

    pub(crate) fn accesses(&self) -> Vec<PathAccess> {
        match self {
            Self::Copy {
                sources,
                destination,
                kind,
            } => {
                let source_mode = if *kind == OperationKind::Move {
                    AccessMode::Write
                } else {
                    AccessMode::Read
                };
                sources
                    .iter()
                    .cloned()
                    .map(|path| PathAccess {
                        path: Location::Local(path),
                        mode: source_mode,
                    })
                    .chain([PathAccess {
                        path: Location::Local(destination.clone()),
                        mode: AccessMode::Write,
                    }])
                    .collect()
            }
            Self::Delete { sources } => sources
                .iter()
                .cloned()
                .map(|path| PathAccess {
                    path: Location::Local(path),
                    mode: AccessMode::Write,
                })
                .collect(),
            Self::Trash { sources } => sources
                .iter()
                .cloned()
                .map(|path| PathAccess {
                    path: Location::Local(path),
                    mode: AccessMode::Write,
                })
                .collect(),
            Self::SyncFile {
                source,
                destination,
                ..
            } => vec![
                PathAccess {
                    path: source.clone(),
                    mode: AccessMode::Read,
                },
                PathAccess {
                    path: destination.clone(),
                    mode: AccessMode::Write,
                },
            ],
            Self::CreateArchive { options } => options
                .sources
                .iter()
                .cloned()
                .map(|path| PathAccess {
                    path: Location::Local(path),
                    mode: AccessMode::Read,
                })
                .chain([PathAccess {
                    path: Location::Local(options.destination.clone()),
                    mode: AccessMode::Write,
                }])
                .collect(),
            Self::CreateHash { options } => options
                .sources
                .iter()
                .cloned()
                .map(|path| PathAccess {
                    path: Location::Local(path),
                    mode: AccessMode::Read,
                })
                .chain([PathAccess {
                    path: Location::Local(options.destination.clone()),
                    mode: AccessMode::Write,
                }])
                .collect(),
            Self::VerifyHash { database, root } => vec![
                PathAccess {
                    path: Location::Local(database.clone()),
                    mode: AccessMode::Read,
                },
                PathAccess {
                    path: Location::Local(root.clone()),
                    mode: AccessMode::Read,
                },
            ],
            Self::StorageCopy {
                sources,
                destination,
                kind,
                ..
            } => {
                let source_mode = if *kind == OperationKind::Move {
                    AccessMode::Write
                } else {
                    AccessMode::Read
                };
                sources
                    .iter()
                    .map(|location| PathAccess {
                        path: location.clone(),
                        mode: source_mode,
                    })
                    .chain([PathAccess {
                        path: destination.clone(),
                        mode: AccessMode::Write,
                    }])
                    .collect()
            }
            Self::StorageDelete { sources, .. } => sources
                .iter()
                .map(|location| PathAccess {
                    path: location.clone(),
                    mode: AccessMode::Write,
                })
                .collect(),
            Self::StorageExtract {
                index, destination, ..
            } => vec![
                PathAccess {
                    path: Location::Local(index.source.clone()),
                    mode: AccessMode::Read,
                },
                PathAccess {
                    path: destination.clone(),
                    mode: AccessMode::Write,
                },
            ],
            Self::Extract {
                index, destination, ..
            } => vec![
                PathAccess {
                    path: Location::Local(index.source.clone()),
                    mode: AccessMode::Read,
                },
                PathAccess {
                    path: Location::Local(destination.clone()),
                    mode: AccessMode::Write,
                },
            ],
            Self::TestArchive { index, .. } => vec![PathAccess {
                path: Location::Local(index.source.clone()),
                mode: AccessMode::Read,
            }],
        }
    }

    pub(crate) fn start(self) -> OperationHandle {
        match self {
            Self::Copy {
                sources,
                destination,
                kind,
            } => OperationHandle::start_copy(sources, destination, kind),
            Self::Delete { sources } => OperationHandle::start_delete(sources),
            Self::Trash { sources } => OperationHandle::start_trash(sources),
            Self::SyncFile {
                storage,
                source,
                destination,
            } => OperationHandle::start_sync_file(storage, source, destination),
            Self::CreateArchive { options } => OperationHandle::start_create_archive(options),
            Self::CreateHash { options } => OperationHandle::start_create_hash(options),
            Self::VerifyHash { database, root } => {
                OperationHandle::start_verify_hash(database, root)
            }
            Self::StorageCopy {
                storage,
                sources,
                destination,
                kind,
            } => OperationHandle::start_storage_copy(storage, sources, destination, kind),
            Self::StorageDelete { storage, sources } => {
                OperationHandle::start_storage_delete(storage, sources)
            }
            Self::StorageExtract {
                storage,
                index,
                roots,
                base,
                destination,
                password,
                temporary,
            } => OperationHandle::start_storage_extract(
                storage,
                index,
                roots,
                base,
                destination,
                password,
                temporary,
            ),
            Self::Extract {
                index,
                roots,
                base,
                destination,
                password,
                temporary,
            } => {
                OperationHandle::start_extract(index, roots, base, destination, password, temporary)
            }
            Self::TestArchive {
                index,
                password,
                temporary,
            } => OperationHandle::start_test_archive(index, password, temporary),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccessMode {
    Read,
    Write,
}

#[derive(Clone, Debug)]
pub(crate) struct PathAccess {
    pub(crate) path: Location,
    pub(crate) mode: AccessMode,
}

pub struct Job {
    pub id: JobId,
    pub kind: OperationKind,
    pub launch: LaunchMode,
    pub state: JobState,
    pub initiating_pane: usize,
    pub delete_paths: Option<Vec<PathBuf>>,
    pub(crate) request: Option<JobRequest>,
    pub(crate) accesses: Vec<PathAccess>,
    pub(crate) handle: Option<OperationHandle>,
    pub(crate) speedometer: Speedometer,
    pub speed: Option<SpeedSnapshot>,
    pub(crate) final_snapshot: Option<ProgressSnapshot>,
}

impl Job {
    pub fn snapshot(&self) -> ProgressSnapshot {
        self.handle
            .as_ref()
            .map(|handle| handle.stats.snapshot())
            .or_else(|| self.final_snapshot.clone())
            .unwrap_or_default()
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            JobState::Queued(_)
                | JobState::Running
                | JobState::Paused
                | JobState::WaitingConflict(_)
                | JobState::Cancelling
        )
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }
}

pub enum JobUpdate {
    Conflict {
        id: JobId,
        path: PathBuf,
    },
    Finished {
        id: JobId,
        kind: OperationKind,
        outcome: JobOutcome,
        snapshot: ProgressSnapshot,
        initiating_pane: usize,
        delete_paths: Option<Vec<PathBuf>>,
    },
}
