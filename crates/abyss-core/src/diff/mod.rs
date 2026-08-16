//! Unified text diffing between two files.
//!
//! Uses the `similar` crate for the diff algorithm, so Abyss produces the
//! side-by-side comparison `delta` is used for without needing it installed.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use similar::{ChangeTag, TextDiff};

/// Files larger than this are refused: a diff of two huge files is neither
/// fast nor readable.
const MAX_DIFF_BYTES: u64 = 8 * 1024 * 1024;

/// Unchanged lines kept either side of a change.
const CONTEXT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffTag {
    Context,
    Insert,
    Delete,
    /// Marks a jump over unchanged lines, as `@@` does in a unified diff.
    Separator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    pub tag: DiffTag,
    /// Line number in the left file, when the line exists there.
    pub left: Option<usize>,
    /// Line number in the right file, when the line exists there.
    pub right: Option<usize>,
    pub text: String,
}

/// Summary counts for the header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffStats {
    pub inserted: usize,
    pub deleted: usize,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub lines: Vec<DiffLine>,
    pub stats: DiffStats,
    /// True when the two files are byte-identical.
    pub identical: bool,
}

/// Diff two text files, returning grouped changes with surrounding context.
pub fn diff_files(left: &Path, right: &Path) -> Result<FileDiff, String> {
    let left_text = read_text(left)?;
    let right_text = read_text(right)?;
    Ok(diff_text(&left_text, &right_text))
}

fn read_text(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }
    if metadata.len() > MAX_DIFF_BYTES {
        return Err(format!(
            "{} is too large to diff ({} MiB limit)",
            path.display(),
            MAX_DIFF_BYTES / (1024 * 1024)
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not a text file", path.display()))
}

/// The diffing itself, split out so it can be tested without touching disk.
pub fn diff_text(left: &str, right: &str) -> FileDiff {
    let diff = TextDiff::from_lines(left, right);
    let mut lines = Vec::new();
    let mut stats = DiffStats::default();

    let groups = diff.grouped_ops(CONTEXT);
    for (index, group) in groups.iter().enumerate() {
        if index > 0 {
            // Unchanged lines were skipped between the previous group and this
            // one; say so rather than running them together.
            lines.push(DiffLine {
                tag: DiffTag::Separator,
                left: None,
                right: None,
                text: "⋯".to_owned(),
            });
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let tag = match change.tag() {
                    ChangeTag::Equal => DiffTag::Context,
                    ChangeTag::Insert => DiffTag::Insert,
                    ChangeTag::Delete => DiffTag::Delete,
                };
                match tag {
                    DiffTag::Insert => stats.inserted += 1,
                    DiffTag::Delete => stats.deleted += 1,
                    _ => {}
                }
                let text = change.value().to_owned();
                lines.push(DiffLine {
                    tag,
                    left: change.old_index().map(|index| index + 1),
                    right: change.new_index().map(|index| index + 1),
                    text: text.trim_end_matches(['\n', '\r']).to_owned(),
                });
            }
        }
    }

    FileDiff {
        identical: lines.is_empty(),
        lines,
        stats,
    }
}
