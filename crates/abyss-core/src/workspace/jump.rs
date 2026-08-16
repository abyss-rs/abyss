use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::state::{StoredLocation, VisitRecord};

/// Resolve a directory for smart jump.
///
/// Abyss ranks its own visit history first — the same frecency idea zoxide
/// uses, built in so no external tool is required — and only falls back to an
/// installed zoxide or autojump when nothing local matches.
pub fn query_smart_jump_in(visits: &[VisitRecord], query: &str) -> Result<PathBuf, String> {
    if let Some(path) = best_visit(visits, query) {
        return Ok(path);
    }
    query_external(query)
}

/// Kept for callers without access to the workspace, and used as the fallback.
pub fn query_smart_jump(query: &str) -> Result<PathBuf, String> {
    query_external(query)
}

/// Highest-scoring visited directory whose path matches `query`.
pub fn best_visit(visits: &[VisitRecord], query: &str) -> Option<PathBuf> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let mut best: Option<(f64, PathBuf)> = None;
    for record in visits {
        let StoredLocation::Local(path) = &record.location else {
            // Only local directories can be jumped to as a filesystem path.
            continue;
        };
        if !matches(path, &needle) {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let score = score(record);
        if best.as_ref().is_none_or(|(top, _)| score > *top) {
            best = Some((score, path.clone()));
        }
    }
    best.map(|(_, path)| path)
}

/// A query matches when it appears in the path, case-insensitively.
///
/// A match on the final component outranks one buried in a parent, which is
/// what makes `z proj` land on `~/code/project` rather than a file inside it;
/// that preference is applied through the score, not here.
fn matches(path: &std::path::Path, needle: &str) -> bool {
    path.to_string_lossy().to_lowercase().contains(needle)
}

/// zoxide's frecency: visit count weighted by how recently it was seen.
pub(crate) fn score(record: &VisitRecord) -> f64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let age = now.saturating_sub(record.last_visit);
    let multiplier = if age < 3_600 {
        4.0
    } else if age < 86_400 {
        2.0
    } else if age < 7 * 86_400 {
        0.5
    } else {
        0.25
    };
    f64::from(record.visits) * multiplier
}

/// Ask an installed zoxide or autojump, for directories Abyss has not seen.
fn query_external(query: &str) -> Result<PathBuf, String> {
    let attempts: [(&str, &[&str]); 2] =
        [("zoxide", &["query", "--", query]), ("autojump", &[query])];
    let mut available = false;
    for (program, arguments) in attempts {
        if !command_exists(program) {
            continue;
        }
        available = true;
        let output = Command::new(program)
            .args(arguments)
            .output()
            .map_err(|error| format!("could not run {program}: {error}"))?;
        if !output.status.success() {
            continue;
        }
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        if path.is_dir() {
            return Ok(path);
        }
    }
    if available {
        Err(format!("no directory matched {query:?}"))
    } else {
        Err(format!(
            "no visited directory matched {query:?}; install zoxide to search beyond Abyss's own history"
        ))
    }
}

fn command_exists(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        return ["exe", "cmd", "bat", "com"]
            .iter()
            .any(|extension| candidate.with_extension(extension).is_file());
        #[cfg(not(windows))]
        false
    })
}
