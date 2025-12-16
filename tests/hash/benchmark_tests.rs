// Tests for benchmark module
// Extracted from src/hash/benchmark.rs

use abyss::hash::BenchmarkEngine;
use std::time::Duration;

#[test]
fn test_benchmark_engine_creation() {
    let _engine = BenchmarkEngine::new();
    assert!(true); // Just verify it can be created
}

#[test]
fn test_run_benchmarks_small_data() {
    let engine = BenchmarkEngine::new();
    // Use 1MB for faster test
    let results = engine.run_benchmarks(1).unwrap();
    
    // Should have results for all algorithms
    assert!(!results.is_empty());
    
    // All throughput values should be positive
    for result in results {
        assert!(result.throughput_mbps > 0.0);
        assert!(!result.algorithm.is_empty());
    }
}

#[test]
fn test_benchmark_result_structure() {
    use abyss::hash::BenchmarkResult;
    
    let result = BenchmarkResult {
        algorithm: "SHA-256".to_string(),
        throughput_mbps: 500.0,
    };
    
    assert_eq!(result.algorithm, "SHA-256");
    assert_eq!(result.throughput_mbps, 500.0);
}
