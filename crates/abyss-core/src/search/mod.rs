//! In-process file and content search.
//!
//! Backed by the same libraries the familiar tools are built on — `ignore` for
//! `fd`-style traversal and ripgrep's `grep-*` crates for content search — so
//! Abyss needs neither installed.

mod content;
mod files;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub use self::content::search_contents;
pub use self::files::search_files;

/// Results are capped so a search of a huge tree cannot exhaust memory or
/// produce a list no one could read.
pub const RESULT_LIMIT: usize = 5_000;

/// One search result: a path, and for content searches the matching line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 1-based line number, for content matches.
    pub line: Option<u64>,
    /// The matching line, or the file name for a filename match.
    pub preview: String,
}

/// What a search is looking for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchKind {
    /// Match against file names, like `fd`.
    Files,
    /// Match against file contents, like `rg`.
    Contents,
}

#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub root: PathBuf,
    pub query: String,
    pub kind: SearchKind,
    /// Honour `.gitignore` and skip hidden files, as both tools do by default.
    pub respect_ignore: bool,
}

/// A search running on a background thread.
pub struct SearchLoad {
    receiver: Receiver<Result<Vec<SearchHit>, String>>,
    cancelled: Arc<AtomicBool>,
    pub request: SearchRequest,
}

impl SearchLoad {
    pub fn start(request: SearchRequest) -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread_request = request.clone();
        let thread_cancelled = Arc::clone(&cancelled);
        thread::spawn(move || {
            let result = match thread_request.kind {
                SearchKind::Files => search_files(&thread_request, &thread_cancelled),
                SearchKind::Contents => search_contents(&thread_request, &thread_cancelled),
            };
            let _ = sender.send(result);
        });
        Self {
            receiver,
            cancelled,
            request,
        }
    }

    pub fn try_recv(&self) -> Option<Result<Vec<SearchHit>, String>> {
        self.receiver.try_recv().ok()
    }

    /// Ask the worker to stop; it checks between entries.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for SearchLoad {
    fn drop(&mut self) {
        self.cancel();
    }
}
