// Tests for scan module
// Extracted from src/hash/scan.rs

use abyss::hash::ScanEngine;
use std::fs;
use std::path::Path;

#[test]
fn test_scan_single_file() {
    // Create a temporary directory with a single file
    let test_dir = "test_scan_single";
    fs::create_dir_all(test_dir).unwrap();
    
    let test_file = format!("{}/test.txt", test_dir);
    fs::write(&test_file, b"hello world").unwrap();
    
    // Scan the directory
    let engine = ScanEngine::new();
    let output = format!("{}/hashes.txt", test_dir);
    let stats = engine.scan_directory(
        Path::new(test_dir),
        "sha256",
        Path::new(&output),
    ).unwrap();
    
    // Verify statistics
    assert_eq!(stats.files_processed, 1);
    assert_eq!(stats.files_failed, 0);
    assert!(stats.total_bytes > 0);
    
    // Verify output file exists and contains the hash
    assert!(Path::new(&output).exists());
    let content = fs::read_to_string(&output).unwrap();
    assert!(content.contains("test.txt"));
    assert!(content.contains("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_scan_multiple_files() {
    // Create a temporary directory with multiple files
    let test_dir = "test_scan_multiple";
    fs::create_dir_all(test_dir).unwrap();
    
    fs::write(format!("{}/file1.txt", test_dir), b"content1").unwrap();
    fs::write(format!("{}/file2.txt", test_dir), b"content2").unwrap();
    fs::write(format!("{}/file3.txt", test_dir), b"content3").unwrap();
    
    // Scan the directory
    let engine = ScanEngine::new();
    let output = format!("{}/hashes.txt", test_dir);
    let stats = engine.scan_directory(
        Path::new(test_dir),
        "md5",
        Path::new(&output),
    ).unwrap();
    
    // Verify statistics
    assert_eq!(stats.files_processed, 3);
    assert_eq!(stats.files_failed, 0);
    
    // Verify output file contains all files
    let content = fs::read_to_string(&output).unwrap();
    assert!(content.contains("file1.txt"));
    assert!(content.contains("file2.txt"));
    assert!(content.contains("file3.txt"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_scan_nested_directories() {
    // Create a nested directory structure
    let test_dir = "test_scan_nested";
    fs::create_dir_all(format!("{}/subdir1/subdir2", test_dir)).unwrap();
    
    fs::write(format!("{}/root.txt", test_dir), b"root").unwrap();
    fs::write(format!("{}/subdir1/sub1.txt", test_dir), b"sub1").unwrap();
    fs::write(format!("{}/subdir1/subdir2/sub2.txt", test_dir), b"sub2").unwrap();
    
    // Scan the directory
    let engine = ScanEngine::new();
    let output = format!("{}/hashes.txt", test_dir);
    let stats = engine.scan_directory(
        Path::new(test_dir),
        "sha256",
        Path::new(&output),
    ).unwrap();
    
    // Verify all files were found
    assert_eq!(stats.files_processed, 3);
    assert_eq!(stats.files_failed, 0);
    
    // Verify output contains all files
    let content = fs::read_to_string(&output).unwrap();
    assert!(content.contains("root.txt"));
    assert!(content.contains("sub1.txt"));
    assert!(content.contains("sub2.txt"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_scan_empty_directory() {
    // Create an empty directory
    let test_dir = "test_scan_empty";
    fs::create_dir_all(test_dir).unwrap();
    
    // Scan the directory
    let engine = ScanEngine::new();
    let output = format!("{}/hashes.txt", test_dir);
    let stats = engine.scan_directory(
        Path::new(test_dir),
        "sha256",
        Path::new(&output),
    ).unwrap();
    
    // Verify no files were processed
    assert_eq!(stats.files_processed, 0);
    assert_eq!(stats.files_failed, 0);
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_scan_nonexistent_directory() {
    let engine = ScanEngine::new();
    let result = engine.scan_directory(
        Path::new("nonexistent_directory_xyz"),
        "sha256",
        Path::new("output.txt"),
    );
    
    assert!(result.is_err());
}

#[test]
fn test_scan_parallel_mode() {
    // Create a temporary directory with multiple files
    let test_dir = "test_scan_parallel";
    fs::create_dir_all(test_dir).unwrap();
    
    fs::write(format!("{}/file1.txt", test_dir), b"content1").unwrap();
    fs::write(format!("{}/file2.txt", test_dir), b"content2").unwrap();
    fs::write(format!("{}/file3.txt", test_dir), b"content3").unwrap();
    fs::write(format!("{}/file4.txt", test_dir), b"content4").unwrap();
    
    // Scan the directory with parallel mode enabled
    let engine = ScanEngine::with_parallel(true);
    let output = format!("{}/hashes_parallel.txt", test_dir);
    let stats = engine.scan_directory(
        Path::new(test_dir),
        "sha256",
        Path::new(&output),
    ).unwrap();
    
    // Verify statistics
    assert_eq!(stats.files_processed, 4);
    assert_eq!(stats.files_failed, 0);
    
    // Verify output file contains all files
    let content = fs::read_to_string(&output).unwrap();
    assert!(content.contains("file1.txt"));
    assert!(content.contains("file2.txt"));
    assert!(content.contains("file3.txt"));
    assert!(content.contains("file4.txt"));
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_collect_files_recursive() {
    // Create a test directory structure
    let test_dir = "test_collect_files";
    fs::create_dir_all(format!("{}/dir1/dir2", test_dir)).unwrap();
    
    fs::write(format!("{}/file1.txt", test_dir), b"test").unwrap();
    fs::write(format!("{}/dir1/file2.txt", test_dir), b"test").unwrap();
    fs::write(format!("{}/dir1/dir2/file3.txt", test_dir), b"test").unwrap();
    
    // Collect files
    let engine = ScanEngine::new();
    let files = engine.collect_files(std::path::Path::new(test_dir)).unwrap();
    
    // Verify all files were collected
    assert_eq!(files.len(), 3);
    
    // Cleanup
    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn test_scan_parallel_vs_sequential() {
    // Create separate temporary directories for sequential and parallel tests
    let test_dir_seq = "test_scan_seq";
    let test_dir_par = "test_scan_par";
    
    // Setup sequential test directory
    fs::create_dir_all(test_dir_seq).unwrap();
    fs::write(format!("{}/file1.txt", test_dir_seq), b"test data 1").unwrap();
    fs::write(format!("{}/file2.txt", test_dir_seq), b"test data 2").unwrap();
    fs::write(format!("{}/file3.txt", test_dir_seq), b"test data 3").unwrap();
    
    // Setup parallel test directory with identical content
    fs::create_dir_all(test_dir_par).unwrap();
    fs::write(format!("{}/file1.txt", test_dir_par), b"test data 1").unwrap();
    fs::write(format!("{}/file2.txt", test_dir_par), b"test data 2").unwrap();
    fs::write(format!("{}/file3.txt", test_dir_par), b"test data 3").unwrap();
    
    // Scan sequentially
    let engine_seq = ScanEngine::with_parallel(false);
    let output_seq = format!("{}/output_seq.txt", test_dir_seq);
    let stats_seq = engine_seq.scan_directory(
        Path::new(test_dir_seq),
        "sha256",
        Path::new(&output_seq),
    ).unwrap();
    
    // Scan in parallel
    let engine_par = ScanEngine::with_parallel(true);
    let output_par = format!("{}/output_par.txt", test_dir_par);
    let stats_par = engine_par.scan_directory(
        Path::new(test_dir_par),
        "sha256",
        Path::new(&output_par),
    ).unwrap();
    
    // Both should process the same number of files
    assert_eq!(stats_seq.files_processed, stats_par.files_processed);
    assert_eq!(stats_seq.files_processed, 3);
    
    // Cleanup
    fs::remove_dir_all(test_dir_seq).unwrap();
    fs::remove_dir_all(test_dir_par).unwrap();
}

