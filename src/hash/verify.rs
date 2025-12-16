// Verification module
// Compares current hashes against stored database

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::database::{DatabaseHandler, DatabaseEntry};
use super::hash::HashComputer;
use super::path_utils;
use super::error::HashUtilityError;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

// Re-export HashUtilityError as VerifyError for backward compatibility
pub type VerifyError = HashUtilityError;

/// Represents a hash mismatch between expected and actual values
#[derive(Debug, Clone, serde::Serialize)]
pub struct Mismatch {
    pub path: PathBuf,
    pub expected: String,
    pub actual: String,
}

/// Progress information for verify operations
#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyProgress {
    pub files_checked: usize,
    pub current_file: String,
    pub matches: usize,
    pub mismatches: usize,
}

/// Report of verification results
#[derive(Debug, serde::Serialize)]
pub struct VerifyReport {
    pub matches: usize,
    pub mismatches: Vec<Mismatch>,
    pub missing_files: Vec<PathBuf>,
    pub new_files: Vec<PathBuf>,
}

impl VerifyReport {
    /// Display a detailed report of verification results
    pub fn display(&self) {
        // Determine overall status
        let has_issues = !self.mismatches.is_empty() 
            || !self.missing_files.is_empty() 
            || !self.new_files.is_empty();
        
        // Display clear status banner
        println!("\n================================================================");
        if has_issues {
            println!("                  FILE CHANGES DETECTED                         ");
        } else {
            println!("                       ALL GOOD                                 ");
        }
        println!("================================================================\n");
        
        // Display summary counts
        println!("Verification Summary:");
        println!("  Matches:        {}", self.matches);
        println!("  Mismatches:     {}", self.mismatches.len());
        println!("  Missing files:  {}", self.missing_files.len());
        println!("  New files:      {}", self.new_files.len());
        
        // If everything is good, show success message and return
        if !has_issues {
            println!("\nAll files match the database. No changes detected.");
            let total_checked = self.matches + self.mismatches.len();
            println!("Total files verified: {}", total_checked);
            return;
        }
        
        // Show detailed information about issues
        if !self.mismatches.is_empty() {
            println!("\n--- Files with Changed Hashes ({}) ---", self.mismatches.len());
            for mismatch in &self.mismatches {
                println!();
                println!("  File: {}", mismatch.path.display());
                println!("    Expected: {}", mismatch.expected);
                println!("    Actual:   {}", mismatch.actual);
            }
            println!("----------------------------------------------------------------");
        }
        
        if !self.missing_files.is_empty() {
            println!("\n--- Deleted Files ({}) ---", self.missing_files.len());
            println!("(in database but not in filesystem)");
            for path in &self.missing_files {
                println!("  - {}", path.display());
            }
            println!("----------------------------------------------------------------");
        }
        
        if !self.new_files.is_empty() {
            println!("\n--- New Files ({}) ---", self.new_files.len());
            println!("(in filesystem but not in database)");
            for path in &self.new_files {
                println!("  + {}", path.display());
            }
            println!("----------------------------------------------------------------");
        }
        
        // Final summary
        println!("\n================================================================");
        let total_checked = self.matches + self.mismatches.len();
        let total_in_db = total_checked + self.missing_files.len();
        let total_in_fs = total_checked + self.new_files.len();
        println!("Total files checked:      {}", total_checked);
        println!("Total files in database:  {}", total_in_db);
        println!("Total files in filesystem: {}", total_in_fs);
        println!("================================================================");
    }
}

/// Type alias for progress callback function
pub type ProgressCallback = Box<dyn Fn(VerifyProgress) + Send + Sync>;

/// Engine for verifying file integrity against a hash database
pub struct VerifyEngine {
    computer: HashComputer,
    parallel: bool,
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl VerifyEngine {
    /// Create a new VerifyEngine with parallel processing (default)
    pub fn new() -> Self {
        Self {
            computer: HashComputer::new(),
            parallel: true,
            progress_callback: None,
        }
    }
    
    /// Create a new VerifyEngine with parallel processing control
    pub fn with_parallel(parallel: bool) -> Self {
        Self {
            computer: HashComputer::new(),
            parallel,
            progress_callback: None,
        }
    }
    
    /// Set a progress callback function
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(VerifyProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(Box::new(callback)));
        self
    }
    
    /// Verify directory contents against a hash database
    /// 
    /// This function:
    /// 1. Loads the hash database from the specified file
    /// 2. Recursively scans the directory to find all files
    /// 3. Computes current hashes for files in the database
    /// 4. Classifies files as: matches, mismatches, missing, or new
    /// 5. Returns a detailed report
    pub fn verify(
        &self,
        database_path: &Path,
        directory: &Path,
    ) -> Result<VerifyReport, VerifyError> {
        // Verify database file exists
        if !database_path.exists() {
            return Err(HashUtilityError::DatabaseNotFound {
                path: database_path.to_path_buf(),
            });
        }
        
        // Verify directory exists
        if !directory.exists() || !directory.is_dir() {
            return Err(HashUtilityError::DirectoryNotFound {
                path: directory.to_path_buf(),
            });
        }
        
        // Load the hash database
        let database = DatabaseHandler::read_database(database_path)?;
        
        // Get canonical path of database file to exclude it from scan
        let database_canonical = database_path.canonicalize().ok();
        
        // Collect all files in the directory (as canonical paths), excluding the database file
        let mut current_files = self.collect_files_optimized(directory)?;
        if let Some(db_path) = &database_canonical {
            current_files.remove(db_path);
        }
        
        // Convert database paths to canonical for comparison (optimized with caching)
        let database_canonical = self.resolve_database_paths_optimized(&database, directory)?;
        
        if self.parallel {
            self.verify_parallel(database_canonical, current_files)
        } else {
            self.verify_sequential(database_canonical, current_files)
        }
    }
    
    /// Sequential verification implementation
    fn verify_sequential(
        &self,
        database_canonical: HashMap<PathBuf, DatabaseEntry>,
        current_files: HashSet<PathBuf>,
    ) -> Result<VerifyReport, VerifyError> {
        // Track results
        let mut matches = 0;
        let mut mismatches = Vec::new();
        let mut missing_files = Vec::new();
        let mut checked_files = HashSet::new();
        let mut files_checked = 0;
        
        // Create progress bar
        let pb = ProgressBar::new(database_canonical.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%) | {msg}")
                .unwrap()
                .progress_chars("=>-")
        );
        
        // Check each file in the database
        for (db_path, entry) in &database_canonical {
            checked_files.insert(db_path.clone());
            
            // Update progress bar with current file
            let file_name = db_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            pb.set_message(format!("Verifying: {}", file_name));
            
            if current_files.contains(db_path) {
                // File exists, compute current hash using the mode specified in the database
                let hash_result = if entry.fast_mode {
                    self.computer.compute_hash_fast(db_path, &entry.algorithm)
                } else {
                    self.computer.compute_hash(db_path, &entry.algorithm)
                };
                
                match hash_result {
                    Ok(result) => {
                        if result.hash == entry.hash {
                            matches += 1;
                        } else {
                            mismatches.push(Mismatch {
                                path: db_path.clone(),
                                expected: entry.hash.clone(),
                                actual: result.hash,
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to hash {}: {}", db_path.display(), e);
                    }
                }
            } else {
                // File in database but not in filesystem
                missing_files.push(db_path.clone());
            }
            
            files_checked += 1;
            
            // Emit progress event if callback is set
            if let Some(ref callback) = self.progress_callback {
                callback(VerifyProgress {
                    files_checked,
                    current_file: file_name.to_string(),
                    matches,
                    mismatches: mismatches.len(),
                });
            }
            
            pb.inc(1);
        }
        
        // Clear progress bar
        pb.finish_and_clear();
        
        // Find new files (in filesystem but not in database)
        let new_files: Vec<PathBuf> = current_files
            .iter()
            .filter(|path| !checked_files.contains(*path))
            .cloned()
            .collect();
        
        Ok(VerifyReport {
            matches,
            mismatches,
            missing_files,
            new_files,
        })
    }
    
    /// Parallel verification implementation using rayon
    fn verify_parallel(
        &self,
        database_canonical: HashMap<PathBuf, DatabaseEntry>,
        current_files: HashSet<PathBuf>,
    ) -> Result<VerifyReport, VerifyError> {
        // Thread-safe counters for progress tracking
        let matches = Arc::new(Mutex::new(0usize));
        let mismatches = Arc::new(Mutex::new(Vec::new()));
        let missing_files = Arc::new(Mutex::new(Vec::new()));
        let files_checked = Arc::new(Mutex::new(0usize));
        
        // Create progress bar
        let pb = ProgressBar::new(database_canonical.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%) | {msg}")
                .unwrap()
                .progress_chars("=>-")
        );
        
        // Clone Arc references for use in parallel closure
        let matches_clone = Arc::clone(&matches);
        let mismatches_clone = Arc::clone(&mismatches);
        let missing_files_clone = Arc::clone(&missing_files);
        let files_checked_clone = Arc::clone(&files_checked);
        let pb_clone = pb.clone();
        let progress_callback_clone = self.progress_callback.clone();
        
        // Collect database entries into a vector for parallel iteration
        let db_entries: Vec<_> = database_canonical.iter().collect();
        
        // Process all database entries in parallel
        let checked_files: Vec<PathBuf> = db_entries.par_iter().map(|(db_path, entry)| {
            // Update progress bar
            let match_count = *matches_clone.lock().unwrap();
            let mismatch_count = mismatches_clone.lock().unwrap().len();
            let missing_count = missing_files_clone.lock().unwrap().len();
            pb_clone.set_message(format!("{} OK, {} changed, {} missing", match_count, mismatch_count, missing_count));
            
            let file_name = db_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            
            if current_files.contains(*db_path) {
                // File exists, compute current hash using the mode specified in the database
                let computer = HashComputer::new();
                let hash_result = if entry.fast_mode {
                    computer.compute_hash_fast(db_path, &entry.algorithm)
                } else {
                    computer.compute_hash(db_path, &entry.algorithm)
                };
                
                match hash_result {
                    Ok(result) => {
                        if result.hash == entry.hash {
                            let mut count = matches_clone.lock().unwrap();
                            *count += 1;
                        } else {
                            let mut list = mismatches_clone.lock().unwrap();
                            list.push(Mismatch {
                                path: (*db_path).clone(),
                                expected: entry.hash.clone(),
                                actual: result.hash,
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to hash {}: {}", db_path.display(), e);
                    }
                }
            } else {
                // File in database but not in filesystem
                let mut list = missing_files_clone.lock().unwrap();
                list.push((*db_path).clone());
            }
            
            // Update checked counter
            let mut checked = files_checked_clone.lock().unwrap();
            *checked += 1;
            let current_checked = *checked;
            drop(checked);
            
            // Emit progress event if callback is set
            if let Some(ref callback) = progress_callback_clone {
                let matches = *matches_clone.lock().unwrap();
                let mismatches = mismatches_clone.lock().unwrap().len();
                
                callback(VerifyProgress {
                    files_checked: current_checked,
                    current_file: file_name.to_string(),
                    matches,
                    mismatches,
                });
            }
            
            pb_clone.inc(1);
            (*db_path).clone()
        }).collect();
        
        // Clear progress bar
        pb.finish_and_clear();
        
        // Convert checked_files to HashSet for efficient lookup
        let checked_set: HashSet<PathBuf> = checked_files.into_iter().collect();
        
        // Find new files (in filesystem but not in database)
        let new_files: Vec<PathBuf> = current_files
            .iter()
            .filter(|path| !checked_set.contains(*path))
            .cloned()
            .collect();
        
        // Extract final results from Arc<Mutex<>>
        let final_matches = *matches.lock().unwrap();
        let final_mismatches = mismatches.lock().unwrap().clone();
        let final_missing = missing_files.lock().unwrap().clone();
        
        Ok(VerifyReport {
            matches: final_matches,
            mismatches: final_mismatches,
            missing_files: final_missing,
            new_files,
        })
    }
    
    /// Optimized file collection using jwalk (same as scan)
    fn collect_files_optimized(&self, directory: &Path) -> Result<HashSet<PathBuf>, VerifyError> {
        use jwalk::WalkDir;
        
        let mut files = HashSet::new();
        
        // Use jwalk for fast parallel directory traversal (same configuration as scan)
        for entry_result in WalkDir::new(directory)
            .parallelism(jwalk::Parallelism::RayonNewPool(0))
            .skip_hidden(false)  // Don't skip hidden files
            .follow_links(false) // Don't follow symlinks to avoid loops
        {
            match entry_result {
                Ok(entry) => {
                    // Only process regular files
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    
                    let path = entry.path();
                    
                    // Canonicalize the path for consistent comparison
                    if let Ok(canonical_path) = path.canonicalize() {
                        files.insert(canonical_path);
                    }
                }
                Err(e) => {
                    // Log errors but continue processing
                    eprintln!("Warning: Error walking directory: {}", e);
                }
            }
        }
        
        Ok(files)
    }
    
    /// Legacy method for backward compatibility
    fn collect_files(&self, directory: &Path) -> Result<HashSet<PathBuf>, VerifyError> {
        self.collect_files_optimized(directory)
    }
    
    /// Optimized path resolution with caching to reduce canonicalization overhead
    fn resolve_database_paths_optimized(
        &self,
        database: &HashMap<PathBuf, DatabaseEntry>,
        base_directory: &Path,
    ) -> Result<HashMap<PathBuf, DatabaseEntry>, VerifyError> {
        let mut resolved = HashMap::new();
        let mut canonical_cache: HashMap<PathBuf, PathBuf> = HashMap::new();
        
        for (path, entry) in database {
            // Use path_utils to resolve the path properly
            let absolute_path = path_utils::resolve_path(path, base_directory);
            
            // Check cache first to avoid redundant canonicalization
            let final_path = if let Some(cached) = canonical_cache.get(&absolute_path) {
                cached.clone()
            } else {
                // Try to canonicalize if the file exists, otherwise use as-is
                let result = match path_utils::try_canonicalize(&absolute_path) {
                    Ok(canonical) => canonical,
                    Err(_) => absolute_path.clone(),
                };
                canonical_cache.insert(absolute_path, result.clone());
                result
            };
            
            resolved.insert(final_path, entry.clone());
        }
        
        Ok(resolved)
    }
    
    /// Legacy method for backward compatibility
    fn resolve_database_paths(
        &self,
        database: &HashMap<PathBuf, DatabaseEntry>,
        base_directory: &Path,
    ) -> Result<HashMap<PathBuf, DatabaseEntry>, VerifyError> {
        self.resolve_database_paths_optimized(database, base_directory)
    }
}

impl Default for VerifyEngine {
    fn default() -> Self {
        Self::new()
    }
}

// Tests moved to tests/hash/verify_tests.rs

