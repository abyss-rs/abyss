// Centralized error handling module
// Provides comprehensive error types with context for all operations

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Main error type for the hash utility
/// Provides context-rich error messages with file paths and operations
#[derive(Debug)]
pub enum HashUtilityError {
    /// File system errors with context
    FileNotFound { path: PathBuf },
    DirectoryNotFound { path: PathBuf },
    PermissionDenied { path: PathBuf, operation: String },
    IoError { path: Option<PathBuf>, operation: String, source: io::Error },
    
    /// Hash computation errors
    UnsupportedAlgorithm { algorithm: String },
    HashComputationFailed { path: PathBuf, algorithm: String, reason: String },
    
    /// Database errors
    DatabaseNotFound { path: PathBuf },
    DatabaseParseError { path: PathBuf, line: usize, reason: String },
    DatabaseWriteError { path: PathBuf, reason: String },
    EmptyDatabase { path: PathBuf },
    
    /// Verification errors
    VerificationFailed { reason: String },
    
    /// CLI errors
    InvalidArguments { message: String },
    MissingRequiredArgument { argument: String },
    
    /// Benchmark errors
    BenchmarkFailed { algorithm: String, reason: String },
}

impl fmt::Display for HashUtilityError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            // File system errors
            HashUtilityError::FileNotFound { path } => {
                write!(f, "File not found: {}\n", path.display())?;
                write!(f, "Suggestion: Check that the file path is correct and the file exists")
            }
            HashUtilityError::DirectoryNotFound { path } => {
                write!(f, "Directory not found: {}\n", path.display())?;
                write!(f, "Suggestion: Check that the directory path is correct and the directory exists")
            }
            HashUtilityError::PermissionDenied { path, operation } => {
                write!(f, "Permission denied while {} file: {}\n", operation, path.display())?;
                write!(f, "Suggestion: Check file permissions or run with appropriate privileges")
            }
            HashUtilityError::IoError { path, operation, source } => {
                if let Some(p) = path {
                    write!(f, "I/O error while {} file {}: {}\n", operation, p.display(), source)?;
                } else {
                    write!(f, "I/O error while {}: {}\n", operation, source)?;
                }
                write!(f, "Suggestion: Check file permissions and disk space")
            }
            
            // Hash computation errors
            HashUtilityError::UnsupportedAlgorithm { algorithm } => {
                write!(f, "Unsupported hash algorithm: {}\n", algorithm)?;
                write!(f, "Suggestion: Use --list to see available algorithms")
            }
            HashUtilityError::HashComputationFailed { path, algorithm, reason } => {
                write!(f, "Failed to compute {} hash for {}: {}\n", algorithm, path.display(), reason)?;
                write!(f, "Suggestion: Check that the file is readable and not corrupted")
            }
            
            // Database errors
            HashUtilityError::DatabaseNotFound { path } => {
                write!(f, "Hash database file not found: {}\n", path.display())?;
                write!(f, "Suggestion: Create a database first using the 'scan' command")
            }
            HashUtilityError::DatabaseParseError { path, line, reason } => {
                write!(f, "Error parsing database {} at line {}: {}\n", path.display(), line, reason)?;
                write!(f, "Suggestion: Check that the database file format is correct (hash  filepath)")
            }
            HashUtilityError::DatabaseWriteError { path, reason } => {
                write!(f, "Failed to write to database {}: {}\n", path.display(), reason)?;
                write!(f, "Suggestion: Check disk space and write permissions")
            }
            HashUtilityError::EmptyDatabase { path } => {
                write!(f, "Database file is empty: {}\n", path.display())?;
                write!(f, "Suggestion: Ensure the database contains at least one hash entry")
            }
            
            // Verification errors
            HashUtilityError::VerificationFailed { reason } => {
                write!(f, "Verification failed: {}\n", reason)?;
                write!(f, "Suggestion: Check that the database and directory paths are correct")
            }
            
            // CLI errors
            HashUtilityError::InvalidArguments { message } => {
                write!(f, "Invalid arguments: {}\n", message)?;
                write!(f, "Suggestion: Run with --help to see usage information")
            }
            HashUtilityError::MissingRequiredArgument { argument } => {
                write!(f, "Missing required argument: {}\n", argument)?;
                write!(f, "Suggestion: Run with --help to see required arguments")
            }
            
            // Benchmark errors
            HashUtilityError::BenchmarkFailed { algorithm, reason } => {
                write!(f, "Benchmark failed for {}: {}\n", algorithm, reason)?;
                write!(f, "Suggestion: Try running the benchmark again or with a smaller data size")
            }
        }
    }
}

impl std::error::Error for HashUtilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HashUtilityError::IoError { source, .. } => Some(source),
            _ => None,
        }
    }
}

// Conversion from io::Error with context
impl HashUtilityError {
    /// Create an IoError with context about the operation and optional path
    pub fn from_io_error(err: io::Error, operation: &str, path: Option<PathBuf>) -> Self {
        // Check for specific error kinds and provide more specific errors
        match err.kind() {
            io::ErrorKind::NotFound => {
                if let Some(p) = path {
                    if operation.contains("directory") || operation.contains("scan") {
                        HashUtilityError::DirectoryNotFound { path: p }
                    } else {
                        HashUtilityError::FileNotFound { path: p }
                    }
                } else {
                    HashUtilityError::IoError {
                        path: None,
                        operation: operation.to_string(),
                        source: err,
                    }
                }
            }
            io::ErrorKind::PermissionDenied => {
                if let Some(p) = path {
                    HashUtilityError::PermissionDenied {
                        path: p,
                        operation: operation.to_string(),
                    }
                } else {
                    HashUtilityError::IoError {
                        path: None,
                        operation: operation.to_string(),
                        source: err,
                    }
                }
            }
            _ => HashUtilityError::IoError {
                path,
                operation: operation.to_string(),
                source: err,
            },
        }
    }
}

// Default From implementation for io::Error (without context)
impl From<io::Error> for HashUtilityError {
    fn from(err: io::Error) -> Self {
        HashUtilityError::from_io_error(err, "unknown operation", None)
    }
}
