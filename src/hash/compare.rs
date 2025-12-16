// Compare engine module
// Compares two hash databases and generates detailed comparison reports

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use super::database::{DatabaseHandler, DatabaseEntry};
use super::error::HashUtilityError;

/// Result of comparing a single file between two databases
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub hash_db1: String,
    pub hash_db2: String,
}

/// Group of files with the same hash (duplicates)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub paths: Vec<PathBuf>,
    pub count: usize,
}

/// Comprehensive comparison report between two databases
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompareReport {
    pub db1_total_files: usize,
    pub db2_total_files: usize,
    pub unchanged_files: usize,
    pub changed_files: Vec<ChangedFile>,
    pub removed_files: Vec<PathBuf>,
    pub added_files: Vec<PathBuf>,
    pub duplicates_db1: Vec<DuplicateGroup>,
    pub duplicates_db2: Vec<DuplicateGroup>,
}

impl CompareReport {
    /// Display the comparison report in plain text format
    pub fn display(&self) {
        println!("\n=== Database Comparison Report ===\n");
        
        // Summary section
        println!("Summary:");
        println!("  Database 1: {} files", self.db1_total_files);
        println!("  Database 2: {} files", self.db2_total_files);
        println!("  Unchanged:  {} files", self.unchanged_files);
        println!("  Changed:    {} files", self.changed_files.len());
        println!("  Removed:    {} files", self.removed_files.len());
        println!("  Added:      {} files", self.added_files.len());
        println!("  Duplicates in DB1: {} groups", self.duplicates_db1.len());
        println!("  Duplicates in DB2: {} groups", self.duplicates_db2.len());
        
        // Changed files section
        if !self.changed_files.is_empty() {
            println!("\nChanged Files:");
            for changed in &self.changed_files {
                println!("  {}", changed.path.display());
                println!("    DB1: {}", changed.hash_db1);
                println!("    DB2: {}", changed.hash_db2);
            }
        }
        
        // Removed files section
        if !self.removed_files.is_empty() {
            println!("\nRemoved Files (in DB1 but not DB2):");
            for path in &self.removed_files {
                println!("  {}", path.display());
            }
        }
        
        // Added files section
        if !self.added_files.is_empty() {
            println!("\nAdded Files (in DB2 but not DB1):");
            for path in &self.added_files {
                println!("  {}", path.display());
            }
        }
        
        // Duplicates in DB1
        if !self.duplicates_db1.is_empty() {
            println!("\nDuplicates in Database 1:");
            for group in &self.duplicates_db1 {
                println!("  Hash: {} ({} files)", group.hash, group.count);
                for path in &group.paths {
                    println!("    {}", path.display());
                }
            }
        }
        
        // Duplicates in DB2
        if !self.duplicates_db2.is_empty() {
            println!("\nDuplicates in Database 2:");
            for group in &self.duplicates_db2 {
                println!("  Hash: {} ({} files)", group.hash, group.count);
                for path in &group.paths {
                    println!("    {}", path.display());
                }
            }
        }
        
        println!();
    }
    
    /// Format the comparison report as plain text string
    pub fn to_plain_text(&self) -> String {
        let mut output = String::new();
        
        output.push_str("\n=== Database Comparison Report ===\n\n");
        
        // Summary section
        output.push_str("Summary:\n");
        output.push_str(&format!("  Database 1: {} files\n", self.db1_total_files));
        output.push_str(&format!("  Database 2: {} files\n", self.db2_total_files));
        output.push_str(&format!("  Unchanged:  {} files\n", self.unchanged_files));
        output.push_str(&format!("  Changed:    {} files\n", self.changed_files.len()));
        output.push_str(&format!("  Removed:    {} files\n", self.removed_files.len()));
        output.push_str(&format!("  Added:      {} files\n", self.added_files.len()));
        output.push_str(&format!("  Duplicates in DB1: {} groups\n", self.duplicates_db1.len()));
        output.push_str(&format!("  Duplicates in DB2: {} groups\n", self.duplicates_db2.len()));
        
        // Changed files section
        if !self.changed_files.is_empty() {
            output.push_str("\nChanged Files:\n");
            for changed in &self.changed_files {
                output.push_str(&format!("  {}\n", changed.path.display()));
                output.push_str(&format!("    DB1: {}\n", changed.hash_db1));
                output.push_str(&format!("    DB2: {}\n", changed.hash_db2));
            }
        }
        
        // Removed files section
        if !self.removed_files.is_empty() {
            output.push_str("\nRemoved Files (in DB1 but not DB2):\n");
            for path in &self.removed_files {
                output.push_str(&format!("  {}\n", path.display()));
            }
        }
        
        // Added files section
        if !self.added_files.is_empty() {
            output.push_str("\nAdded Files (in DB2 but not DB1):\n");
            for path in &self.added_files {
                output.push_str(&format!("  {}\n", path.display()));
            }
        }
        
        // Duplicates in DB1
        if !self.duplicates_db1.is_empty() {
            output.push_str("\nDuplicates in Database 1:\n");
            for group in &self.duplicates_db1 {
                output.push_str(&format!("  Hash: {} ({} files)\n", group.hash, group.count));
                for path in &group.paths {
                    output.push_str(&format!("    {}\n", path.display()));
                }
            }
        }
        
        // Duplicates in DB2
        if !self.duplicates_db2.is_empty() {
            output.push_str("\nDuplicates in Database 2:\n");
            for group in &self.duplicates_db2 {
                output.push_str(&format!("  Hash: {} ({} files)\n", group.hash, group.count));
                for path in &group.paths {
                    output.push_str(&format!("    {}\n", path.display()));
                }
            }
        }
        
        output.push('\n');
        output
    }
    
    /// Format the comparison report as JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        #[derive(serde::Serialize)]
        struct JsonOutput {
            metadata: Metadata,
            summary: Summary,
            unchanged_files: usize,
            changed_files: Vec<ChangedFileJson>,
            removed_files: Vec<String>,
            added_files: Vec<String>,
            duplicates_db1: Vec<DuplicateGroupJson>,
            duplicates_db2: Vec<DuplicateGroupJson>,
        }
        
        #[derive(serde::Serialize)]
        struct Metadata {
            timestamp: String,
        }
        
        #[derive(serde::Serialize)]
        struct Summary {
            db1_total_files: usize,
            db2_total_files: usize,
            unchanged_count: usize,
            changed_count: usize,
            removed_count: usize,
            added_count: usize,
            duplicates_db1_count: usize,
            duplicates_db2_count: usize,
        }
        
        #[derive(serde::Serialize)]
        struct ChangedFileJson {
            path: String,
            hash_db1: String,
            hash_db2: String,
        }
        
        #[derive(serde::Serialize)]
        struct DuplicateGroupJson {
            hash: String,
            count: usize,
            paths: Vec<String>,
        }
        
        let output = JsonOutput {
            metadata: Metadata {
                timestamp: chrono::Utc::now().to_rfc3339(),
            },
            summary: Summary {
                db1_total_files: self.db1_total_files,
                db2_total_files: self.db2_total_files,
                unchanged_count: self.unchanged_files,
                changed_count: self.changed_files.len(),
                removed_count: self.removed_files.len(),
                added_count: self.added_files.len(),
                duplicates_db1_count: self.duplicates_db1.len(),
                duplicates_db2_count: self.duplicates_db2.len(),
            },
            unchanged_files: self.unchanged_files,
            changed_files: self.changed_files.iter().map(|cf| ChangedFileJson {
                path: cf.path.display().to_string(),
                hash_db1: cf.hash_db1.clone(),
                hash_db2: cf.hash_db2.clone(),
            }).collect(),
            removed_files: self.removed_files.iter().map(|p| p.display().to_string()).collect(),
            added_files: self.added_files.iter().map(|p| p.display().to_string()).collect(),
            duplicates_db1: self.duplicates_db1.iter().map(|dg| DuplicateGroupJson {
                hash: dg.hash.clone(),
                count: dg.count,
                paths: dg.paths.iter().map(|p| p.display().to_string()).collect(),
            }).collect(),
            duplicates_db2: self.duplicates_db2.iter().map(|dg| DuplicateGroupJson {
                hash: dg.hash.clone(),
                count: dg.count,
                paths: dg.paths.iter().map(|p| p.display().to_string()).collect(),
            }).collect(),
        };
        
        serde_json::to_string_pretty(&output)
    }
}

/// Engine for comparing two hash databases
pub struct CompareEngine;

impl CompareEngine {
    /// Create a new CompareEngine
    pub fn new() -> Self {
        CompareEngine
    }
    
    /// Compare two hash databases and generate a detailed report
    /// 
    /// # Arguments
    /// * `database1` - Path to the first database file
    /// * `database2` - Path to the second database file
    /// 
    /// # Returns
    /// A CompareReport containing all comparison findings
    /// 
    /// # Errors
    /// Returns an error if either database cannot be read
    pub fn compare(
        &self,
        database1: &Path,
        database2: &Path,
    ) -> Result<CompareReport, HashUtilityError> {
        // Load both databases
        let db1 = DatabaseHandler::read_database(database1)?;
        let db2 = DatabaseHandler::read_database(database2)?;
        
        // Detect duplicates in each database
        let duplicates_db1 = Self::find_duplicates(&db1);
        let duplicates_db2 = Self::find_duplicates(&db2);
        
        // Get all unique file paths from both databases
        let all_paths: HashSet<PathBuf> = db1.keys()
            .chain(db2.keys())
            .cloned()
            .collect();
        
        // Classify files
        let mut unchanged_count = 0;
        let mut changed_files = Vec::new();
        let mut removed_files = Vec::new();
        let mut added_files = Vec::new();
        
        for path in all_paths {
            match (db1.get(&path), db2.get(&path)) {
                (Some(entry1), Some(entry2)) => {
                    // File exists in both databases
                    if entry1.hash == entry2.hash {
                        // Hashes match - unchanged
                        unchanged_count += 1;
                    } else {
                        // Hashes differ - changed
                        changed_files.push(ChangedFile {
                            path: path.clone(),
                            hash_db1: entry1.hash.clone(),
                            hash_db2: entry2.hash.clone(),
                        });
                    }
                }
                (Some(_), None) => {
                    // File exists in DB1 but not DB2 - removed
                    removed_files.push(path.clone());
                }
                (None, Some(_)) => {
                    // File exists in DB2 but not DB1 - added
                    added_files.push(path.clone());
                }
                (None, None) => {
                    // This should never happen since we got the path from one of the databases
                    unreachable!("Path should exist in at least one database");
                }
            }
        }
        
        // Sort results for consistent output
        changed_files.sort_by(|a, b| a.path.cmp(&b.path));
        removed_files.sort();
        added_files.sort();
        
        Ok(CompareReport {
            db1_total_files: db1.len(),
            db2_total_files: db2.len(),
            unchanged_files: unchanged_count,
            changed_files,
            removed_files,
            added_files,
            duplicates_db1,
            duplicates_db2,
        })
    }
    
    /// Find duplicate hashes within a database
    /// 
    /// # Arguments
    /// * `database` - The database to search for duplicates
    /// 
    /// # Returns
    /// A vector of DuplicateGroup, each containing files with the same hash
    pub fn find_duplicates(database: &HashMap<PathBuf, DatabaseEntry>) -> Vec<DuplicateGroup> {
        // Build a map from hash to list of paths
        let mut hash_to_paths: HashMap<String, Vec<PathBuf>> = HashMap::new();
        
        for (path, entry) in database {
            hash_to_paths
                .entry(entry.hash.clone())
                .or_insert_with(Vec::new)
                .push(path.clone());
        }
        
        // Filter to only groups with more than one file (duplicates)
        let mut duplicates: Vec<DuplicateGroup> = hash_to_paths
            .into_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .map(|(hash, mut paths)| {
                paths.sort();
                let count = paths.len();
                DuplicateGroup {
                    hash,
                    paths,
                    count,
                }
            })
            .collect();
        
        // Sort by hash for consistent output
        duplicates.sort_by(|a, b| a.hash.cmp(&b.hash));
        
        duplicates
    }
}

// Tests moved to tests/hash/compare_tests.rs

