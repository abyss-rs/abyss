use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;

use crate::copy::ConflictDecision;
use crate::operation::copy::copy_paths;
use crate::operation::delete::{delete_paths, trash_paths};
use crate::operation::move_op::move_paths;
use crate::operation::types::{
    ChannelConflictResolver, OperationEvent, OperationHandle, OperationKind,
};
use crate::progress::CopyStats;
use crate::storage::{Location, StorageRuntime};

impl OperationHandle {
    pub fn start_copy(sources: Vec<PathBuf>, destination: PathBuf, kind: OperationKind) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let resolver =
            ChannelConflictResolver::new(event_tx.clone(), response_rx, Arc::clone(&cancelled));
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result = match kind {
                OperationKind::Copy => copy_paths(
                    &sources,
                    &destination,
                    thread_cancelled,
                    thread_stats,
                    &resolver,
                ),
                OperationKind::Move => move_paths(
                    &sources,
                    &destination,
                    thread_cancelled,
                    thread_stats,
                    &resolver,
                ),
                OperationKind::Delete => unreachable!("delete has a separate constructor"),
                OperationKind::Trash => unreachable!("trash has a separate constructor"),
                OperationKind::Sync => unreachable!("sync has a separate constructor"),
                OperationKind::Archive => unreachable!("archive has a separate constructor"),
                OperationKind::Hash => unreachable!("hash has a separate constructor"),
                OperationKind::Verify => unreachable!("verify has a separate constructor"),
                OperationKind::Extract => unreachable!("extract has a separate constructor"),
                OperationKind::Test => unreachable!("test has a separate constructor"),
            };
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_sync_file(
        storage: Arc<StorageRuntime>,
        source: Location,
        destination: Location,
    ) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let resolver = ChannelConflictResolver::with_mode(
            event_tx.clone(),
            response_rx,
            Arc::clone(&cancelled),
            ConflictDecision::Overwrite,
        );
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result = match (&source, &destination) {
                (Location::Local(src), Location::Local(dst)) => copy_paths(
                    std::slice::from_ref(src),
                    dst,
                    thread_cancelled,
                    thread_stats,
                    &resolver,
                ),
                #[cfg(feature = "tokio")]
                _ => crate::remote_operation::transfer(
                    storage,
                    vec![source],
                    destination,
                    false,
                    thread_cancelled,
                    thread_stats,
                    &resolver,
                ),
                #[cfg(not(feature = "tokio"))]
                _ => {
                    let _ = (storage, source, destination);
                    Err(Error::message(
                        "remote storage is disabled in this build; build with --features remote",
                    ))
                }
            };
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_delete(sources: Vec<PathBuf>) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result = delete_paths(&sources, &thread_cancelled, &thread_stats);
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_trash(sources: Vec<PathBuf>) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result = trash_paths(&sources, &thread_cancelled, &thread_stats);
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }
}
