// Database format handler module
// Reads and writes plain text hash database files

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use xz2::read::XzDecoder;
use xz2::write::XzEncoder;

use super::path_utils;
use super::error::HashUtilityError;

/// Database entry with metadata
#[derive(Debug, Clone)]
pub struct DatabaseEntry {
    pub hash: String,
    pub algorithm: String,
    pub fast_mode: bool,
}

/// Database format type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DatabaseFormat {
    /// Standard format: hash  algorithm  fast_mode  filepath
    Standard,
    /// Hashdeep format: size,hash1,hash2,...,filename
    Hashdeep,
}

/// Handler for reading and writing hash database files
pub struct DatabaseHandler;

impl DatabaseHandler {
    /// Check if a path has .xz extension (compressed database)
    pub fn is_compressed(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "xz")
            .unwrap_or(false)
    }
    
    /// Compress a database file with LZMA
    /// Creates a new file with .xz extension
    pub fn compress_database(input_path: &Path) -> Result<PathBuf, HashUtilityError> {
        // Read the input file
        let input_file = File::open(input_path).map_err(|e| {
            HashUtilityError::from_io_error(e, "opening database for compression", Some(input_path.to_path_buf()))
        })?;
        
        // Create output path with .xz extension
        let output_path = input_path.with_extension(
            format!("{}.xz", input_path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("txt"))
        );
        
        // Create compressed output file
        let output_file = File::create(&output_path).map_err(|e| {
            HashUtilityError::from_io_error(e, "creating compressed database", Some(output_path.clone()))
        })?;
        
        // Create LZMA encoder with compression level 6 (good balance of speed and compression)
        let mut encoder = XzEncoder::new(output_file, 6);
        
        // Copy data through the encoder
        let mut reader = BufReader::new(input_file);
        std::io::copy(&mut reader, &mut encoder).map_err(|e| {
            HashUtilityError::from_io_error(e, "compressing database", Some(input_path.to_path_buf()))
        })?;
        
        // Finish compression
        encoder.finish().map_err(|e| {
            HashUtilityError::from_io_error(e, "finalizing compression", Some(output_path.clone()))
        })?;
        
        Ok(output_path)
    }
    
    /// Open a database file, automatically decompressing if it has .xz extension
    pub fn open_database_reader(path: &Path) -> Result<Box<dyn BufRead>, HashUtilityError> {
        let file = File::open(path).map_err(|e| {
            HashUtilityError::from_io_error(e, "opening database", Some(path.to_path_buf()))
        })?;
        
        if Self::is_compressed(path) {
            // Decompress on the fly
            let decoder = XzDecoder::new(file);
            Ok(Box::new(BufReader::new(decoder)))
        } else {
            // Read normally
            Ok(Box::new(BufReader::new(file)))
        }
    }
    
    /// Detect the format of a database file by reading its first few lines
    pub fn detect_format(path: &Path) -> Result<DatabaseFormat, HashUtilityError> {
        let reader = Self::open_database_reader(path)?;
        
        for line_result in reader.lines().take(10) {
            let line = line_result.map_err(|e| {
                HashUtilityError::from_io_error(e, "reading database", Some(path.to_path_buf()))
            })?;
            
            let trimmed = line.trim();
            
            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }
            
            // Check for hashdeep header (starts with %)
            if trimmed.starts_with('%') {
                return Ok(DatabaseFormat::Hashdeep);
            }
            
            // Check for hashdeep CSV format (contains commas)
            if trimmed.contains(',') {
                return Ok(DatabaseFormat::Hashdeep);
            }
            
            // Check for standard format (contains two spaces)
            if trimmed.contains("  ") {
                return Ok(DatabaseFormat::Standard);
            }
        }
        
        // Default to standard format if we can't determine
        Ok(DatabaseFormat::Standard)
    }
    /// Write a single hash entry to the output writer
    /// Format: `<hash>  <algorithm>  <fast_mode>  <filepath>` (two spaces between fields)
    pub fn write_entry(
        writer: &mut impl Write,
        hash: &str,
        algorithm: &str,
        fast_mode: bool,
        path: &Path,
    ) -> io::Result<()> {
        let fast_str = if fast_mode { "fast" } else { "normal" };
        writeln!(writer, "{}  {}  {}  {}", hash, algorithm, fast_str, path.display())
    }
    
    /// Write hashdeep format header
    /// Includes metadata and column definitions
    pub fn write_hashdeep_header(
        writer: &mut impl Write,
        algorithms: &[String],
    ) -> io::Result<()> {
        writeln!(writer, "%%%% HASHDEEP-1.0")?;
        writeln!(writer, "%%%% size,{},filename", algorithms.join(","))?;
        writeln!(writer, "## Invoked from: hash utility")?;
        writeln!(writer, "## $ hash scan --format hashdeep")?;
        writeln!(writer, "##")?;
        Ok(())
    }
    
    /// Write a single entry in hashdeep format
    /// Format: size,hash1,hash2,...,filename
    pub fn write_hashdeep_entry(
        writer: &mut impl Write,
        size: u64,
        hashes: &[String],
        path: &Path,
    ) -> io::Result<()> {
        write!(writer, "{}", size)?;
        for hash in hashes {
            write!(writer, ",{}", hash)?;
        }
        writeln!(writer, ",{}", path.display())
    }
    
    /// Read a hash database file and parse it into a HashMap
    /// Maps file paths to their database entries (hash, algorithm, fast_mode)
    /// Malformed lines are skipped with a warning to stderr
    /// Auto-detects format (standard or hashdeep)
    pub fn read_database(path: &Path) -> Result<HashMap<PathBuf, DatabaseEntry>, HashUtilityError> {
        let format = Self::detect_format(path)?;
        
        match format {
            DatabaseFormat::Standard => Self::read_standard_database(path),
            DatabaseFormat::Hashdeep => Self::read_hashdeep_database(path),
        }
    }
    
    /// Read a standard format database file
    pub fn read_standard_database(path: &Path) -> Result<HashMap<PathBuf, DatabaseEntry>, HashUtilityError> {
        let reader = Self::open_database_reader(path)?;
        let mut database = HashMap::new();
        
        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                HashUtilityError::from_io_error(e, "reading database", Some(path.to_path_buf()))
            })?;
            
            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }
            
            // Parse line: split on two spaces
            match Self::parse_line(&line) {
                Some((hash, algorithm, fast_mode, file_path)) => {
                    database.insert(file_path, DatabaseEntry {
                        hash,
                        algorithm,
                        fast_mode,
                    });
                }
                None => {
                    // Warn about malformed line but continue processing (Requirement 2.4)
                    eprintln!(
                        "Warning: Skipping malformed line {} in database {}: {}",
                        line_num + 1,
                        path.display(),
                        line
                    );
                }
            }
        }
        
        Ok(database)
    }
    
    /// Parse a single line from the database file
    /// Expected format: `<hash>  <algorithm>  <fast_mode>  <filepath>` (two spaces between fields)
    /// Returns None if the line is malformed
    /// Handles both forward and backward slashes in paths
    /// Note: Filenames may contain two spaces, so we only split on the first 3 delimiters
    pub fn parse_line(line: &str) -> Option<(String, String, bool, PathBuf)> {
        // Split on two spaces, but only for the first 3 fields
        // The rest is the filename (which may contain two spaces)
        let parts: Vec<&str> = line.splitn(4, "  ").collect();
        
        if parts.len() == 4 {
            let hash = parts[0].trim();
            let algorithm = parts[1].trim();
            let fast_mode_str = parts[2].trim();
            let path_str = parts[3].trim();
            
            // Parse fast_mode
            let fast_mode = match fast_mode_str {
                "fast" => true,
                "normal" => false,
                _ => return None, // Invalid fast_mode value
            };
            
            // Validate that all fields are not empty
            if !hash.is_empty() && !algorithm.is_empty() && !path_str.is_empty() {
                // Use path_utils to parse the path with proper separator handling
                let path = path_utils::parse_database_path(path_str);
                return Some((hash.to_string(), algorithm.to_string(), fast_mode, path));
            }
        }
        
        None
    }
    
    /// Read a hashdeep format database file
    /// Format: size,hash1,hash2,...,filename
    /// Header lines start with %
    /// Note: For files with multiple hashes, only the first hash is stored
    fn read_hashdeep_database(path: &Path) -> Result<HashMap<PathBuf, DatabaseEntry>, HashUtilityError> {
        let reader = Self::open_database_reader(path)?;
        let mut database = HashMap::new();
        let mut hash_algorithms = Vec::new();
        
        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                HashUtilityError::from_io_error(e, "reading database", Some(path.to_path_buf()))
            })?;
            
            let trimmed = line.trim();
            
            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }
            
            // Parse header lines
            if trimmed.starts_with('%') {
                // Extract algorithm information from header
                // Format: %%%% HASHDEEP-1.0
                // %%%% size,md5,sha256,filename
                if trimmed.starts_with("%%%%") && trimmed.contains(',') {
                    // Parse the algorithm list from header
                    let header_parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if header_parts.len() >= 2 {
                        let fields = header_parts[1];
                        let field_list: Vec<&str> = fields.split(',').collect();
                        // First field is size, last is filename, middle are hash algorithms
                        if field_list.len() >= 3 {
                            hash_algorithms = field_list[1..field_list.len()-1]
                                .iter()
                                .map(|s| s.to_string())
                                .collect();
                        }
                    }
                }
                continue;
            }
            
            // Parse data lines
            match Self::parse_hashdeep_line(trimmed, &hash_algorithms) {
                Some(entries) => {
                    // Only use the first hash entry for each file
                    // (hashdeep can have multiple hashes per file, but our verify engine expects one)
                    if let Some((file_path, entry)) = entries.into_iter().next() {
                        database.insert(file_path, entry);
                    }
                }
                None => {
                    eprintln!(
                        "Warning: Skipping malformed line {} in hashdeep database {}: {}",
                        line_num + 1,
                        path.display(),
                        trimmed
                    );
                }
            }
        }
        
        Ok(database)
    }
    
    /// Parse a single hashdeep format line
    /// Format: size,hash1,hash2,...,filename
    /// Returns multiple entries (one per hash algorithm)
    pub fn parse_hashdeep_line(line: &str, algorithms: &[String]) -> Option<Vec<(PathBuf, DatabaseEntry)>> {
        let parts: Vec<&str> = line.split(',').collect();
        
        // Need at least: size, one hash, filename
        if parts.len() < 3 {
            return None;
        }
        
        // First part is size (we don't use it currently)
        let _size = parts[0].trim();
        
        // Last part is filename
        let filename = parts[parts.len() - 1].trim();
        if filename.is_empty() {
            return None;
        }
        
        let path = path_utils::parse_database_path(filename);
        
        // Middle parts are hashes
        let hashes: Vec<&str> = parts[1..parts.len()-1]
            .iter()
            .map(|s| s.trim())
            .collect();
        
        if hashes.is_empty() {
            return None;
        }
        
        let mut entries = Vec::new();
        
        // If we have algorithm names from header, use them
        if !algorithms.is_empty() && algorithms.len() == hashes.len() {
            for (i, hash) in hashes.iter().enumerate() {
                if !hash.is_empty() {
                    entries.push((
                        path.clone(),
                        DatabaseEntry {
                            hash: hash.to_string(),
                            algorithm: algorithms[i].clone(),
                            fast_mode: false,
                        }
                    ));
                }
            }
        } else {
            // No header or mismatch - try to infer algorithm from hash length
            for hash in hashes {
                if !hash.is_empty() {
                    let algorithm = Self::infer_algorithm_from_hash(hash);
                    entries.push((
                        path.clone(),
                        DatabaseEntry {
                            hash: hash.to_string(),
                            algorithm,
                            fast_mode: false,
                        }
                    ));
                }
            }
        }
        
        if entries.is_empty() {
            None
        } else {
            Some(entries)
        }
    }
    
    /// Infer hash algorithm from hash string length
    pub fn infer_algorithm_from_hash(hash: &str) -> String {
        match hash.len() {
            32 => "md5".to_string(),
            40 => "sha1".to_string(),
            56 => "sha224".to_string(),
            64 => "sha256".to_string(),
            96 => "sha384".to_string(),
            128 => "sha512".to_string(),
            _ => "unknown".to_string(),
        }
    }
}

// Tests moved to tests/hash/database_tests.rs

