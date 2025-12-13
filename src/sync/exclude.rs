//! Exclude pattern matching for sync operations.
//!
//! Supports .gitignore-style patterns for excluding files from sync.

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};

/// Default patterns to exclude from sync operations.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    // Version control
    ".git",
    ".git/**",
    ".svn",
    ".svn/**",
    ".hg",
    ".hg/**",
    
    // OS-specific
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "*.swp",
    "*.swo",
    "*~",
    
    // IDE/Editor
    ".idea",
    ".idea/**",
    ".vscode",
    ".vscode/**",
    "*.iml",
    
    // Build artifacts
    "target",
    "target/**",
    "node_modules",
    "node_modules/**",
    "__pycache__",
    "__pycache__/**",
    "*.pyc",
    "*.pyo",
    
    // Temporary files
    "*.tmp",
    "*.temp",
    "*.bak",
    "*.orig",
];

/// Pattern matching for file exclusion.
#[derive(Debug, Clone)]
pub struct ExcludePatterns {
    /// Compiled glob set for matching.
    glob_set: GlobSet,
    /// Raw pattern strings (for display/serialization).
    patterns: Vec<String>,
    /// Whether to use default excludes.
    use_defaults: bool,
}

impl Default for ExcludePatterns {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ExcludePatterns {
    /// Create a new empty exclude pattern set.
    pub fn new() -> Self {
        Self {
            glob_set: GlobSet::empty(),
            patterns: Vec::new(),
            use_defaults: false,
        }
    }

    /// Create with default exclude patterns.
    pub fn with_defaults() -> Self {
        let mut builder = GlobSetBuilder::new();
        let mut patterns = Vec::new();
        
        for pattern in DEFAULT_EXCLUDES {
            if let Ok(glob) = Glob::new(pattern) {
                builder.add(glob);
                patterns.push(pattern.to_string());
            }
        }
        
        Self {
            glob_set: builder.build().unwrap_or_else(|_| GlobSet::empty()),
            patterns,
            use_defaults: true,
        }
    }

    /// Create from a list of patterns.
    pub fn from_patterns(patterns: &[&str]) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        let mut pattern_list = Vec::new();
        
        for pattern in patterns {
            let glob = Glob::new(pattern)?;
            builder.add(glob);
            pattern_list.push(pattern.to_string());
        }
        
        Ok(Self {
            glob_set: builder.build()?,
            patterns: pattern_list,
            use_defaults: false,
        })
    }

    /// Add a pattern to the exclude set.
    pub fn add_pattern(&mut self, pattern: &str) -> Result<()> {
        // Rebuild the glob set with the new pattern
        let mut builder = GlobSetBuilder::new();
        
        for existing in &self.patterns {
            if let Ok(glob) = Glob::new(existing) {
                builder.add(glob);
            }
        }
        
        let glob = Glob::new(pattern)?;
        builder.add(glob);
        self.patterns.push(pattern.to_string());
        
        self.glob_set = builder.build()?;
        Ok(())
    }

    /// Remove a pattern from the exclude set.
    pub fn remove_pattern(&mut self, pattern: &str) -> Result<()> {
        self.patterns.retain(|p| p != pattern);
        
        // Rebuild glob set
        let mut builder = GlobSetBuilder::new();
        for existing in &self.patterns {
            if let Ok(glob) = Glob::new(existing) {
                builder.add(glob);
            }
        }
        
        self.glob_set = builder.build()?;
        Ok(())
    }

    /// Check if a path should be excluded.
    pub fn is_excluded(&self, path: &str) -> bool {
        // Check against the path and also just the filename
        if self.glob_set.is_match(path) {
            return true;
        }
        
        // Also check just the filename for patterns like ".DS_Store"
        if let Some(filename) = std::path::Path::new(path).file_name() {
            if self.glob_set.is_match(filename.to_string_lossy().as_ref()) {
                return true;
            }
        }
        
        // Check each path component for directory patterns
        for component in std::path::Path::new(path).components() {
            if let std::path::Component::Normal(name) = component {
                if self.glob_set.is_match(name.to_string_lossy().as_ref()) {
                    return true;
                }
            }
        }
        
        false
    }

    /// Get the list of patterns.
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Parse patterns from a string (one per line, like .gitignore).
    pub fn parse_gitignore(content: &str) -> Result<Self> {
        let mut patterns = Vec::new();
        
        for line in content.lines() {
            let line = line.trim();
            
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            
            // Handle negation (!) - we skip these for now
            if line.starts_with('!') {
                continue;
            }
            
            patterns.push(line);
        }
        
        Self::from_patterns(&patterns.iter().map(|s| s.as_ref()).collect::<Vec<&str>>())
    }

    /// Merge another exclude patterns set into this one.
    pub fn merge(&mut self, other: &ExcludePatterns) -> Result<()> {
        for pattern in &other.patterns {
            if !self.patterns.contains(pattern) {
                self.add_pattern(pattern)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_excludes() {
        let excludes = ExcludePatterns::with_defaults();
        
        assert!(excludes.is_excluded(".git"));
        assert!(excludes.is_excluded(".git/config"));
        assert!(excludes.is_excluded(".DS_Store"));
        assert!(excludes.is_excluded("node_modules"));
        assert!(excludes.is_excluded("node_modules/package/index.js"));
        assert!(excludes.is_excluded("file.swp"));
        
        assert!(!excludes.is_excluded("src/main.rs"));
        assert!(!excludes.is_excluded("README.md"));
    }

    #[test]
    fn test_custom_patterns() {
        let excludes = ExcludePatterns::from_patterns(&["*.log", "temp/**"]).unwrap();
        
        assert!(excludes.is_excluded("debug.log"));
        assert!(excludes.is_excluded("temp/file.txt"));
        
        assert!(!excludes.is_excluded("main.rs"));
    }

    #[test]
    fn test_gitignore_parsing() {
        let content = r#"
# Comment
*.log
temp/

# Another comment
!important.log
"#;
        
        let excludes = ExcludePatterns::parse_gitignore(content).unwrap();
        
        assert!(excludes.is_excluded("debug.log"));
        assert!(excludes.is_excluded("temp/"));
    }

    #[test]
    fn test_add_remove_pattern() {
        let mut excludes = ExcludePatterns::new();
        
        excludes.add_pattern("*.txt").unwrap();
        assert!(excludes.is_excluded("file.txt"));
        
        excludes.remove_pattern("*.txt").unwrap();
        assert!(!excludes.is_excluded("file.txt"));
    }
}
