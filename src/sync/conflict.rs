//! Conflict detection and resolution strategies for sync operations.

use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// Strategy for resolving file conflicts during sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictStrategy {
    /// Use the file with the most recent modification time (default).
    #[default]
    LastWriteWins,
    /// Always use the file with newer mtime, regardless of source/dest.
    NewerWins,
    /// Always prefer the source file.
    SourceWins,
    /// Always prefer the destination file.
    DestWins,
    /// Keep both files, renaming the conflicting one with a suffix.
    KeepBoth,
    /// Skip conflicting files entirely.
    Skip,
    /// Queue conflicts for manual user resolution.
    Manual,
}

impl ConflictStrategy {
    /// Get a human-readable description of the strategy.
    pub fn description(&self) -> &'static str {
        match self {
            Self::LastWriteWins => "Use most recently modified file",
            Self::NewerWins => "Use newer file",
            Self::SourceWins => "Always use source",
            Self::DestWins => "Always use destination",
            Self::KeepBoth => "Keep both (rename conflict)",
            Self::Skip => "Skip conflicting files",
            Self::Manual => "Ask user for each conflict",
        }
    }
}

/// Information about a file for conflict comparison.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub hash: Option<String>,
}

impl FileInfo {
    pub fn new(path: impl Into<String>, size: u64, modified: Option<DateTime<Utc>>) -> Self {
        Self {
            path: path.into(),
            size,
            modified,
            hash: None,
        }
    }

    pub fn with_hash(mut self, hash: String) -> Self {
        self.hash = Some(hash);
        self
    }
}

/// Represents a detected conflict between source and destination.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Relative path of the conflicting file.
    pub path: String,
    /// Information about the source file.
    pub source: FileInfo,
    /// Information about the destination file.
    pub dest: FileInfo,
    /// The resolution strategy to apply.
    pub strategy: ConflictStrategy,
    /// Whether this conflict has been resolved.
    pub resolved: bool,
    /// Resolution result (if resolved).
    pub resolution: Option<ConflictResolution>,
}

/// The result of resolving a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Use the source file.
    UseSource,
    /// Use the destination file.
    UseDest,
    /// Keep both files.
    KeepBoth,
    /// Skip this file.
    Skip,
}

impl Conflict {
    /// Create a new conflict.
    pub fn new(path: impl Into<String>, source: FileInfo, dest: FileInfo) -> Self {
        Self {
            path: path.into(),
            source,
            dest,
            strategy: ConflictStrategy::default(),
            resolved: false,
            resolution: None,
        }
    }

    /// Set the resolution strategy.
    pub fn with_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Resolve this conflict using the configured strategy.
    pub fn resolve(&mut self) -> ConflictResolution {
        let resolution = match self.strategy {
            ConflictStrategy::LastWriteWins => {
                match (&self.source.modified, &self.dest.modified) {
                    (Some(src_time), Some(dst_time)) => {
                        if src_time >= dst_time {
                            ConflictResolution::UseSource
                        } else {
                            ConflictResolution::UseDest
                        }
                    }
                    (Some(_), None) => ConflictResolution::UseSource,
                    (None, Some(_)) => ConflictResolution::UseDest,
                    (None, None) => ConflictResolution::UseSource, // Default to source
                }
            }
            ConflictStrategy::NewerWins => {
                match (&self.source.modified, &self.dest.modified) {
                    (Some(src_time), Some(dst_time)) => {
                        if src_time > dst_time {
                            ConflictResolution::UseSource
                        } else {
                            ConflictResolution::UseDest
                        }
                    }
                    (Some(_), None) => ConflictResolution::UseSource,
                    (None, Some(_)) => ConflictResolution::UseDest,
                    (None, None) => ConflictResolution::UseSource,
                }
            }
            ConflictStrategy::SourceWins => ConflictResolution::UseSource,
            ConflictStrategy::DestWins => ConflictResolution::UseDest,
            ConflictStrategy::KeepBoth => ConflictResolution::KeepBoth,
            ConflictStrategy::Skip => ConflictResolution::Skip,
            ConflictStrategy::Manual => {
                // For manual, we return Skip until user decides
                ConflictResolution::Skip
            }
        };

        self.resolved = true;
        self.resolution = Some(resolution);
        resolution
    }

    /// Generate a conflict-renamed path (e.g., file.txt -> file.conflict-1.txt)
    pub fn conflict_path(&self, suffix: u32) -> String {
        let path = PathBuf::from(&self.path);
        let stem = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let parent = path.parent().map(|p| p.to_string_lossy()).unwrap_or_default();
        
        if parent.is_empty() {
            format!("{}.conflict-{}{}", stem, suffix, ext)
        } else {
            format!("{}/{}.conflict-{}{}", parent, stem, suffix, ext)
        }
    }
}

/// Conflict resolver that tracks and resolves multiple conflicts.
#[derive(Debug, Default)]
pub struct ConflictResolver {
    /// Default strategy for new conflicts.
    pub default_strategy: ConflictStrategy,
    /// Pending conflicts awaiting resolution.
    pub pending: Vec<Conflict>,
    /// Resolved conflicts.
    pub resolved: Vec<Conflict>,
}

impl ConflictResolver {
    /// Create a new resolver with the default strategy.
    pub fn new(strategy: ConflictStrategy) -> Self {
        Self {
            default_strategy: strategy,
            pending: Vec::new(),
            resolved: Vec::new(),
        }
    }

    /// Add a new conflict.
    pub fn add_conflict(&mut self, path: impl Into<String>, source: FileInfo, dest: FileInfo) {
        let conflict = Conflict::new(path, source, dest).with_strategy(self.default_strategy);
        self.pending.push(conflict);
    }

    /// Resolve all pending conflicts using their configured strategies.
    pub fn resolve_all(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for mut conflict in pending {
            if conflict.strategy != ConflictStrategy::Manual {
                conflict.resolve();
            }
            self.resolved.push(conflict);
        }
    }

    /// Get pending conflicts that require manual resolution.
    pub fn manual_conflicts(&self) -> Vec<&Conflict> {
        self.pending
            .iter()
            .filter(|c| c.strategy == ConflictStrategy::Manual)
            .collect()
    }

    /// Resolve a specific conflict manually.
    pub fn resolve_manual(&mut self, path: &str, resolution: ConflictResolution) {
        if let Some(idx) = self.pending.iter().position(|c| c.path == path) {
            let mut conflict = self.pending.remove(idx);
            conflict.resolved = true;
            conflict.resolution = Some(resolution);
            self.resolved.push(conflict);
        }
    }

    /// Get statistics about conflicts.
    pub fn stats(&self) -> ConflictStats {
        let mut stats = ConflictStats::default();
        
        for conflict in &self.resolved {
            match conflict.resolution {
                Some(ConflictResolution::UseSource) => stats.used_source += 1,
                Some(ConflictResolution::UseDest) => stats.used_dest += 1,
                Some(ConflictResolution::KeepBoth) => stats.kept_both += 1,
                Some(ConflictResolution::Skip) => stats.skipped += 1,
                None => stats.pending += 1,
            }
        }
        stats.pending += self.pending.len();
        
        stats
    }
}

/// Statistics about conflict resolution.
#[derive(Debug, Default)]
pub struct ConflictStats {
    pub used_source: usize,
    pub used_dest: usize,
    pub kept_both: usize,
    pub skipped: usize,
    pub pending: usize,
}

impl ConflictStats {
    pub fn total(&self) -> usize {
        self.used_source + self.used_dest + self.kept_both + self.skipped + self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_last_write_wins_source_newer() {
        let source = FileInfo::new("test.txt", 100, Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap()));
        let dest = FileInfo::new("test.txt", 100, Some(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()));
        
        let mut conflict = Conflict::new("test.txt", source, dest)
            .with_strategy(ConflictStrategy::LastWriteWins);
        
        assert_eq!(conflict.resolve(), ConflictResolution::UseSource);
    }

    #[test]
    fn test_last_write_wins_dest_newer() {
        let source = FileInfo::new("test.txt", 100, Some(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()));
        let dest = FileInfo::new("test.txt", 100, Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap()));
        
        let mut conflict = Conflict::new("test.txt", source, dest)
            .with_strategy(ConflictStrategy::LastWriteWins);
        
        assert_eq!(conflict.resolve(), ConflictResolution::UseDest);
    }

    #[test]
    fn test_conflict_path_generation() {
        let source = FileInfo::new("dir/file.txt", 100, None);
        let dest = FileInfo::new("dir/file.txt", 100, None);
        let conflict = Conflict::new("dir/file.txt", source, dest);
        
        assert_eq!(conflict.conflict_path(1), "dir/file.conflict-1.txt");
        assert_eq!(conflict.conflict_path(2), "dir/file.conflict-2.txt");
    }
}
