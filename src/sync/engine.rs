//! Sync engine for bidirectional synchronization.
//!
//! Provides the core sync logic including change detection,
//! conflict resolution, and file transfer coordination.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;

use crate::fs::{FileEntry, StorageBackend};
use crate::sync::conflict::{Conflict, ConflictResolver, ConflictResolution, ConflictStrategy, FileInfo};
use crate::sync::compression::{CompressionType, CompressionLevel};
use crate::sync::exclude::ExcludePatterns;
use crate::sync::hash::hash_bytes;
use crate::sync::throttle::{BandwidthLimiter, TransferStats};

/// Sync mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// One-way sync: source -> destination.
    #[default]
    OneWay,
    /// Bidirectional sync: changes flow both ways.
    Bidirectional,
    /// Mirror: destination matches source exactly (deletes extras).
    Mirror,
}

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Sync mode.
    pub mode: SyncMode,
    /// Conflict resolution strategy.
    pub conflict_strategy: ConflictStrategy,
    /// Exclude patterns.
    pub exclude: ExcludePatterns,
    /// Compression type for transfers.
    pub compression: CompressionType,
    /// Compression level.
    pub compression_level: CompressionLevel,
    /// Bandwidth limit.
    pub bandwidth_limit: crate::sync::throttle::BandwidthLimit,
    /// Whether this is a dry run (no actual changes).
    pub dry_run: bool,
    /// Delete files in destination that don't exist in source (mirror mode).
    pub delete_extra: bool,
    /// Verify file integrity with checksums.
    pub verify: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            mode: SyncMode::OneWay,
            conflict_strategy: ConflictStrategy::LastWriteWins,
            exclude: ExcludePatterns::with_defaults(),
            compression: CompressionType::None,
            compression_level: CompressionLevel::balanced(),
            bandwidth_limit: crate::sync::throttle::BandwidthLimit::unlimited(),
            dry_run: false,
            delete_extra: false,
            verify: false,
        }
    }
}

/// Action to take for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Copy file from source to destination.
    CopyToDestination { path: String },
    /// Copy file from destination to source (bidirectional).
    CopyToSource { path: String },
    /// Delete file from destination.
    DeleteFromDestination { path: String },
    /// Delete file from source (bidirectional).
    DeleteFromSource { path: String },
    /// Create directory in destination.
    CreateDirInDestination { path: String },
    /// Create directory in source (bidirectional).
    CreateDirInSource { path: String },
    /// Skip file (no action needed).
    Skip { path: String, reason: String },
    /// Conflict requires resolution.
    Conflict { path: String },
}

impl SyncAction {
    /// Get the path associated with this action.
    pub fn path(&self) -> &str {
        match self {
            Self::CopyToDestination { path } => path,
            Self::CopyToSource { path } => path,
            Self::DeleteFromDestination { path } => path,
            Self::DeleteFromSource { path } => path,
            Self::CreateDirInDestination { path } => path,
            Self::CreateDirInSource { path } => path,
            Self::Skip { path, .. } => path,
            Self::Conflict { path } => path,
        }
    }

    /// Check if this is a skip action.
    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }
}

/// Current sync status.
#[derive(Debug, Clone, Default)]
pub enum SyncStatus {
    /// Not syncing.
    #[default]
    Idle,
    /// Scanning for changes.
    Scanning,
    /// Comparing files.
    Comparing,
    /// Transferring files.
    Transferring { current: String, progress: f32 },
    /// Sync complete.
    Complete { stats: SyncStats },
    /// Sync failed.
    Error { message: String },
}

/// Sync statistics.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Files scanned.
    pub files_scanned: usize,
    /// Files copied.
    pub files_copied: usize,
    /// Files deleted.
    pub files_deleted: usize,
    /// Directories created.
    pub dirs_created: usize,
    /// Files skipped.
    pub files_skipped: usize,
    /// Conflicts detected.
    pub conflicts: usize,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Total duration.
    pub duration_ms: u64,
}

/// Result of a sync operation.
#[derive(Debug)]
pub struct SyncResult {
    /// Actions that were planned/executed.
    pub actions: Vec<SyncAction>,
    /// Conflicts that need resolution.
    pub conflicts: Vec<Conflict>,
    /// Statistics.
    pub stats: SyncStats,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// File information for comparison.
#[derive(Debug, Clone)]
struct FileState {
    path: String,
    size: u64,
    modified: Option<DateTime<Utc>>,
    is_dir: bool,
    hash: Option<String>,
}

impl From<&FileEntry> for FileState {
    fn from(entry: &FileEntry) -> Self {
        Self {
            path: entry.name.clone(),
            size: entry.size,
            modified: entry.modified,
            is_dir: entry.is_dir,
            hash: None,
        }
    }
}

/// Progress update for sync operations.
#[derive(Debug, Clone)]
pub struct SyncProgress {
    /// Current phase.
    pub phase: SyncPhase,
    /// Current file being processed.
    pub current_file: String,
    /// Files processed so far.
    pub files_done: usize,
    /// Total files to process.
    pub total_files: usize,
    /// Bytes transferred so far.
    pub bytes_done: u64,
    /// Total bytes to transfer.
    pub total_bytes: u64,
}

impl SyncProgress {
    /// Get progress as a percentage (0.0 - 1.0).
    pub fn percentage(&self) -> f32 {
        if self.total_files == 0 {
            return 0.0;
        }
        self.files_done as f32 / self.total_files as f32
    }
}

/// Current sync phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPhase {
    Scanning,
    Comparing,
    Transferring,
    Verifying,
    Complete,
}

/// Sync engine for orchestrating sync operations.
pub struct SyncEngine {
    /// Source backend.
    source: Arc<dyn StorageBackend>,
    /// Destination backend.
    dest: Arc<dyn StorageBackend>,
    /// Configuration.
    config: SyncConfig,
    /// Conflict resolver.
    conflict_resolver: ConflictResolver,
    /// Bandwidth limiter.
    limiter: BandwidthLimiter,
    /// Transfer statistics.
    stats: TransferStats,
    /// Progress callback.
    progress_tx: Option<tokio::sync::mpsc::Sender<SyncProgress>>,
}

impl SyncEngine {
    /// Create a new sync engine.
    pub fn new(
        source: Arc<dyn StorageBackend>,
        dest: Arc<dyn StorageBackend>,
        config: SyncConfig,
    ) -> Self {
        let limiter = BandwidthLimiter::new(config.bandwidth_limit);
        let conflict_resolver = ConflictResolver::new(config.conflict_strategy);
        
        Self {
            source,
            dest,
            config,
            conflict_resolver,
            limiter,
            stats: TransferStats::new(),
            progress_tx: None,
        }
    }

    /// Create a sync engine with progress reporting.
    pub fn with_progress(
        source: Arc<dyn StorageBackend>,
        dest: Arc<dyn StorageBackend>,
        config: SyncConfig,
        progress_tx: tokio::sync::mpsc::Sender<SyncProgress>,
    ) -> Self {
        let mut engine = Self::new(source, dest, config);
        engine.progress_tx = Some(progress_tx);
        engine
    }

    /// Send a progress update.
    async fn send_progress(&self, progress: SyncProgress) {
        if let Some(ref tx) = self.progress_tx {
            let _ = tx.send(progress).await;
        }
    }

    /// Perform a sync operation.
    pub async fn sync(&mut self, source_path: &str, dest_path: &str) -> Result<SyncResult> {
        self.stats.start();
        
        // Scan source and destination
        let source_files = self.scan_directory(&*self.source, source_path).await?;
        let dest_files = self.scan_directory(&*self.dest, dest_path).await?;
        
        // Build lookup maps
        let source_map: HashMap<&str, &FileState> = source_files
            .iter()
            .map(|f| (f.path.as_str(), f))
            .collect();
        let dest_map: HashMap<&str, &FileState> = dest_files
            .iter()
            .map(|f| (f.path.as_str(), f))
            .collect();
        
        // Determine actions
        let mut actions = Vec::new();
        let mut stats = SyncStats::default();
        
        // Files in source
        for file in &source_files {
            stats.files_scanned += 1;
            
            // Check excludes
            if self.config.exclude.is_excluded(&file.path) {
                actions.push(SyncAction::Skip {
                    path: file.path.clone(),
                    reason: "Excluded by pattern".to_string(),
                });
                stats.files_skipped += 1;
                continue;
            }
            
            match dest_map.get(file.path.as_str()) {
                Some(dest_file) => {
                    // File exists in both - check if needs update
                    if file.is_dir {
                        // Directory exists, skip
                        continue;
                    }
                    
                    if self.needs_update(file, dest_file) {
                        if self.is_conflict(file, dest_file) {
                            self.conflict_resolver.add_conflict(
                                &file.path,
                                FileInfo::new(&file.path, file.size, file.modified),
                                FileInfo::new(&dest_file.path, dest_file.size, dest_file.modified),
                            );
                            actions.push(SyncAction::Conflict { path: file.path.clone() });
                            stats.conflicts += 1;
                        } else {
                            actions.push(SyncAction::CopyToDestination { path: file.path.clone() });
                        }
                    } else {
                        actions.push(SyncAction::Skip {
                            path: file.path.clone(),
                            reason: "Already up to date".to_string(),
                        });
                        stats.files_skipped += 1;
                    }
                }
                None => {
                    // File only in source - copy to destination
                    if file.is_dir {
                        actions.push(SyncAction::CreateDirInDestination { path: file.path.clone() });
                    } else {
                        actions.push(SyncAction::CopyToDestination { path: file.path.clone() });
                    }
                }
            }
        }
        
        // Files only in destination (for mirror/bidirectional)
        if self.config.delete_extra || self.config.mode == SyncMode::Mirror {
            for file in &dest_files {
                if !source_map.contains_key(file.path.as_str()) {
                    if !self.config.exclude.is_excluded(&file.path) {
                        actions.push(SyncAction::DeleteFromDestination { path: file.path.clone() });
                    }
                }
            }
        } else if self.config.mode == SyncMode::Bidirectional {
            // In bidirectional mode, copy new dest files to source
            for file in &dest_files {
                if !source_map.contains_key(file.path.as_str()) {
                    if !self.config.exclude.is_excluded(&file.path) {
                        if file.is_dir {
                            actions.push(SyncAction::CreateDirInSource { path: file.path.clone() });
                        } else {
                            actions.push(SyncAction::CopyToSource { path: file.path.clone() });
                        }
                    }
                }
            }
        }
        
        // Resolve conflicts
        self.conflict_resolver.resolve_all();
        
        // Count total actions for progress
        let total_actions = actions.iter().filter(|a| !a.is_skip()).count();
        let mut actions_done = 0;
        
        // Execute actions if not dry run
        if !self.config.dry_run {
            for action in &actions {
                match action {
                    SyncAction::CopyToDestination { path } => {
                        // Send progress update
                        self.send_progress(SyncProgress {
                            phase: SyncPhase::Transferring,
                            current_file: path.clone(),
                            files_done: actions_done,
                            total_files: total_actions,
                            bytes_done: self.stats.bytes_transferred,
                            total_bytes: 0, // Unknown until we read files
                        }).await;
                        
                        let src_full = format!("{}/{}", source_path, path);
                        let dst_full = format!("{}/{}", dest_path, path);
                        self.copy_file(&src_full, &dst_full, true).await?;
                        stats.files_copied += 1;
                        actions_done += 1;
                    }
                    SyncAction::CopyToSource { path } => {
                        self.send_progress(SyncProgress {
                            phase: SyncPhase::Transferring,
                            current_file: path.clone(),
                            files_done: actions_done,
                            total_files: total_actions,
                            bytes_done: self.stats.bytes_transferred,
                            total_bytes: 0,
                        }).await;
                        
                        let src_full = format!("{}/{}", source_path, path);
                        let dst_full = format!("{}/{}", dest_path, path);
                        self.copy_file(&dst_full, &src_full, false).await?;
                        stats.files_copied += 1;
                        actions_done += 1;
                    }
                    SyncAction::CreateDirInDestination { path } => {
                        let dst_full = format!("{}/{}", dest_path, path);
                        self.dest.create_dir(&dst_full).await?;
                        stats.dirs_created += 1;
                        actions_done += 1;
                    }
                    SyncAction::CreateDirInSource { path } => {
                        let src_full = format!("{}/{}", source_path, path);
                        self.source.create_dir(&src_full).await?;
                        stats.dirs_created += 1;
                        actions_done += 1;
                    }
                    SyncAction::DeleteFromDestination { path } => {
                        let dst_full = format!("{}/{}", dest_path, path);
                        self.dest.delete(&dst_full).await?;
                        stats.files_deleted += 1;
                        actions_done += 1;
                    }
                    SyncAction::DeleteFromSource { path } => {
                        let src_full = format!("{}/{}", source_path, path);
                        self.source.delete(&src_full).await?;
                        stats.files_deleted += 1;
                        actions_done += 1;
                    }
                    _ => {}
                }
            }
            
            // Send completion progress
            self.send_progress(SyncProgress {
                phase: SyncPhase::Complete,
                current_file: String::new(),
                files_done: actions_done,
                total_files: total_actions,
                bytes_done: self.stats.bytes_transferred,
                total_bytes: self.stats.bytes_transferred,
            }).await;
        }
        
        self.stats.stop();
        stats.bytes_transferred = self.stats.bytes_transferred;
        stats.duration_ms = self.stats.elapsed().as_millis() as u64;
        
        // Get pending conflicts
        let conflicts: Vec<Conflict> = self.conflict_resolver.pending
            .iter()
            .cloned()
            .collect();
        
        Ok(SyncResult {
            actions,
            conflicts,
            stats,
            dry_run: self.config.dry_run,
        })
    }

    /// Perform a dry run (preview changes without applying).
    pub async fn dry_run(&mut self, source_path: &str, dest_path: &str) -> Result<SyncResult> {
        let original_dry_run = self.config.dry_run;
        self.config.dry_run = true;
        let result = self.sync(source_path, dest_path).await;
        self.config.dry_run = original_dry_run;
        result
    }

    /// Scan a directory recursively.
    async fn scan_directory(&self, backend: &dyn StorageBackend, path: &str) -> Result<Vec<FileState>> {
        let mut all_files = Vec::new();
        let mut to_scan = vec![path.to_string()];
        
        while let Some(current_path) = to_scan.pop() {
            let entries = backend.list_dir(&current_path).await?;
            
            for entry in entries {
                let relative_path = if current_path == path {
                    entry.name.clone()
                } else {
                    let prefix = current_path.strip_prefix(path).unwrap_or(&current_path);
                    let prefix = prefix.trim_start_matches('/');
                    if prefix.is_empty() {
                        entry.name.clone()
                    } else {
                        format!("{}/{}", prefix, entry.name)
                    }
                };
                
                let state = FileState {
                    path: relative_path,
                    size: entry.size,
                    modified: entry.modified,
                    is_dir: entry.is_dir,
                    hash: None,
                };
                
                if entry.is_dir {
                    let full_path = format!("{}/{}", current_path, entry.name);
                    to_scan.push(full_path);
                }
                
                all_files.push(state);
            }
        }
        
        Ok(all_files)
    }

    /// Check if a file needs to be updated.
    fn needs_update(&self, source: &FileState, dest: &FileState) -> bool {
        // Different size means update needed
        if source.size != dest.size {
            return true;
        }
        
        // Check modification time if available
        match (&source.modified, &dest.modified) {
            (Some(src_time), Some(dst_time)) => src_time > dst_time,
            _ => false, // If no timestamps, assume no update needed
        }
    }

    /// Check if there's a conflict (both files modified).
    fn is_conflict(&self, source: &FileState, dest: &FileState) -> bool {
        // In bidirectional mode, check if both were modified
        if self.config.mode != SyncMode::Bidirectional {
            return false;
        }
        
        // If sizes differ and both have recent modifications, it's a conflict
        match (&source.modified, &dest.modified) {
            (Some(src_time), Some(dst_time)) => {
                source.size != dest.size && 
                (*src_time - *dst_time).num_seconds().abs() < 60 // Within 1 minute
            }
            _ => false,
        }
    }

    /// Copy a file between backends.
    async fn copy_file(&mut self, from: &str, to: &str, source_to_dest: bool) -> Result<()> {
        let (src_backend, dst_backend) = if source_to_dest {
            (&self.source, &self.dest)
        } else {
            (&self.dest, &self.source)
        };
        
        // Read from source
        let data = src_backend.read_bytes(from).await
            .context(format!("Failed to read {}", from))?;
        
        // Apply bandwidth limiting
        self.limiter.acquire(data.len()).await;
        
        // Apply compression if configured (for network transfers)
        let transfer_data = if self.config.compression != CompressionType::None 
            && !crate::sync::compression::CompressionType::is_already_compressed(from) 
        {
            crate::sync::compression::compress(&data, self.config.compression, self.config.compression_level)?
        } else {
            data.clone()
        };
        
        // Record transfer
        self.stats.record(transfer_data.len() as u64);
        
        // Write to destination (decompress if needed)
        let write_data = if self.config.compression != CompressionType::None
            && !crate::sync::compression::CompressionType::is_already_compressed(from)
        {
            crate::sync::compression::decompress(&transfer_data, self.config.compression)?
        } else {
            transfer_data
        };
        
        dst_backend.write_bytes(to, write_data).await
            .context(format!("Failed to write {}", to))?;
        
        // Verify if configured
        if self.config.verify {
            let original_hash = hash_bytes(&data);
            let written = dst_backend.read_bytes(to).await?;
            let written_hash = hash_bytes(&written);
            
            if original_hash != written_hash {
                anyhow::bail!("Verification failed for {}: hash mismatch", to);
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_defaults() {
        let config = SyncConfig::default();
        
        assert_eq!(config.mode, SyncMode::OneWay);
        assert_eq!(config.conflict_strategy, ConflictStrategy::LastWriteWins);
        assert!(!config.dry_run);
        assert!(!config.delete_extra);
    }

    #[test]
    fn test_sync_action_path() {
        let action = SyncAction::CopyToDestination { path: "test.txt".to_string() };
        assert_eq!(action.path(), "test.txt");
        
        let skip = SyncAction::Skip { path: "skip.txt".to_string(), reason: "test".to_string() };
        assert!(skip.is_skip());
    }
}
