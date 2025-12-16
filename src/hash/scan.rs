// Directory scanning module
// Handles recursive directory traversal and hash computation

use super::hash::HashComputer;
use super::database::DatabaseHandler;
use super::path_utils;
use super::error::HashUtilityError;
use super::ignore_handler::IgnoreHandler;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::thread;
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use crossbeam_channel::{bounded, Sender};
use jwalk::WalkDir;

// Re-export HashUtilityError as ScanError for backward compatibility
pub type ScanError = HashUtilityError;

/// Statistics collected during a directory scan
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanStats {
    pub files_processed: usize,
    pub files_failed: usize,
    pub total_bytes: u64,
    #[serde(serialize_with = "serialize_duration")]
    pub duration: Duration,
}

/// Progress information for scan operations
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanProgress {
    pub files_processed: usize,
    pub current_file: String,
    pub bytes_processed: u64,
    pub throughput_mbps: f64,
}

// Helper function to serialize Duration as seconds
fn serialize_duration<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_f64(duration.as_secs_f64())
}

use super::database::DatabaseFormat;

/// Type alias for progress callback function
pub type ProgressCallback = Box<dyn Fn(ScanProgress) + Send + Sync>;

/// Engine for scanning directories and generating hash databases
pub struct ScanEngine {
    computer: HashComputer,
    parallel: bool,
    fast_mode: bool,
    use_ignore: bool,
    format: DatabaseFormat,
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl ScanEngine {
    /// Create a new ScanEngine with default settings
    pub fn new() -> Self {
        Self {
            computer: HashComputer::new(),
            parallel: false,
            fast_mode: false,
            use_ignore: true,
            format: DatabaseFormat::Standard,
            progress_callback: None,
        }
    }
    
    /// Create a new ScanEngine with parallel processing enabled
    pub fn with_parallel(parallel: bool) -> Self {
        Self {
            computer: HashComputer::new(),
            parallel,
            fast_mode: false,
            use_ignore: true,
            format: DatabaseFormat::Standard,
            progress_callback: None,
        }
    }
    
    /// Enable or disable fast mode for large file hashing
    pub fn with_fast_mode(mut self, fast_mode: bool) -> Self {
        self.fast_mode = fast_mode;
        self
    }
    
    /// Enable or disable .hashignore file support
    pub fn with_ignore(mut self, use_ignore: bool) -> Self {
        self.use_ignore = use_ignore;
        self
    }
    
    /// Set the output format
    pub fn with_format(mut self, format: DatabaseFormat) -> Self {
        self.format = format;
        self
    }
    
    /// Set a progress callback function
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(ScanProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(Box::new(callback)));
        self
    }
    
    /// Scan a directory recursively and write hash database to output file
    /// 
    /// # Arguments
    /// * `root` - Root directory to scan
    /// * `algorithm` - Hash algorithm to use
    /// * `output` - Output file path for hash database
    /// 
    /// # Returns
    /// Statistics about the scan operation
    pub fn scan_directory(
        &self,
        root: &Path,
        algorithm: &str,
        output: &Path,
    ) -> Result<ScanStats, ScanError> {
        let start_time = Instant::now();
        
        // Canonicalize root directory for consistent path handling
        let canonical_root = root.canonicalize().map_err(|e| {
            HashUtilityError::from_io_error(e, "scanning directory", Some(root.to_path_buf()))
        })?;
        
        // Get absolute path of output file to exclude it from scan
        // We need to get the absolute path before the file exists
        let output_absolute = if output.is_absolute() {
            output.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(output))
                .unwrap_or_else(|_| output.to_path_buf())
        };
        
        // Collect all files in the directory tree (only for sequential mode)
        println!("Scanning directory: {}", root.display());
        let files = if !self.parallel {
            self.collect_files_with_exclusion(root, Some(&output_absolute))?
        } else {
            // For parallel mode, we don't pre-collect files
            Vec::new()
        };
        
        if !self.parallel {
            println!("Found {} files to process", files.len());
        }
        
        if self.fast_mode {
            println!("Fast mode enabled: sampling first, middle, and last 100MB of large files");
        }
        
        if self.parallel {
            self.scan_parallel(&files, algorithm, output, &canonical_root, &output_absolute, start_time)
        } else {
            self.scan_sequential(&files, algorithm, output, &canonical_root, start_time)
        }
    }
    
    /// Sequential scan implementation
    fn scan_sequential(
        &self,
        files: &[PathBuf],
        algorithm: &str,
        output: &Path,
        canonical_root: &Path,
        start_time: Instant,
    ) -> Result<ScanStats, ScanError> {
        // Open output file for writing
        let output_file = File::create(output).map_err(|e| {
            HashUtilityError::from_io_error(e, "creating output file", Some(output.to_path_buf()))
        })?;
        let mut writer = BufWriter::new(output_file);
        
        // Write hashdeep header if using hashdeep format
        if self.format == DatabaseFormat::Hashdeep {
            DatabaseHandler::write_hashdeep_header(&mut writer, &[algorithm.to_string()])
                .map_err(|e| {
                    HashUtilityError::from_io_error(e, "writing hashdeep header", Some(output.to_path_buf()))
                })?;
        }
        
        // Track statistics
        let mut files_processed = 0;
        let mut files_failed = 0;
        let mut files_skipped = 0;
        let mut total_bytes = 0u64;
        
        // Create progress bar
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%) | Processed: {msg}")
                .unwrap()
                .progress_chars("=>-")
        );
        
        // Process each file
        for file_path in files.iter() {
            // Update progress bar with counts instead of filename to avoid encoding issues
            pb.set_message(format!("{} OK, {} failed, {} skipped", files_processed, files_failed, files_skipped));
            
            // Check if file still exists and is accessible before processing
            let metadata_check = fs::metadata(file_path);
            if metadata_check.is_err() {
                files_skipped += 1;
                pb.inc(1);
                continue;
            }
            
            // Compute hash for the file (using fast mode if enabled)
            let hash_result = if self.fast_mode {
                self.computer.compute_hash_fast(file_path, algorithm)
            } else {
                self.computer.compute_hash(file_path, algorithm)
            };
            
            match hash_result {
                Ok(result) => {
                    // Try to get relative path for cleaner database entries
                    // Use cached version since canonical_root is already canonicalized
                    let path_to_write = match path_utils::get_relative_path_cached(file_path, canonical_root) {
                        Ok(rel_path) => rel_path,
                        Err(_) => file_path.clone(),
                    };
                    
                    // Get file size for hashdeep format
                    let file_size = fs::metadata(file_path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    
                    // Write hash entry to database with metadata
                    let write_result = match self.format {
                        DatabaseFormat::Standard => {
                            DatabaseHandler::write_entry(
                                &mut writer,
                                &result.hash,
                                algorithm,
                                self.fast_mode,
                                &path_to_write,
                            )
                        }
                        DatabaseFormat::Hashdeep => {
                            DatabaseHandler::write_hashdeep_entry(
                                &mut writer,
                                file_size,
                                &[result.hash.clone()],
                                &path_to_write,
                            )
                        }
                    };
                    
                    if let Err(e) = write_result {
                        eprintln!("Warning: Failed to write entry for {}: {}", 
                            file_path.display(), e);
                        files_failed += 1;
                    } else {
                        files_processed += 1;
                        total_bytes += file_size;
                        
                        // Emit progress event if callback is set
                        if let Some(ref callback) = self.progress_callback {
                            let elapsed = start_time.elapsed().as_secs_f64();
                            let throughput_mbps = if elapsed > 0.0 {
                                (total_bytes as f64 / 1_048_576.0) / elapsed
                            } else {
                                0.0
                            };
                            
                            callback(ScanProgress {
                                files_processed,
                                current_file: file_path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                bytes_processed: total_bytes,
                                throughput_mbps,
                            });
                        }
                    }
                }
                Err(e) => {
                    // Log error but continue processing
                    eprintln!("Warning: Failed to hash {}: {}", file_path.display(), e);
                    files_failed += 1;
                }
            }
            
            pb.inc(1);
        }
        
        let duration = start_time.elapsed();
        
        // Clear progress bar and display summary
        pb.finish_and_clear();
        
        println!("\nScan complete!");
        println!("Files processed: {}", files_processed);
        println!("Files failed: {}", files_failed);
        println!("Files skipped: {}", files_skipped);
        println!("Total bytes: {} ({:.2} MB)", total_bytes, total_bytes as f64 / 1_048_576.0);
        println!("Duration: {:.2}s", duration.as_secs_f64());
        
        // Calculate and display throughput
        if duration.as_secs_f64() > 0.0 {
            let throughput_mbps = (total_bytes as f64 / 1_048_576.0) / duration.as_secs_f64();
            println!("Throughput: {:.2} MB/s", throughput_mbps);
        }
        
        println!("Output written to: {}", output.display());
        
        Ok(ScanStats {
            files_processed,
            files_failed: files_failed + files_skipped,
            total_bytes,
            duration,
        })
    }
    
    /// Parallel scan implementation using producer-consumer pattern with jwalk and crossbeam-channel
    fn scan_parallel(
        &self,
        _files: &[PathBuf],
        algorithm: &str,
        output: &Path,
        canonical_root: &Path,
        output_absolute: &Path,
        start_time: Instant,
    ) -> Result<ScanStats, ScanError> {
        // Thread-safe counters for progress tracking
        let files_processed = Arc::new(Mutex::new(0usize));
        let files_failed = Arc::new(Mutex::new(0usize));
        let files_skipped = Arc::new(Mutex::new(0usize));
        let total_bytes = Arc::new(Mutex::new(0u64));
        
        // Create progress bar (we'll update the style once discovery is complete)
        let pb = ProgressBar::new(0);
        // Start with "Counting..." style
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] Counting... {pos} files found | Processing: {msg}")
                .unwrap()
                .progress_chars("=>-")
        );
        
        // Create bounded channel with backpressure (buffer size: 10000 entries)
        // Larger buffer helps with very large directory scans
        let (sender, receiver) = bounded::<PathBuf>(10000);
        
        // Track total files discovered
        let total_files_discovered = Arc::new(Mutex::new(0usize));
        let discovery_complete = Arc::new(Mutex::new(false));
        
        // Capture fast_mode for use in closure
        let fast_mode = self.fast_mode;
        
        // Clone canonical_root and output_absolute for the walker thread
        let walker_root = canonical_root.to_path_buf();
        let use_ignore = self.use_ignore;
        let output_to_exclude = output_absolute.to_path_buf();
        
        // Clone for walker thread
        let total_files_discovered_walker = Arc::clone(&total_files_discovered);
        let discovery_complete_walker = Arc::clone(&discovery_complete);
        let pb_walker = pb.clone();
        
        // Spawn walker thread using jwalk to traverse directories
        let walker_handle = thread::spawn(move || {
            let result = Self::walk_directory_streaming(&walker_root, sender, use_ignore, Some(&output_to_exclude), Arc::clone(&total_files_discovered_walker));
            
            // Mark discovery as complete and update progress bar with total and new style
            let total = *total_files_discovered_walker.lock().unwrap();
            pb_walker.set_length(total as u64);
            pb_walker.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%) | Processed: {msg}")
                    .unwrap()
                    .progress_chars("=>-")
            );
            *discovery_complete_walker.lock().unwrap() = true;
            
            result
        });
        
        // Clone Arc references for use in parallel closure
        let files_processed_clone = Arc::clone(&files_processed);
        let files_failed_clone = Arc::clone(&files_failed);
        let files_skipped_clone = Arc::clone(&files_skipped);
        let total_bytes_clone = Arc::clone(&total_bytes);
        let pb_clone = pb.clone();
        let canonical_root_clone = canonical_root.to_path_buf();
        let progress_callback_clone = self.progress_callback.clone();
        let start_time_clone = start_time;
        
        // Use rayon's par_bridge to consume from channel in parallel
        // This starts hashing immediately as files are discovered
        let results: Vec<_> = receiver
            .into_iter()
            .par_bridge()
            .filter_map(|file_path| {
                // Check if file still exists and is accessible before processing
                let metadata_check = fs::metadata(&file_path);
                if metadata_check.is_err() {
                    let mut skipped = files_skipped_clone.lock().unwrap();
                    *skipped += 1;
                    pb_clone.inc(1);
                    return None;
                }
                
                // Update progress bar with counts instead of filename to avoid encoding issues
                let processed = files_processed_clone.lock().unwrap();
                let failed = files_failed_clone.lock().unwrap();
                let skipped = files_skipped_clone.lock().unwrap();
                pb_clone.set_message(format!("{} OK, {} failed, {} skipped", *processed, *failed, *skipped));
                drop(processed);
                drop(failed);
                drop(skipped);
                
                // Compute hash for the file (using fast mode if enabled)
                let computer = HashComputer::new();
                let hash_result = if fast_mode {
                    computer.compute_hash_fast(&file_path, algorithm)
                } else {
                    computer.compute_hash(&file_path, algorithm)
                };
                
                let result = match hash_result {
                    Ok(result) => {
                        // Try to get relative path for cleaner database entries
                        // Use cached version since canonical_root_clone is already canonicalized
                        let path_to_write = match path_utils::get_relative_path_cached(&file_path, &canonical_root_clone) {
                            Ok(rel_path) => rel_path,
                            Err(_) => file_path.clone(),
                        };
                        
                        // Track file size
                        if let Ok(metadata) = fs::metadata(&file_path) {
                            let size = metadata.len();
                            let mut bytes = total_bytes_clone.lock().unwrap();
                            *bytes += size;
                        }
                        
                        // Update success counter
                        let mut processed = files_processed_clone.lock().unwrap();
                        *processed += 1;
                        let current_processed = *processed;
                        drop(processed);
                        
                        // Emit progress event if callback is set
                        if let Some(ref callback) = progress_callback_clone {
                            let bytes = *total_bytes_clone.lock().unwrap();
                            let elapsed = start_time_clone.elapsed().as_secs_f64();
                            let throughput_mbps = if elapsed > 0.0 {
                                (bytes as f64 / 1_048_576.0) / elapsed
                            } else {
                                0.0
                            };
                            
                            callback(ScanProgress {
                                files_processed: current_processed,
                                current_file: file_path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                bytes_processed: bytes,
                                throughput_mbps,
                            });
                        }
                        
                        Some((result.hash, path_to_write))
                    }
                    Err(e) => {
                        // Log error but continue processing
                        eprintln!("Warning: Failed to hash {}: {}", file_path.display(), e);
                        
                        // Update failure counter
                        let mut failed = files_failed_clone.lock().unwrap();
                        *failed += 1;
                        
                        None
                    }
                };
                
                pb_clone.inc(1);
                result
            })
            .collect();
        
        // Wait for walker thread to complete
        // Note: The walker thread should already be done since we consumed all items from the channel
        match walker_handle.join() {
            Ok(walk_result) => {
                if let Err(e) = walk_result {
                    eprintln!("Warning: Walker thread encountered error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Warning: Walker thread panicked: {:?}", e);
            }
        }
        
        let duration = start_time.elapsed();
        
        // Clear progress bar
        pb.finish_and_clear();
        
        // Write all results to output file
        let output_file = File::create(output).map_err(|e| {
            HashUtilityError::from_io_error(e, "creating output file", Some(output.to_path_buf()))
        })?;
        let mut writer = BufWriter::new(output_file);
        
        // Write hashdeep header if using hashdeep format
        if self.format == DatabaseFormat::Hashdeep {
            if let Err(e) = DatabaseHandler::write_hashdeep_header(&mut writer, &[algorithm.to_string()]) {
                eprintln!("Warning: Failed to write hashdeep header: {}", e);
            }
        }
        
        for result in results.iter() {
            let write_result = match self.format {
                DatabaseFormat::Standard => {
                    DatabaseHandler::write_entry(
                        &mut writer,
                        &result.0,
                        algorithm,
                        fast_mode,
                        &result.1,
                    )
                }
                DatabaseFormat::Hashdeep => {
                    // Get file size
                    let file_size = fs::metadata(&result.1)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    DatabaseHandler::write_hashdeep_entry(
                        &mut writer,
                        file_size,
                        &[result.0.clone()],
                        &result.1,
                    )
                }
            };
            
            if let Err(e) = write_result {
                eprintln!("Warning: Failed to write entry: {}", e);
            }
        }
        
        // Flush the writer to ensure all data is written
        writer.flush().map_err(|e| {
            HashUtilityError::from_io_error(e, "flushing output file", Some(output.to_path_buf()))
        })?;
        
        // Extract final statistics
        let final_processed = *files_processed.lock().unwrap();
        let final_failed = *files_failed.lock().unwrap();
        let final_skipped = *files_skipped.lock().unwrap();
        let final_bytes = *total_bytes.lock().unwrap();
        
        // Display summary
        println!("\nScan complete!");
        println!("Files processed: {}", final_processed);
        println!("Files failed: {}", final_failed);
        println!("Files skipped: {}", final_skipped);
        println!("Total bytes: {} ({:.2} MB)", final_bytes, final_bytes as f64 / 1_048_576.0);
        println!("Duration: {:.2}s", duration.as_secs_f64());
        
        // Calculate and display throughput
        if duration.as_secs_f64() > 0.0 {
            let throughput_mbps = (final_bytes as f64 / 1_048_576.0) / duration.as_secs_f64();
            println!("Throughput: {:.2} MB/s", throughput_mbps);
        }
        
        println!("Output written to: {}", output.display());
        
        Ok(ScanStats {
            files_processed: final_processed,
            files_failed: final_failed + final_skipped,
            total_bytes: final_bytes,
            duration,
        })
    }
    
    /// Walk directory using jwalk and send file paths to channel as they're discovered
    /// This is the producer in the producer-consumer pattern
    fn walk_directory_streaming(
        root: &Path,
        sender: Sender<PathBuf>,
        use_ignore: bool,
        exclude_file: Option<&Path>,
        total_files_discovered: Arc<Mutex<usize>>,
    ) -> Result<(), ScanError> {
        // Load .hashignore patterns if enabled
        let ignore_handler = if use_ignore {
            match IgnoreHandler::new(root) {
                Ok(handler) => Some(handler),
                Err(e) => {
                    eprintln!("Warning: Failed to load .hashignore: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Canonicalize exclude path once before the loop to avoid redundant calls
        let canonical_exclude = exclude_file.and_then(|p| p.canonicalize().ok());
        
        // Use jwalk for parallel directory traversal
        // Use RayonNewPool to parallelize directory walking in a separate thread pool
        // This avoids conflicts with the main rayon pool used for hashing
        // Configure to follow links and not skip hidden files
        for entry_result in WalkDir::new(root)
            .parallelism(jwalk::Parallelism::RayonNewPool(0)) // 0 = use default thread count
            .skip_hidden(false)  // Don't skip hidden files
            .follow_links(false) // Don't follow symlinks to avoid loops
        {
            match entry_result {
                Ok(entry) => {
                    let path = entry.path();
                    
                    // Only process regular files
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    
                    // Check if this is the excluded file
                    if let Some(ref exclude_canonical) = canonical_exclude {
                        // Compare canonical paths (only canonicalize current path once)
                        if let Ok(canonical_path) = path.canonicalize() {
                            if &canonical_path == exclude_canonical {
                                continue;
                            }
                        }
                    }
                    
                    // Check if this path should be ignored
                    if let Some(ref handler) = ignore_handler {
                        if let Ok(rel_path) = path.strip_prefix(root) {
                            if handler.should_ignore(rel_path, false) {
                                continue;
                            }
                        }
                    }
                    
                    // Send file path to channel
                    // If channel is full, this will block (backpressure)
                    if let Err(_) = sender.send(path) {
                        // Receiver has been dropped, stop walking
                        break;
                    }
                    
                    // Track total files discovered
                    let mut total = total_files_discovered.lock().unwrap();
                    *total += 1;
                }
                Err(e) => {
                    // Log errors during directory scans without stopping
                    eprintln!("Warning: Error walking directory: {}", e);
                }
            }
        }
        
        // Channel will be closed when sender is dropped
        Ok(())
    }
    
    /// Recursively collect all regular files in a directory tree
    /// 
    /// # Arguments
    /// * `root` - Root directory to traverse
    /// 
    /// # Returns
    /// Vector of all file paths found
    pub fn collect_files(&self, root: &Path) -> Result<Vec<PathBuf>, ScanError> {
        self.collect_files_with_exclusion(root, None)
    }
    
    /// Recursively collect all regular files in a directory tree, excluding a specific file
    /// 
    /// # Arguments
    /// * `root` - Root directory to traverse
    /// * `exclude_file` - Optional file path to exclude from collection
    /// 
    /// # Returns
    /// Vector of all file paths found
    fn collect_files_with_exclusion(&self, root: &Path, exclude_file: Option<&Path>) -> Result<Vec<PathBuf>, ScanError> {
        let mut files = Vec::new();
        
        // Load .hashignore patterns if enabled
        let ignore_handler = if self.use_ignore {
            match IgnoreHandler::new(root) {
                Ok(handler) => Some(handler),
                Err(e) => {
                    eprintln!("Warning: Failed to load .hashignore: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        self.collect_files_recursive(root, root, &mut files, ignore_handler.as_ref(), exclude_file)?;
        Ok(files)
    }
    
    /// Helper function for recursive file collection
    fn collect_files_recursive(
        &self,
        root: &Path,
        dir: &Path,
        files: &mut Vec<PathBuf>,
        ignore_handler: Option<&IgnoreHandler>,
        exclude_file: Option<&Path>,
    ) -> Result<(), ScanError> {
        self.collect_files_recursive_with_cache(root, dir, files, ignore_handler, exclude_file, &mut None)
    }
    
    /// Helper function for recursive file collection with cached exclude path
    fn collect_files_recursive_with_cache(
        &self,
        root: &Path,
        dir: &Path,
        files: &mut Vec<PathBuf>,
        ignore_handler: Option<&IgnoreHandler>,
        exclude_file: Option<&Path>,
        canonical_exclude_cache: &mut Option<PathBuf>,
    ) -> Result<(), ScanError> {
        // Check if path exists and is accessible
        if !dir.exists() {
            return Err(HashUtilityError::DirectoryNotFound {
                path: dir.to_path_buf(),
            });
        }
        
        // Canonicalize exclude path once on first call
        if canonical_exclude_cache.is_none() && exclude_file.is_some() {
            *canonical_exclude_cache = exclude_file.and_then(|p| p.canonicalize().ok());
        }
        
        // Read directory entries
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                // Log permission errors but don't stop the scan (Requirement 2.4)
                eprintln!("Warning: Cannot read directory {}: {}", dir.display(), e);
                return Ok(());
            }
        };
        
        // Process each entry
        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(e) => {
                    // Log errors during directory scans without stopping (Requirement 2.4)
                    eprintln!("Warning: Cannot read directory entry: {}", e);
                    continue;
                }
            };
            
            let path = entry.path();
            
            // Get metadata to determine if it's a file or directory
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(e) => {
                    // Log errors during directory scans without stopping (Requirement 2.4)
                    eprintln!("Warning: Cannot read metadata for {}: {}", path.display(), e);
                    continue;
                }
            };
            
            let is_dir = metadata.is_dir();
            
            // Check if this is the excluded file using cached canonical path
            if let Some(ref exclude_canonical) = canonical_exclude_cache {
                if let Ok(canonical_path) = path.canonicalize() {
                    if &canonical_path == exclude_canonical {
                        // Skip the excluded file
                        continue;
                    }
                }
            }
            
            // Check if this path should be ignored
            if let Some(handler) = ignore_handler {
                // Get relative path for ignore matching
                if let Ok(rel_path) = path.strip_prefix(root) {
                    if handler.should_ignore(rel_path, is_dir) {
                        // Skip ignored files and directories
                        continue;
                    }
                }
            }
            
            if metadata.is_file() {
                // Add regular files to the list
                files.push(path);
            } else if is_dir {
                // Recursively process subdirectories with cached exclude path
                if let Err(e) = self.collect_files_recursive_with_cache(root, &path, files, ignore_handler, exclude_file, canonical_exclude_cache) {
                    // Log error but continue with other directories (Requirement 2.4)
                    eprintln!("Warning: Error processing directory {}: {}", path.display(), e);
                }
            }
            // Skip symbolic links and other special files
        }
        
        Ok(())
    }
}

impl Default for ScanEngine {
    fn default() -> Self {
        Self::new()
    }
}

// Tests moved to tests/hash/scan_tests.rs

