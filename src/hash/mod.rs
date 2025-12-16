// Hash Core Library
// Shared library for hash computation, scanning, verification, and comparison

pub mod hash;
pub mod scan;
pub mod verify;
pub mod benchmark;
pub mod database;
pub mod path_utils;
pub mod error;
pub mod ignore_handler;
pub mod wildcard;
pub mod compare;
pub mod dedup;

// Re-export commonly used types for convenience
pub use error::HashUtilityError;
pub use hash::{HashComputer, HashRegistry, HashResult, AlgorithmInfo, Hasher};
pub use scan::{ScanEngine, ScanStats};
pub use verify::{VerifyEngine, VerifyReport, Mismatch};
pub use benchmark::{BenchmarkEngine, BenchmarkResult, generate_test_data, calculate_throughput};
pub use database::{DatabaseHandler, DatabaseFormat, DatabaseEntry};
pub use compare::{CompareEngine, CompareReport, ChangedFile, DuplicateGroup};
pub use dedup::{DedupEngine, DedupReport, DedupStats};

