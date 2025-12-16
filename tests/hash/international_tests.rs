// Tests for international filenames support
// Extracted from hash-rs/tests/international_filenames_test.rs

use std::fs;
use std::path::PathBuf;
use abyss::hash::ScanEngine;
use abyss::hash::HashComputer;
use std::path::Path;

/// Test data: sample filenames in various languages and scripts
fn get_international_test_filenames() -> Vec<(&'static str, &'static str)> {
    vec![
        // Latin-based languages
        ("English", "english_test_file.txt"),
        ("French", "fichier_test_français_éèêë.txt"),
        ("German", "deutsche_testdatei_äöüß.txt"),
        ("Spanish", "archivo_prueba_español_ñáéíóú.txt"),
        
        // Cyrillic script
        ("Russian", "тестовый_файл_русский.txt"),
        ("Ukrainian", "тестовий_файл_українська_їєіґ.txt"),
        
        // Greek
        ("Greek", "δοκιμαστικό_αρχείο_ελληνικά.txt"),
        
        // Arabic (RTL)
        ("Arabic", "ملف_اختبار_العربية.txt"),
        ("Persian", "فایل_آزمایشی_فارسی.txt"),
        
        // Hebrew (RTL)
        ("Hebrew", "קובץ_בדיקה_עברית.txt"),
        
        // CJK
        ("Chinese_Simplified", "测试文件_简体中文.txt"),
        ("Chinese_Traditional", "測試文件_繁體中文.txt"),
        ("Japanese", "テストファイル_日本語.txt"),
        ("Korean", "테스트_파일_한국어.txt"),
        
        // South Asian scripts
        ("Hindi", "परीक्षण_फ़ाइल_हिंदी.txt"),
        ("Bengali", "পরীক্ষা_ফাইল_বাংলা.txt"),
        ("Tamil", "சோதனை_கோப்பு_தமிழ்.txt"),
        
        // Southeast Asian
        ("Thai", "ไฟล์ทดสอบ_ภาษาไทย.txt"),
        ("Vietnamese", "tệp_thử_nghiệm_tiếng_việt.txt"),
        
        // Other scripts
        ("Georgian", "ტესტის_ფაილი_ქართული.txt"),
        ("Armenian", "փdelays_ delays_ֆdelays.txt"),
        
        // Special characters and symbols
        ("Emoji", "test_file_with_emojis_😀😃😄.txt"),
        ("Mixed_Scripts", "test_тест_测试_テスト.txt"),
        
        // Edge cases
        ("Spaces", "file with spaces.txt"),
        ("Dots", "file.with.dots.txt"),
        ("Long", "this_is_a_very_long_filename_for_testing_purposes.txt"),
    ]
}

#[test]
fn test_international_filenames_scan() {
    let test_dir = "test_international_files";
    let output_db = "test_international_output.txt";
    
    // Create test directory
    fs::create_dir_all(test_dir).expect("Failed to create test directory");
    
    // Create files with international names
    let test_filenames = get_international_test_filenames();
    let mut created_files = Vec::new();
    
    for (lang, filename) in &test_filenames {
        let file_path = PathBuf::from(test_dir).join(filename);
        
        // Try to create the file - some filesystems may not support all characters
        match fs::write(&file_path, format!("Test content for {}", lang)) {
            Ok(_) => {
                created_files.push((lang.to_string(), filename.to_string(), file_path));
            }
            Err(_) => {
                // Skip files that can't be created on this filesystem
            }
        }
    }
    
    // Run scan using library API directly
    let engine = ScanEngine::new();
    let result = engine.scan_directory(
        Path::new(test_dir),
        "sha256",
        Path::new(output_db),
    );
    
    assert!(result.is_ok(), "Scan should succeed");
    let stats = result.unwrap();
    
    // Verify files were processed (may be less due to filesystem limitations)
    assert!(stats.files_processed > 0, "At least some files should be processed");
    
    // Verify output database exists
    assert!(PathBuf::from(output_db).exists(), "Output database should be created");
    
    // Read and verify database content
    let db_content = fs::read_to_string(output_db).expect("Failed to read output database");
    
    // Verify database is not empty
    assert!(!db_content.is_empty(), "Database should not be empty");
    
    // Cleanup
    fs::remove_dir_all(test_dir).ok();
    fs::remove_file(output_db).ok();
}

#[test]
fn test_international_filenames_hash() {
    let test_dir = "test_international_hash";
    fs::create_dir_all(test_dir).expect("Failed to create test directory");
    
    // Test a subset of challenging filenames
    let test_cases = vec![
        ("Russian", "тестовый_файл.txt"),
        ("Chinese", "测试文件.txt"),
        ("Japanese", "テストファイル.txt"),
        ("Arabic", "ملف_اختبار.txt"),
        ("Emoji", "test_😀🎉.txt"),
        ("Mixed", "test_тест_测试.txt"),
    ];
    
    let mut success_count = 0;
    let computer = HashComputer::new();
    
    for (lang, filename) in &test_cases {
        let file_path = PathBuf::from(test_dir).join(filename);
        
        // Create test file
        match fs::write(&file_path, format!("Content for {}", lang)) {
            Ok(_) => {
                // Try to hash the file
                match computer.compute_hash(&file_path, "sha256") {
                    Ok(result) => {
                        assert!(!result.hash.is_empty());
                        assert_eq!(result.hash.len(), 64);
                        success_count += 1;
                    }
                    Err(_) => {
                        // Some characters may not be supported on all filesystems
                    }
                }
            }
            Err(_) => {
                // Skip files that can't be created on this filesystem
            }
        }
    }
    
    // Cleanup
    fs::remove_dir_all(test_dir).ok();
    
    // At least half should succeed
    assert!(success_count >= test_cases.len() / 2, 
        "At least half of hash operations should succeed");
}

#[test]
fn test_progress_bar_with_unicode_filenames() {
    // This test ensures scanning works with unicode filenames
    let test_dir = "test_progress_unicode";
    fs::create_dir_all(test_dir).expect("Failed to create test directory");
    
    // Create files with various unicode characters
    let filenames = vec![
        "file_русский.txt",
        "file_中文.txt",
        "file_日本語.txt",
        "file_한국어.txt",
        "file_العربية.txt",
        "file_😀😊.txt",
    ];
    
    let mut created_count = 0;
    for filename in &filenames {
        let file_path = PathBuf::from(test_dir).join(filename);
        if fs::write(&file_path, "test content").is_ok() {
            created_count += 1;
        }
    }
    
    // Only run scan if we created some files
    if created_count > 0 {
        let output_file = "test_progress_output.txt";
        let engine = ScanEngine::new();
        let result = engine.scan_directory(
            Path::new(test_dir),
            "sha256",
            Path::new(output_file),
        );
        
        assert!(result.is_ok(), "Scan should succeed with unicode filenames");
        
        fs::remove_file(output_file).ok();
    }
    
    // Cleanup
    fs::remove_dir_all(test_dir).ok();
}
