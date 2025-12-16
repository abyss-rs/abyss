// Hash computation module
// Provides hash algorithm registry and computation logic

use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use super::error::HashUtilityError;
use memmap2::Mmap;
use std::io::IsTerminal;

/// Trait for hash algorithm implementations
pub trait Hasher: Send {
    /// Update the hasher with new data
    fn update(&mut self, data: &[u8]);
    
    /// Finalize the hash and return the result
    fn finalize(self: Box<Self>) -> Vec<u8>;
    
    /// Get the output size in bytes
    fn output_size(&self) -> usize;
}

/// Information about a hash algorithm
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlgorithmInfo {
    pub name: String,
    pub output_bits: usize,
    pub post_quantum: bool,
    pub cryptographic: bool,
}

// Re-export HashUtilityError as HashError for backward compatibility
pub type HashError = HashUtilityError;

// Wrapper types for hash algorithms
use md5::{Md5, Digest as Md5Digest};
use sha1::{Sha1, Digest as Sha1Digest};
use sha2::{Sha224, Sha256, Sha384, Sha512, Digest as Sha2Digest};
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512, Digest as Sha3Digest};
use blake2::{Blake2b512, Blake2s256, Digest as Blake2Digest};
use blake3::Hasher as Blake3Hasher;

// MD5 wrapper
pub struct Md5Wrapper(Md5);

impl Hasher for Md5Wrapper {
    fn update(&mut self, data: &[u8]) {
        Md5Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Md5Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        16 // 128 bits
    }
}

// SHA1 wrapper
pub struct Sha1Wrapper(Sha1);

impl Hasher for Sha1Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha1Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha1Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        20 // 160 bits
    }
}

// SHA-224 wrapper
pub struct Sha224Wrapper(Sha224);

impl Hasher for Sha224Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        28 // 224 bits
    }
}

// SHA-256 wrapper
pub struct Sha256Wrapper(Sha256);

impl Hasher for Sha256Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        32 // 256 bits
    }
}

// SHA-384 wrapper
pub struct Sha384Wrapper(Sha384);

impl Hasher for Sha384Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        48 // 384 bits
    }
}

// SHA-512 wrapper
pub struct Sha512Wrapper(Sha512);

impl Hasher for Sha512Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        64 // 512 bits
    }
}

// SHA3-224 wrapper
pub struct Sha3_224Wrapper(Sha3_224);

impl Hasher for Sha3_224Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha3Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha3Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        28 // 224 bits
    }
}

// SHA3-256 wrapper
pub struct Sha3_256Wrapper(Sha3_256);

impl Hasher for Sha3_256Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha3Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha3Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        32 // 256 bits
    }
}

// SHA3-384 wrapper
pub struct Sha3_384Wrapper(Sha3_384);

impl Hasher for Sha3_384Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha3Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha3Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        48 // 384 bits
    }
}

// SHA3-512 wrapper
pub struct Sha3_512Wrapper(Sha3_512);

impl Hasher for Sha3_512Wrapper {
    fn update(&mut self, data: &[u8]) {
        Sha3Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Sha3Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        64 // 512 bits
    }
}

// BLAKE2b wrapper
pub struct Blake2b512Wrapper(Blake2b512);

impl Hasher for Blake2b512Wrapper {
    fn update(&mut self, data: &[u8]) {
        Blake2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Blake2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        64 // 512 bits
    }
}

// BLAKE2s wrapper
pub struct Blake2s256Wrapper(Blake2s256);

impl Hasher for Blake2s256Wrapper {
    fn update(&mut self, data: &[u8]) {
        Blake2Digest::update(&mut self.0, data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Blake2Digest::finalize(self.0).to_vec()
    }
    
    fn output_size(&self) -> usize {
        32 // 256 bits
    }
}

// BLAKE3 wrapper
// 
// When the rayon feature is enabled for blake3, this wrapper automatically uses
// multi-threaded hashing for large files, utilizing all available CPU cores.
// This provides significant performance improvements on multi-core systems.
// 
// The blake3 crate's update_rayon() method is always available when the rayon
// feature is enabled, and it automatically parallelizes the hashing across
// multiple CPU cores for better throughput.
pub struct Blake3Wrapper(Blake3Hasher);

impl Hasher for Blake3Wrapper {
    fn update(&mut self, data: &[u8]) {
        // The blake3 crate with rayon feature enabled provides update_rayon()
        // which automatically uses multi-threaded hashing for large inputs.
        // Since we've enabled the rayon feature in Cargo.toml, we can use it directly.
        self.0.update_rayon(data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().as_bytes().to_vec()
    }
    
    fn output_size(&self) -> usize {
        32 // 256 bits
    }
}

// XXH3 wrapper (64-bit non-cryptographic hash)
use xxhash_rust::xxh3::Xxh3 as Xxh3Hasher;

pub struct Xxh3Wrapper(Xxh3Hasher);

impl Hasher for Xxh3Wrapper {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        // XXH3 produces a 64-bit hash
        self.0.digest().to_le_bytes().to_vec()
    }
    
    fn output_size(&self) -> usize {
        8 // 64 bits
    }
}

// XXH128 wrapper (128-bit non-cryptographic hash)
use xxhash_rust::xxh3::Xxh3 as Xxh3HasherBase;

pub struct Xxh128Wrapper(Xxh3HasherBase);

impl Hasher for Xxh128Wrapper {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    
    fn finalize(self: Box<Self>) -> Vec<u8> {
        // XXH128 produces a 128-bit hash
        self.0.digest128().to_le_bytes().to_vec()
    }
    
    fn output_size(&self) -> usize {
        16 // 128 bits
    }
}

/// Registry for hash algorithms
pub struct HashRegistry;

impl HashRegistry {
    /// Get a hasher instance for the specified algorithm
    pub fn get_hasher(algorithm: &str) -> Result<Box<dyn Hasher>, HashError> {
        let alg_lower = algorithm.to_lowercase();
        
        match alg_lower.as_str() {
            "md5" => Ok(Box::new(Md5Wrapper(Md5Digest::new()))),
            "sha1" => Ok(Box::new(Sha1Wrapper(Sha1Digest::new()))),
            "sha224" | "sha-224" => Ok(Box::new(Sha224Wrapper(Sha2Digest::new()))),
            "sha256" | "sha-256" => Ok(Box::new(Sha256Wrapper(Sha2Digest::new()))),
            "sha384" | "sha-384" => Ok(Box::new(Sha384Wrapper(Sha2Digest::new()))),
            "sha512" | "sha-512" => Ok(Box::new(Sha512Wrapper(Sha2Digest::new()))),
            "sha3-224" => Ok(Box::new(Sha3_224Wrapper(Sha3Digest::new()))),
            "sha3-256" => Ok(Box::new(Sha3_256Wrapper(Sha3Digest::new()))),
            "sha3-384" => Ok(Box::new(Sha3_384Wrapper(Sha3Digest::new()))),
            "sha3-512" => Ok(Box::new(Sha3_512Wrapper(Sha3Digest::new()))),
            "blake2b" | "blake2b-512" => Ok(Box::new(Blake2b512Wrapper(Blake2Digest::new()))),
            "blake2s" | "blake2s-256" => Ok(Box::new(Blake2s256Wrapper(Blake2Digest::new()))),
            "blake3" => Ok(Box::new(Blake3Wrapper(Blake3Hasher::new()))),
            "xxh3" => Ok(Box::new(Xxh3Wrapper(Xxh3Hasher::new()))),
            "xxh128" => Ok(Box::new(Xxh128Wrapper(Xxh3HasherBase::new()))),
            _ => Err(HashUtilityError::UnsupportedAlgorithm {
                algorithm: algorithm.to_string(),
            }),
        }
    }
    
    /// List all available hash algorithms
    pub fn list_algorithms() -> Vec<AlgorithmInfo> {
        vec![
            AlgorithmInfo {
                name: "MD5".to_string(),
                output_bits: 128,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA1".to_string(),
                output_bits: 160,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA-224".to_string(),
                output_bits: 224,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA-256".to_string(),
                output_bits: 256,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA-384".to_string(),
                output_bits: 384,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA-512".to_string(),
                output_bits: 512,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA3-224".to_string(),
                output_bits: 224,
                post_quantum: true,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA3-256".to_string(),
                output_bits: 256,
                post_quantum: true,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA3-384".to_string(),
                output_bits: 384,
                post_quantum: true,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "SHA3-512".to_string(),
                output_bits: 512,
                post_quantum: true,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "BLAKE2b-512".to_string(),
                output_bits: 512,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "BLAKE2s-256".to_string(),
                output_bits: 256,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "BLAKE3".to_string(),
                output_bits: 256,
                post_quantum: false,
                cryptographic: true,
            },
            AlgorithmInfo {
                name: "XXH3".to_string(),
                output_bits: 64,
                post_quantum: false,
                cryptographic: false,
            },
            AlgorithmInfo {
                name: "XXH128".to_string(),
                output_bits: 128,
                post_quantum: false,
                cryptographic: false,
            },
        ]
    }
    
    /// Check if an algorithm is post-quantum resistant
    pub fn is_post_quantum(algorithm: &str) -> bool {
        let alg_lower = algorithm.to_lowercase();
        
        // SHA-3 family algorithms are considered post-quantum resistant
        alg_lower.starts_with("sha3-") || 
        alg_lower == "shake128" || 
        alg_lower == "shake256"
    }
}

/// Result of a hash computation
#[derive(Debug, Clone, serde::Serialize)]
pub struct HashResult {
    pub algorithm: String,
    pub hash: String,  // hex-encoded
    pub file_path: PathBuf,
}

/// Hash computer with streaming I/O
pub struct HashComputer {
    buffer_size: usize,
}

// Constants for fast mode sampling
const FAST_MODE_SAMPLE_SIZE: u64 = 100 * 1024 * 1024; // 100MB
const FAST_MODE_THRESHOLD: u64 = 3 * FAST_MODE_SAMPLE_SIZE; // 300MB

// Constants for memory mapping
const MMAP_THRESHOLD: u64 = 2 * 1024 * 1024 * 1024; // 2GB

// Constants for progress bar
const PROGRESS_BAR_THRESHOLD: u64 = 1024 * 1024 * 1024; // 1GB
const PROGRESS_UPDATE_INTERVAL_MS: u64 = 100; // 10 times per second

impl HashComputer {
    /// Create a new HashComputer with default buffer size (1MB)
    pub fn new() -> Self {
        Self {
            buffer_size: 1024 * 1024,
        }
    }
    
    /// Create a new HashComputer with custom buffer size
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self { buffer_size }
    }
    
    /// Compute hash from text string
    pub fn compute_hash_text(
        &self,
        text: &str,
        algorithm: &str,
    ) -> Result<HashResult, HashError> {
        // Get hasher for the specified algorithm
        let mut hasher = HashRegistry::get_hasher(algorithm)?;
        
        // Hash the UTF-8 bytes of the text
        hasher.update(text.as_bytes());
        
        // Finalize hash and convert to hex
        let hash_bytes = hasher.finalize();
        let hash_hex = bytes_to_hex(&hash_bytes);
        
        Ok(HashResult {
            algorithm: algorithm.to_string(),
            hash: hash_hex,
            file_path: PathBuf::from("<text>"), // Use "<text>" to indicate text input
        })
    }
    
    /// Compute multiple hashes from text string in a single pass
    pub fn compute_multiple_hashes_text(
        &self,
        text: &str,
        algorithms: &[String],
    ) -> Result<Vec<HashResult>, HashError> {
        // Get hashers for all specified algorithms
        let mut hashers: Vec<(String, Box<dyn Hasher>)> = Vec::new();
        for algorithm in algorithms {
            let hasher = HashRegistry::get_hasher(algorithm)?;
            hashers.push((algorithm.clone(), hasher));
        }
        
        // Hash the UTF-8 bytes of the text with all hashers
        let text_bytes = text.as_bytes();
        for (_, hasher) in &mut hashers {
            hasher.update(text_bytes);
        }
        
        // Finalize all hashes and collect results
        let mut results = Vec::new();
        for (algorithm, hasher) in hashers {
            let hash_bytes = hasher.finalize();
            let hash_hex = bytes_to_hex(&hash_bytes);
            
            results.push(HashResult {
                algorithm,
                hash: hash_hex,
                file_path: PathBuf::from("<text>"), // Use "<text>" to indicate text input
            });
        }
        
        Ok(results)
    }
    
    /// Compute hash from stdin using streaming I/O
    pub fn compute_hash_stdin(
        &self,
        algorithm: &str,
    ) -> Result<HashResult, HashError> {
        use std::io::{stdin, Read};
        
        // Get hasher for the specified algorithm
        let mut hasher = HashRegistry::get_hasher(algorithm)?;
        
        // Get stdin handle
        let mut stdin = stdin();
        
        // Create buffer for streaming reads
        let mut buffer = vec![0u8; self.buffer_size];
        
        // Stream stdin data through hasher
        loop {
            let bytes_read = stdin.read(&mut buffer).map_err(|e| {
                HashUtilityError::from_io_error(e, "reading from stdin", None)
            })?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        // Finalize hash and convert to hex
        let hash_bytes = hasher.finalize();
        let hash_hex = bytes_to_hex(&hash_bytes);
        
        Ok(HashResult {
            algorithm: algorithm.to_string(),
            hash: hash_hex,
            file_path: PathBuf::from("-"), // Use "-" to indicate stdin
        })
    }
    
    /// Compute hash for a single file using streaming I/O or memory mapping
    /// 
    /// For files smaller than 2GB, uses memory mapping to avoid kernel-to-userspace copy overhead.
    /// For files larger than 2GB, falls back to buffered reading with 1MB buffer.
    /// 
    /// # Safety
    /// 
    /// Memory mapping assumes the file will not be modified by other processes during hashing.
    /// If the file is modified concurrently, the hash result may be inconsistent.
    /// This is acceptable for typical use cases where files are not being actively modified.
    pub fn compute_hash(
        &self,
        path: &Path,
        algorithm: &str,
    ) -> Result<HashResult, HashError> {
        self.compute_hash_with_progress(path, algorithm, false)
    }
    
    /// Compute hash for a single file with optional progress bar
    /// 
    /// If show_progress is true and the file is larger than 1GB and stdout is a TTY,
    /// displays a progress bar that updates 10 times per second.
    pub fn compute_hash_with_progress(
        &self,
        path: &Path,
        algorithm: &str,
        show_progress: bool,
    ) -> Result<HashResult, HashError> {
        // Get hasher for the specified algorithm
        let mut hasher = HashRegistry::get_hasher(algorithm)?;
        
        // Open file for reading with better error context
        let file = File::open(path).map_err(|e| {
            HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
        })?;
        
        // Get file size to determine whether to use memory mapping
        let file_size = file.metadata()
            .map_err(|e| HashUtilityError::from_io_error(e, "reading metadata", Some(path.to_path_buf())))?
            .len();
        
        // Determine if we should show progress bar
        let should_show_progress = show_progress 
            && file_size > PROGRESS_BAR_THRESHOLD 
            && std::io::stdout().is_terminal();
        
        // Use memory mapping for files smaller than 2GB
        if file_size > 0 && file_size < MMAP_THRESHOLD {
            // Try to memory map the file
            match unsafe { Mmap::map(&file) } {
                Ok(mmap) => {
                    // Hash the entire mapped file in one go
                    // Note: Progress bar not shown for mmap as it's very fast
                    hasher.update(&mmap[..]);
                }
                Err(_) => {
                    // Fall back to buffered reading if mmap fails
                    if should_show_progress {
                        self.hash_with_buffered_io_progress(&mut hasher, file, path, file_size)?;
                    } else {
                        self.hash_with_buffered_io(&mut hasher, file, path)?;
                    }
                }
            }
        } else {
            // Use buffered reading for large files (>2GB) or empty files
            if should_show_progress {
                self.hash_with_buffered_io_progress(&mut hasher, file, path, file_size)?;
            } else {
                self.hash_with_buffered_io(&mut hasher, file, path)?;
            }
        }
        
        // Finalize hash and convert to hex
        let hash_bytes = hasher.finalize();
        let hash_hex = bytes_to_hex(&hash_bytes);
        
        Ok(HashResult {
            algorithm: algorithm.to_string(),
            hash: hash_hex,
            file_path: path.to_path_buf(),
        })
    }
    
    /// Helper method to hash a file using buffered I/O
    fn hash_with_buffered_io(
        &self,
        hasher: &mut Box<dyn Hasher>,
        mut file: File,
        path: &Path,
    ) -> Result<(), HashError> {
        let mut buffer = vec![0u8; self.buffer_size];
        
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
            })?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(())
    }
    
    /// Helper method to hash a file using buffered I/O with progress bar
    fn hash_with_buffered_io_progress(
        &self,
        hasher: &mut Box<dyn Hasher>,
        mut file: File,
        path: &Path,
        file_size: u64,
    ) -> Result<(), HashError> {
        use indicatif::{ProgressBar, ProgressStyle};
        use std::time::{Duration, Instant};
        
        // Create progress bar
        let pb = ProgressBar::new(file_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-")
        );
        pb.set_message(format!("Hashing: {}", path.display()));
        
        let mut buffer = vec![0u8; self.buffer_size];
        let mut bytes_processed = 0u64;
        let mut last_update = Instant::now();
        let update_interval = Duration::from_millis(PROGRESS_UPDATE_INTERVAL_MS);
        
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                pb.finish_and_clear();
                HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
            })?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
            bytes_processed += bytes_read as u64;
            
            // Update progress bar at the specified interval
            let now = Instant::now();
            if now.duration_since(last_update) >= update_interval {
                pb.set_position(bytes_processed);
                last_update = now;
            }
        }
        
        // Finish progress bar
        pb.finish_and_clear();
        
        Ok(())
    }
    
    /// Compute multiple hashes from stdin in a single pass
    pub fn compute_multiple_hashes_stdin(
        &self,
        algorithms: &[String],
    ) -> Result<Vec<HashResult>, HashError> {
        use std::io::{stdin, Read};
        
        // Get hashers for all specified algorithms
        let mut hashers: Vec<(String, Box<dyn Hasher>)> = Vec::new();
        for algorithm in algorithms {
            let hasher = HashRegistry::get_hasher(algorithm)?;
            hashers.push((algorithm.clone(), hasher));
        }
        
        // Get stdin handle
        let mut stdin = stdin();
        
        // Create buffer for streaming reads
        let mut buffer = vec![0u8; self.buffer_size];
        
        // Stream stdin data through all hashers in single pass
        loop {
            let bytes_read = stdin.read(&mut buffer).map_err(|e| {
                HashUtilityError::from_io_error(e, "reading from stdin", None)
            })?;
            if bytes_read == 0 {
                break;
            }
            
            // Update all hashers with the same data
            for (_, hasher) in &mut hashers {
                hasher.update(&buffer[..bytes_read]);
            }
        }
        
        // Finalize all hashes and collect results
        let mut results = Vec::new();
        for (algorithm, hasher) in hashers {
            let hash_bytes = hasher.finalize();
            let hash_hex = bytes_to_hex(&hash_bytes);
            
            results.push(HashResult {
                algorithm,
                hash: hash_hex,
                file_path: PathBuf::from("-"), // Use "-" to indicate stdin
            });
        }
        
        Ok(results)
    }
    
    /// Compute multiple hashes for a single file in a single pass
    /// 
    /// For files smaller than 2GB, uses memory mapping to avoid kernel-to-userspace copy overhead.
    /// For files larger than 2GB, falls back to buffered reading with 1MB buffer.
    /// 
    /// # Safety
    /// 
    /// Memory mapping assumes the file will not be modified by other processes during hashing.
    /// If the file is modified concurrently, the hash results may be inconsistent.
    pub fn compute_multiple_hashes(
        &self,
        path: &Path,
        algorithms: &[String],
    ) -> Result<Vec<HashResult>, HashError> {
        self.compute_multiple_hashes_with_progress(path, algorithms, false)
    }
    
    /// Compute multiple hashes for a single file with optional progress bar
    /// 
    /// If show_progress is true and the file is larger than 1GB and stdout is a TTY,
    /// displays a progress bar that updates 10 times per second.
    pub fn compute_multiple_hashes_with_progress(
        &self,
        path: &Path,
        algorithms: &[String],
        show_progress: bool,
    ) -> Result<Vec<HashResult>, HashError> {
        // Get hashers for all specified algorithms
        let mut hashers: Vec<(String, Box<dyn Hasher>)> = Vec::new();
        for algorithm in algorithms {
            let hasher = HashRegistry::get_hasher(algorithm)?;
            hashers.push((algorithm.clone(), hasher));
        }
        
        // Open file for reading with better error context
        let file = File::open(path).map_err(|e| {
            HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
        })?;
        
        // Get file size to determine whether to use memory mapping
        let file_size = file.metadata()
            .map_err(|e| HashUtilityError::from_io_error(e, "reading metadata", Some(path.to_path_buf())))?
            .len();
        
        // Determine if we should show progress bar
        let should_show_progress = show_progress 
            && file_size > PROGRESS_BAR_THRESHOLD 
            && std::io::stdout().is_terminal();
        
        // Use memory mapping for files smaller than 2GB
        if file_size > 0 && file_size < MMAP_THRESHOLD {
            // Try to memory map the file
            match unsafe { Mmap::map(&file) } {
                Ok(mmap) => {
                    // Hash the entire mapped file with all hashers
                    // Note: Progress bar not shown for mmap as it's very fast
                    for (_, hasher) in &mut hashers {
                        hasher.update(&mmap[..]);
                    }
                }
                Err(_) => {
                    // Fall back to buffered reading if mmap fails
                    if should_show_progress {
                        self.hash_multiple_with_buffered_io_progress(&mut hashers, file, path, file_size)?;
                    } else {
                        self.hash_multiple_with_buffered_io(&mut hashers, file, path)?;
                    }
                }
            }
        } else {
            // Use buffered reading for large files (>2GB) or empty files
            if should_show_progress {
                self.hash_multiple_with_buffered_io_progress(&mut hashers, file, path, file_size)?;
            } else {
                self.hash_multiple_with_buffered_io(&mut hashers, file, path)?;
            }
        }
        
        // Finalize all hashes and collect results
        let mut results = Vec::new();
        for (algorithm, hasher) in hashers {
            let hash_bytes = hasher.finalize();
            let hash_hex = bytes_to_hex(&hash_bytes);
            
            results.push(HashResult {
                algorithm,
                hash: hash_hex,
                file_path: path.to_path_buf(),
            });
        }
        
        Ok(results)
    }
    
    /// Helper method to hash a file with multiple hashers using buffered I/O
    fn hash_multiple_with_buffered_io(
        &self,
        hashers: &mut [(String, Box<dyn Hasher>)],
        mut file: File,
        path: &Path,
    ) -> Result<(), HashError> {
        let mut buffer = vec![0u8; self.buffer_size];
        
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
            })?;
            if bytes_read == 0 {
                break;
            }
            
            // Update all hashers with the same data
            for (_, hasher) in hashers.iter_mut() {
                hasher.update(&buffer[..bytes_read]);
            }
        }
        
        Ok(())
    }
    
    /// Helper method to hash a file with multiple hashers using buffered I/O with progress bar
    fn hash_multiple_with_buffered_io_progress(
        &self,
        hashers: &mut [(String, Box<dyn Hasher>)],
        mut file: File,
        path: &Path,
        file_size: u64,
    ) -> Result<(), HashError> {
        use indicatif::{ProgressBar, ProgressStyle};
        use std::time::{Duration, Instant};
        
        // Create progress bar
        let pb = ProgressBar::new(file_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-")
        );
        pb.set_message(format!("Hashing: {}", path.display()));
        
        let mut buffer = vec![0u8; self.buffer_size];
        let mut bytes_processed = 0u64;
        let mut last_update = Instant::now();
        let update_interval = Duration::from_millis(PROGRESS_UPDATE_INTERVAL_MS);
        
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                pb.finish_and_clear();
                HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
            })?;
            if bytes_read == 0 {
                break;
            }
            
            // Update all hashers with the same data
            for (_, hasher) in hashers.iter_mut() {
                hasher.update(&buffer[..bytes_read]);
            }
            
            bytes_processed += bytes_read as u64;
            
            // Update progress bar at the specified interval
            let now = Instant::now();
            if now.duration_since(last_update) >= update_interval {
                pb.set_position(bytes_processed);
                last_update = now;
            }
        }
        
        // Finish progress bar
        pb.finish_and_clear();
        
        Ok(())
    }
    
    /// Compute hash for a file using fast mode (sampling strategy)
    /// 
    /// For files larger than 300MB, samples three 100MB regions:
    /// - First 100MB
    /// - Middle 100MB (centered at file_size/2)
    /// - Last 100MB
    /// 
    /// For files smaller than 300MB, uses the full file.
    pub fn compute_hash_fast(
        &self,
        path: &Path,
        algorithm: &str,
    ) -> Result<HashResult, HashError> {
        
        // Get hasher for the specified algorithm
        let mut hasher = HashRegistry::get_hasher(algorithm)?;
        
        // Open file for reading with better error context
        let mut file = File::open(path).map_err(|e| {
            HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
        })?;
        
        // Get file size
        let file_size = file.metadata()
            .map_err(|e| HashUtilityError::from_io_error(e, "reading metadata", Some(path.to_path_buf())))?
            .len();
        
        // If file is smaller than threshold, hash the entire file
        if file_size < FAST_MODE_THRESHOLD {
            let mut buffer = vec![0u8; self.buffer_size];
            loop {
                let bytes_read = file.read(&mut buffer).map_err(|e| {
                    HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf()))
                })?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
        } else {
            // Sample three regions: first 100MB, middle 100MB, last 100MB
            
            // Read first 100MB
            self.read_region(&mut file, &mut hasher, 0, FAST_MODE_SAMPLE_SIZE, path)?;
            
            // Calculate middle region: centered at file_size/2
            let middle_start = (file_size / 2).saturating_sub(FAST_MODE_SAMPLE_SIZE / 2);
            self.read_region(&mut file, &mut hasher, middle_start, FAST_MODE_SAMPLE_SIZE, path)?;
            
            // Read last 100MB
            let last_start = file_size.saturating_sub(FAST_MODE_SAMPLE_SIZE);
            self.read_region(&mut file, &mut hasher, last_start, FAST_MODE_SAMPLE_SIZE, path)?;
        }
        
        // Finalize hash and convert to hex
        let hash_bytes = hasher.finalize();
        let hash_hex = bytes_to_hex(&hash_bytes);
        
        Ok(HashResult {
            algorithm: algorithm.to_string(),
            hash: hash_hex,
            file_path: path.to_path_buf(),
        })
    }
    
    /// Helper function to read a specific region of a file
    fn read_region(
        &self,
        file: &mut File,
        hasher: &mut Box<dyn Hasher>,
        start: u64,
        length: u64,
        path: &Path,
    ) -> Result<(), HashError> {
        
        // Seek to the start position
        file.seek(std::io::SeekFrom::Start(start))
            .map_err(|e| HashUtilityError::from_io_error(e, "seeking", Some(path.to_path_buf())))?;
        
        // Read up to 'length' bytes
        let mut buffer = vec![0u8; self.buffer_size];
        let mut bytes_remaining = length;
        
        while bytes_remaining > 0 {
            let to_read = std::cmp::min(bytes_remaining, buffer.len() as u64) as usize;
            let bytes_read = file.read(&mut buffer[..to_read])
                .map_err(|e| HashUtilityError::from_io_error(e, "reading", Some(path.to_path_buf())))?;
            
            if bytes_read == 0 {
                break; // End of file
            }
            
            hasher.update(&buffer[..bytes_read]);
            bytes_remaining -= bytes_read as u64;
        }
        
        Ok(())
    }
}

impl Default for HashComputer {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert bytes to hexadecimal string
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// Tests moved to tests/hash/hash_tests.rs

