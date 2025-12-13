//! Sync & Replication Module (Phase 3)
//!
//! This module provides bidirectional synchronization, compression,
//! and smart sync strategies for file transfers.

pub mod conflict;
pub mod compression;
pub mod engine;
pub mod exclude;
pub mod hash;
pub mod throttle;
pub mod watcher;

pub use conflict::{Conflict, ConflictStrategy};
pub use compression::{CompressionType, CompressedReader, CompressedWriter};
pub use engine::{SyncEngine, SyncConfig, SyncResult, SyncStatus, SyncAction, SyncMode, SyncProgress, SyncPhase};
pub use exclude::ExcludePatterns;
pub use hash::{HashType, FileHash, hash_file, hash_bytes};
pub use throttle::BandwidthLimiter;
pub use watcher::{FileWatcher, WatchEvent};
