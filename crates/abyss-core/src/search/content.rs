use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;

use super::{RESULT_LIMIT, SearchHit, SearchRequest};

/// Longest slice of a matching line we keep for the results list.
const PREVIEW_LIMIT: usize = 200;

/// Search file contents across `root`, as ripgrep does.
///
/// The query is a regular expression, smart-case: lowercase queries match
/// case-insensitively, a query with any uppercase is taken literally.
pub fn search_contents(
    request: &SearchRequest,
    cancelled: &Arc<AtomicBool>,
) -> Result<Vec<SearchHit>, String> {
    if request.query.trim().is_empty() {
        return Err("enter something to search for".to_owned());
    }
    if !request.root.is_dir() {
        return Err(format!("{} is not a directory", request.root.display()));
    }
    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .build(&request.query)
        .map_err(|error| format!("invalid search pattern: {error}"))?;

    let mut searcher = SearcherBuilder::new()
        // Binaries produce noise rather than matches.
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();

    let mut hits = Vec::new();
    let walker = WalkBuilder::new(&request.root)
        .hidden(request.respect_ignore)
        .git_ignore(request.respect_ignore)
        .git_global(request.respect_ignore)
        .git_exclude(request.respect_ignore)
        // Honour a .gitignore wherever it sits, not only inside a
        // checkout: a file manager browses plenty of directories that
        // carry ignore rules without being repositories.
        .require_git(false)
        .parents(request.respect_ignore)
        .follow_links(false)
        .build();

    for entry in walker {
        if cancelled.load(Ordering::Relaxed) || hits.len() >= RESULT_LIMIT {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path().to_owned();
        let mut collector = Collector {
            path: &path,
            hits: &mut hits,
        };
        // A file we cannot read is skipped, not fatal.
        let _ = searcher.search_path(&matcher, &path, &mut collector);
    }
    Ok(hits)
}

/// Receives matching lines from the searcher.
struct Collector<'a> {
    path: &'a std::path::Path,
    hits: &'a mut Vec<SearchHit>,
}

impl Sink for Collector<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if self.hits.len() >= RESULT_LIMIT {
            // Returning false stops this file early.
            return Ok(false);
        }
        let text = String::from_utf8_lossy(matched.bytes());
        let trimmed = text.trim_end_matches(['\n', '\r']);
        let preview: String = trimmed.chars().take(PREVIEW_LIMIT).collect();
        self.hits.push(SearchHit {
            path: self.path.to_owned(),
            line: matched.line_number(),
            preview,
        });
        Ok(true)
    }
}
