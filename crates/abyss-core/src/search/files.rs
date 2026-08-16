use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ignore::WalkBuilder;

use super::{RESULT_LIMIT, SearchHit, SearchRequest};

/// Walk `root` and collect entries whose name matches the query.
///
/// This is `fd`'s behaviour: a case-insensitive substring match on the file
/// name, honouring `.gitignore` unless asked otherwise.
pub fn search_files(
    request: &SearchRequest,
    cancelled: &Arc<AtomicBool>,
) -> Result<Vec<SearchHit>, String> {
    if request.query.trim().is_empty() {
        return Err("enter something to search for".to_owned());
    }
    if !request.root.is_dir() {
        return Err(format!("{} is not a directory", request.root.display()));
    }
    let needle = request.query.to_lowercase();
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
        // An unreadable subdirectory should not abort the whole search.
        let Ok(entry) = entry else {
            continue;
        };
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !name.to_lowercase().contains(&needle) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&request.root)
            .unwrap_or(entry.path());
        hits.push(SearchHit {
            path: entry.path().to_owned(),
            line: None,
            preview: relative.to_string_lossy().into_owned(),
        });
    }
    Ok(hits)
}
