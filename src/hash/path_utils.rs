// Path normalization utilities for cross-platform path handling
// Handles both forward and backward slashes in database parsing
// Provides utilities for canonicalization and relative path handling

use std::io;
use std::path::{Path, PathBuf, Component};

/// Normalize a path string by handling both forward and backward slashes
/// Converts all path separators to the platform-specific separator
pub fn normalize_path_string(path_str: &str) -> String {
    // Replace both types of separators with the platform separator
    let normalized = if cfg!(windows) {
        // On Windows, convert forward slashes to backslashes
        path_str.replace('/', "\\")
    } else {
        // On Unix-like systems, convert backslashes to forward slashes
        path_str.replace('\\', "/")
    };
    
    normalized
}

/// Parse a path from a database entry, handling mixed separators
/// Returns a PathBuf with normalized separators
pub fn parse_database_path(path_str: &str) -> PathBuf {
    let normalized = normalize_path_string(path_str);
    PathBuf::from(normalized)
}

/// Canonicalize a path if it exists, otherwise return the path as-is
/// This is useful for handling paths that may not exist yet
pub fn try_canonicalize(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        path.canonicalize()
    } else {
        // Return absolute path without resolving symlinks
        Ok(path.to_path_buf())
    }
}

/// Get a relative path from a base directory
/// If the path cannot be made relative, returns the absolute path
pub fn get_relative_path(path: &Path, base: &Path) -> io::Result<PathBuf> {
    // Canonicalize both paths for consistent comparison
    let canonical_path = path.canonicalize()?;
    let canonical_base = base.canonicalize()?;
    
    // Try to strip the base prefix
    match canonical_path.strip_prefix(&canonical_base) {
        Ok(relative) => Ok(relative.to_path_buf()),
        Err(_) => {
            // If we can't make it relative, return the canonical path
            Ok(canonical_path)
        }
    }
}

/// Get a relative path from a pre-canonicalized base directory
/// This is more efficient when the base path is reused multiple times
/// If the path cannot be made relative, returns the absolute path
pub fn get_relative_path_cached(path: &Path, canonical_base: &Path) -> io::Result<PathBuf> {
    // Only canonicalize the file path
    let canonical_path = path.canonicalize()?;
    
    // Try to strip the base prefix
    match canonical_path.strip_prefix(canonical_base) {
        Ok(relative) => Ok(relative.to_path_buf()),
        Err(_) => {
            // If we can't make it relative, return the canonical path
            Ok(canonical_path)
        }
    }
}

/// Resolve a path that may be relative or absolute
/// If relative, resolves against the provided base directory
/// If absolute, uses the path as-is
pub fn resolve_path(path: &Path, base_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

/// Clean a path by removing redundant components like "." and ".."
/// This provides a normalized form without requiring the path to exist
pub fn clean_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    
    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip "." components
                continue;
            }
            Component::ParentDir => {
                // Handle ".." by popping the last component if possible
                if !components.is_empty() {
                    let last = components.last();
                    // Only pop if the last component is not ".." or a root
                    if let Some(Component::Normal(_)) = last {
                        components.pop();
                        continue;
                    }
                }
                components.push(component);
            }
            _ => {
                components.push(component);
            }
        }
    }
    
    // Reconstruct the path from components
    let mut result = PathBuf::new();
    for component in components {
        result.push(component);
    }
    
    // If the result is empty, return current directory
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

// Tests moved to tests/hash/utility_tests.rs

