use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;

use zeroize::Zeroizing;

use crate::archive::{self, ArchiveCreateOptions, ArchiveIndex};
use crate::hashing::{self, HashCreateOptions};
use crate::operation::copy::{extract_paths, test_archive};
use crate::operation::types::{ChannelConflictResolver, OperationEvent, OperationHandle};
use crate::progress::CopyStats;

impl OperationHandle {
    pub fn start_create_archive(options: ArchiveCreateOptions) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result =
                archive::create_archive(&options, &thread_cancelled, &thread_stats).map(|_| ());
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_create_hash(options: HashCreateOptions) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result =
                hashing::create_database(&options, &thread_cancelled, &thread_stats).map(|_| ());
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_verify_hash(database: PathBuf, root: PathBuf) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let result =
                hashing::verify_database(&database, &root, &thread_cancelled, &thread_stats);
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_extract(
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        destination: PathBuf,
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
            let result = extract_paths(
                &index,
                &roots,
                &base,
                &destination,
                password.as_deref().map(String::as_str),
                &thread_cancelled,
                &thread_stats,
                &resolver,
            );
            let _ = event_tx.send(OperationEvent::Finished(result));
        });

        Self {
            stats,
            cancelled,
            events,
            conflict_response: response_tx,
        }
    }

    pub fn start_test_archive(
        index: Arc<ArchiveIndex>,
        password: Option<Zeroizing<String>>,
        temporary_hold: Option<Arc<tempfile::NamedTempFile>>,
    ) -> Self {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (event_tx, events) = mpsc::channel();
        let (response_tx, _response_rx) = mpsc::channel();
        let thread_stats = Arc::clone(&stats);
        let thread_cancelled = Arc::clone(&cancelled);

        thread::spawn(move || {
            let _temporary_hold = temporary_hold;
            let result = test_archive(
                &index,
                password.as_deref().map(String::as_str),
                &thread_cancelled,
                &thread_stats,
            );
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
