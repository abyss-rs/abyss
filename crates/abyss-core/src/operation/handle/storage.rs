use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;

use zeroize::Zeroizing;

use crate::Error;
use crate::archive::ArchiveIndex;
use crate::operation::copy::extract_paths;
use crate::operation::types::{
    ChannelConflictResolver, OperationEvent, OperationHandle, OperationKind,
};
use crate::progress::CopyStats;
use crate::storage::{Location, StorageRuntime};

impl OperationHandle {
    pub fn start_storage_copy(
        storage: Arc<StorageRuntime>,
        sources: Vec<Location>,
        destination: Location,
        kind: OperationKind,
    ) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let resolver =
            ChannelConflictResolver::new(event_tx.clone(), response_rx, Arc::clone(&cancelled));
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);
        thread::spawn(move || {
            #[cfg(feature = "tokio")]
            let result = crate::remote_operation::transfer(
                storage,
                sources,
                destination,
                kind == OperationKind::Move,
                thread_cancelled,
                thread_stats,
                &resolver,
            );
            #[cfg(not(feature = "tokio"))]
            let result = {
                let _ = (
                    storage,
                    sources,
                    destination,
                    kind,
                    resolver,
                    thread_stats,
                    thread_cancelled,
                );
                Err(Error::message(
                    "remote storage is disabled in this build; build with --features remote",
                ))
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

    pub fn start_storage_delete(storage: Arc<StorageRuntime>, sources: Vec<Location>) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);
        thread::spawn(move || {
            #[cfg(feature = "tokio")]
            let result =
                crate::remote_operation::delete(storage, sources, thread_cancelled, thread_stats);
            #[cfg(not(feature = "tokio"))]
            let result = {
                let _ = (storage, sources, thread_stats, thread_cancelled);
                Err(Error::message(
                    "remote storage is disabled in this build; build with --features remote",
                ))
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

    pub fn start_storage_extract(
        storage: Arc<StorageRuntime>,
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        destination: Location,
        password: Option<Zeroizing<String>>,
        temporary_hold: Option<Arc<tempfile::NamedTempFile>>,
    ) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let resolver =
            ChannelConflictResolver::new(event_tx.clone(), response_rx, Arc::clone(&cancelled));
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);
        thread::spawn(move || {
            let _temporary_hold = temporary_hold;
            let result = (|| {
                let staging = tempfile::tempdir().map_err(|error| {
                    Error::message(format!("create extraction staging: {error}"))
                })?;
                extract_paths(
                    &index,
                    &roots,
                    &base,
                    staging.path(),
                    password.as_deref().map(String::as_str),
                    &thread_cancelled,
                    &thread_stats,
                    &resolver,
                )?;
                let sources = fs::read_dir(staging.path())
                    .map_err(|error| Error::io("read extraction staging", staging.path(), error))?
                    .map(|entry| {
                        entry
                            .map(|entry| Location::Local(entry.path()))
                            .map_err(|error| {
                                Error::io("read extraction staging", staging.path(), error)
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                #[cfg(feature = "tokio")]
                {
                    crate::remote_operation::transfer(
                        storage,
                        sources,
                        destination,
                        false,
                        Arc::clone(&thread_cancelled),
                        Arc::clone(&thread_stats),
                        &resolver,
                    )
                }
                #[cfg(not(feature = "tokio"))]
                {
                    let _ = (storage, sources, destination);
                    Err(Error::message(
                        "remote storage is disabled in this build; build with --features remote",
                    ))
                }
            })();
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
