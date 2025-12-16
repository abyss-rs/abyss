// Tests for utility modules: wildcard, path_utils, ignore_handler
// Extracted from src/hash/*.rs

use abyss::hash::HashUtilityError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// ============ Wildcard Tests ============

#[test]
fn test_expand_pattern_no_wildcard() {
    use abyss::hash::wildcard::expand_pattern;
    
    let result = expand_pattern("file.txt").unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], PathBuf::from("file.txt"));
}

#[test]
fn test_expand_pattern_no_matches() {
    use abyss::hash::wildcard::expand_pattern;
    
    let result = expand_pattern("nonexistent*.txt");
    assert!(result.is_err());
    
    if let Err(HashUtilityError::InvalidArguments { message }) = result {
        assert!(message.contains("No files match pattern"));
    } else {
        panic!("Expected InvalidArguments error");
    }
}

#[test]
fn test_expand_pattern_with_matches() {
    use abyss::hash::wildcard::expand_pattern;
    
    // Create temporary test files
    let temp_dir = std::env::temp_dir();
    let test_files = vec![
        temp_dir.join("test_wildcard_1.txt"),
        temp_dir.join("test_wildcard_2.txt"),
        temp_dir.join("test_wildcard_3.txt"),
    ];
    
    // Create the test files
    for file in &test_files {
        let mut f = fs::File::create(file).unwrap();
        f.write_all(b"test").unwrap();
    }
    
    // Test wildcard expansion
    let pattern = temp_dir.join("test_wildcard_*.txt").to_string_lossy().to_string();
    let result = expand_pattern(&pattern).unwrap();
    
    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|p| p.to_string_lossy().contains("test_wildcard_")));
    
    // Clean up test files
    for file in &test_files {
        let _ = fs::remove_file(file);
    }
}

// ============ Path Utils Tests ============

#[test]
fn test_normalize_path_string_forward_slash() {
    use abyss::hash::path_utils::normalize_path_string;
    
    let input = "path/to/file.txt";
    let result = normalize_path_string(input);
    
    if cfg!(windows) {
        assert_eq!(result, "path\\to\\file.txt");
    } else {
        assert_eq!(result, "path/to/file.txt");
    }
}

#[test]
fn test_normalize_path_string_backward_slash() {
    use abyss::hash::path_utils::normalize_path_string;
    
    let input = "path\\to\\file.txt";
    let result = normalize_path_string(input);
    
    if cfg!(windows) {
        assert_eq!(result, "path\\to\\file.txt");
    } else {
        assert_eq!(result, "path/to/file.txt");
    }
}

#[test]
fn test_parse_database_path() {
    use abyss::hash::path_utils::parse_database_path;
    
    let input = "path/to\\file.txt";
    let result = parse_database_path(input);
    
    // Should create a valid PathBuf
    assert!(result.to_str().is_some());
}

#[test]
fn test_try_canonicalize_existing_file() {
    use abyss::hash::path_utils::try_canonicalize;
    
    // Create a temporary file
    let test_file = "test_canonicalize_temp.txt";
    fs::write(test_file, b"test").unwrap();
    
    let result = try_canonicalize(Path::new(test_file));
    assert!(result.is_ok());
    
    let canonical = result.unwrap();
    assert!(canonical.is_absolute());
    
    // Cleanup
    fs::remove_file(test_file).unwrap();
}

#[test]
fn test_resolve_path_relative() {
    use abyss::hash::path_utils::resolve_path;
    
    let base = Path::new("/base/dir");
    let relative = Path::new("subdir/file.txt");
    
    let result = resolve_path(relative, base);
    assert_eq!(result, PathBuf::from("/base/dir/subdir/file.txt"));
}

#[test]
fn test_clean_path_with_current_dir() {
    use abyss::hash::path_utils::clean_path;
    
    let path = Path::new("./path/./to/./file.txt");
    let result = clean_path(path);
    
    assert_eq!(result, PathBuf::from("path/to/file.txt"));
}

#[test]
fn test_clean_path_with_parent_dir() {
    use abyss::hash::path_utils::clean_path;
    
    let path = Path::new("path/to/../file.txt");
    let result = clean_path(path);
    
    assert_eq!(result, PathBuf::from("path/file.txt"));
}

// ============ Ignore Handler Tests ============

#[test]
fn test_ignore_handler_no_hashignore() {
    use abyss::hash::ignore_handler::IgnoreHandler;
    
    // Create a temporary directory without .hashignore
    let test_dir = "test_ignore_no_file";
    fs::create_dir_all(test_dir).unwrap();
    
    // Create handler
    let handler = IgnoreHandler::new(Path::new(test_dir)).unwrap();
    
    // No files should be ignored
    assert!(!handler.should_ignore(Path::new("test.txt"), false));
    assert!(!handler.should_ignore(Path::new("subdir/file.txt"), false));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_ignore_handler_basic_patterns() {
    use abyss::hash::ignore_handler::IgnoreHandler;
    
    // Create a temporary directory with .hashignore
    let test_dir = "test_ignore_basic";
    fs::create_dir_all(test_dir).unwrap();
    
    // Create .hashignore with basic patterns
    let hashignore_content = "*.log\n*.tmp\ntemp/\n";
    fs::write(format!("{}/.hashignore", test_dir), hashignore_content).unwrap();
    
    // Create handler
    let handler = IgnoreHandler::new(Path::new(test_dir)).unwrap();
    
    // Test patterns
    assert!(handler.should_ignore(Path::new("test.log"), false));
    assert!(handler.should_ignore(Path::new("file.tmp"), false));
    assert!(handler.should_ignore(Path::new("temp"), true));
    assert!(!handler.should_ignore(Path::new("test.txt"), false));
    assert!(!handler.should_ignore(Path::new("data.csv"), false));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

// ============ Additional Path Utils Tests ============

#[test]
fn test_normalize_path_string_mixed() {
    use abyss::hash::path_utils::normalize_path_string;
    
    let input = "path/to\\mixed/file.txt";
    let result = normalize_path_string(input);
    
    if cfg!(windows) {
        assert_eq!(result, "path\\to\\mixed\\file.txt");
    } else {
        assert_eq!(result, "path/to/mixed/file.txt");
    }
}

#[test]
fn test_try_canonicalize_nonexistent_file() {
    use abyss::hash::path_utils::try_canonicalize;
    
    let result = try_canonicalize(Path::new("nonexistent_file_xyz.txt"));
    assert!(result.is_ok());
    
    // Should return the path as-is
    let path = result.unwrap();
    assert_eq!(path, PathBuf::from("nonexistent_file_xyz.txt"));
}

#[test]
fn test_get_relative_path() {
    use abyss::hash::path_utils::get_relative_path;
    
    // Create a temporary directory structure
    let test_dir = "test_relative_path";
    fs::create_dir_all(format!("{}/subdir", test_dir)).unwrap();
    
    let file_path = format!("{}/subdir/file.txt", test_dir);
    fs::write(&file_path, b"test").unwrap();
    
    // Get relative path
    let base = Path::new(test_dir).canonicalize().unwrap();
    let file = Path::new(&file_path).canonicalize().unwrap();
    
    let result = get_relative_path(&file, &base);
    assert!(result.is_ok());
    
    let relative = result.unwrap();
    // Verify the relative path contains the expected parts
    let relative_str = relative.to_string_lossy();
    assert!(relative_str.contains("subdir"));
    assert!(relative_str.contains("file.txt"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_get_relative_path_cached() {
    use abyss::hash::path_utils::get_relative_path_cached;
    
    // Create a temporary directory structure
    let test_dir = "test_relative_path_cached";
    fs::create_dir_all(format!("{}/subdir", test_dir)).unwrap();
    
    let file_path = format!("{}/subdir/file.txt", test_dir);
    fs::write(&file_path, b"test").unwrap();
    
    // Pre-canonicalize base path (simulating cached scenario)
    let canonical_base = Path::new(test_dir).canonicalize().unwrap();
    let file = Path::new(&file_path);
    
    // Get relative path using cached base
    let result = get_relative_path_cached(&file, &canonical_base);
    assert!(result.is_ok());
    
    let relative = result.unwrap();
    let relative_str = relative.to_string_lossy();
    assert!(relative_str.contains("subdir"));
    assert!(relative_str.contains("file.txt"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_resolve_path_absolute() {
    use abyss::hash::path_utils::resolve_path;
    
    let base = Path::new("/base/dir");
    let absolute = Path::new("/absolute/path/file.txt");
    
    let result = resolve_path(absolute, base);
    assert_eq!(result, PathBuf::from("/absolute/path/file.txt"));
}

#[test]
fn test_clean_path_complex() {
    use abyss::hash::path_utils::clean_path;
    
    let path = Path::new("./path/./to/../../other/file.txt");
    let result = clean_path(path);
    
    assert_eq!(result, PathBuf::from("other/file.txt"));
}

#[test]
fn test_clean_path_empty() {
    use abyss::hash::path_utils::clean_path;
    
    let path = Path::new("./.");
    let result = clean_path(path);
    
    assert_eq!(result, PathBuf::from("."));
}

#[test]
fn test_clean_path_parent_only() {
    use abyss::hash::path_utils::clean_path;
    
    let path = Path::new("..");
    let result = clean_path(path);
    
    assert_eq!(result, PathBuf::from(".."));
}

// ============ Additional Wildcard Tests ============

#[test]
fn test_contains_wildcard() {
    use abyss::hash::wildcard::contains_wildcard;
    
    assert!(contains_wildcard("*.txt"));
    assert!(contains_wildcard("file?.bin"));
    assert!(contains_wildcard("[abc]*.jpg"));
    assert!(contains_wildcard("data/*/hashes"));
    assert!(!contains_wildcard("file.txt"));
    assert!(!contains_wildcard("path/to/file.bin"));
}

#[test]
fn test_expand_pattern_question_mark() {
    use abyss::hash::wildcard::expand_pattern;
    
    // Create temporary test files
    let temp_dir = std::env::temp_dir();
    let test_files = vec![
        temp_dir.join("test_q1.bin"),
        temp_dir.join("test_q2.bin"),
        temp_dir.join("test_qa.bin"),
    ];
    
    // Create the test files
    for file in &test_files {
        let mut f = fs::File::create(file).unwrap();
        f.write_all(b"test").unwrap();
    }
    
    // Test wildcard expansion with ?
    let pattern = temp_dir.join("test_q?.bin").to_string_lossy().to_string();
    let result = expand_pattern(&pattern).unwrap();
    
    assert_eq!(result.len(), 3);
    
    // Clean up test files
    for file in &test_files {
        let _ = fs::remove_file(file);
    }
}

// ============ Additional Ignore Handler Tests ============

#[test]
fn test_ignore_handler_negation() {
    use abyss::hash::ignore_handler::IgnoreHandler;
    
    // Create a temporary directory with .hashignore
    let test_dir = "test_ignore_negation";
    fs::create_dir_all(test_dir).unwrap();
    
    // Create .hashignore with negation pattern
    let hashignore_content = "*.log\n!important.log\n";
    fs::write(format!("{}/.hashignore", test_dir), hashignore_content).unwrap();
    
    // Create handler
    let handler = IgnoreHandler::new(Path::new(test_dir)).unwrap();
    
    // Test patterns
    assert!(handler.should_ignore(Path::new("test.log"), false));
    assert!(handler.should_ignore(Path::new("debug.log"), false));
    assert!(!handler.should_ignore(Path::new("important.log"), false));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_ignore_handler_comments() {
    use abyss::hash::ignore_handler::IgnoreHandler;
    
    // Create a temporary directory with .hashignore
    let test_dir = "test_ignore_comments";
    fs::create_dir_all(test_dir).unwrap();
    
    // Create .hashignore with comments
    let hashignore_content = "# This is a comment\n*.log\n# Another comment\n*.tmp\n";
    fs::write(format!("{}/.hashignore", test_dir), hashignore_content).unwrap();
    
    // Create handler
    let handler = IgnoreHandler::new(Path::new(test_dir)).unwrap();
    
    // Test patterns (comments should be ignored)
    assert!(handler.should_ignore(Path::new("test.log"), false));
    assert!(handler.should_ignore(Path::new("file.tmp"), false));
    assert!(!handler.should_ignore(Path::new("test.txt"), false));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_ignore_handler_subdirectories() {
    use abyss::hash::ignore_handler::IgnoreHandler;
    
    // Create a temporary directory with .hashignore
    let test_dir = "test_ignore_subdir";
    fs::create_dir_all(test_dir).unwrap();
    
    // Create .hashignore with directory patterns
    let hashignore_content = "build/\nnode_modules/\n*.o\n";
    fs::write(format!("{}/.hashignore", test_dir), hashignore_content).unwrap();
    
    // Create handler
    let handler = IgnoreHandler::new(Path::new(test_dir)).unwrap();
    
    // Test directory patterns
    assert!(handler.should_ignore(Path::new("build"), true));
    assert!(handler.should_ignore(Path::new("node_modules"), true));
    assert!(handler.should_ignore(Path::new("src/main.o"), false));
    assert!(!handler.should_ignore(Path::new("src"), true));
    assert!(!handler.should_ignore(Path::new("src/main.c"), false));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

