//! Cleaner module - High-performance folder cleaner for development temp files
//!
//! This module provides parallel scanning and deletion of common development
//! temporary files and folders like .terraform, target, node_modules, __pycache__, etc.

pub mod config;
pub mod deleter;
pub mod patterns;
pub mod scanner;
pub mod stats;
pub mod tree;

pub use config::Config;
pub use deleter::Deleter;
pub use patterns::PatternMatcher;
pub use scanner::{ScanResult, Scanner};
pub use stats::Stats;
pub use tree::{DirEntry, DirTree, ScanProgress};
