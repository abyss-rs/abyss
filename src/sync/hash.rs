//! Hashing utilities for sync operations.
//!
//! Provides fast hashing using BLAKE3 and rolling checksums for delta sync.

use anyhow::Result;
use std::io::Read;
use std::path::Path;

/// Hash algorithm type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HashType {
    /// BLAKE3 - fast and secure (default).
    #[default]
    Blake3,
    /// Rolling checksum for delta sync (Rsync-like).
    Rolling,
}

/// A computed file hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHash {
    /// The hash algorithm used.
    pub algorithm: HashType,
    /// The hash value as hex string.
    pub value: String,
    /// File size in bytes.
    pub size: u64,
}

impl FileHash {
    /// Create a new file hash.
    pub fn new(algorithm: HashType, value: String, size: u64) -> Self {
        Self { algorithm, value, size }
    }

    /// Check if two hashes are equal (same content).
    pub fn equals(&self, other: &FileHash) -> bool {
        self.algorithm == other.algorithm && 
        self.value == other.value && 
        self.size == other.size
    }
}

/// Hash bytes using BLAKE3.
pub fn hash_bytes(data: &[u8]) -> String {
    // Use parallel hashing for data > 128KB
    if data.len() > 128 * 1024 {
        let mut hasher = blake3::Hasher::new();
        hasher.update_rayon(data);
        hasher.finalize().to_hex().to_string()
    } else {
        blake3::hash(data).to_hex().to_string()
    }
}

/// Hash a file using BLAKE3 with multicore support for large files.
pub fn hash_file(path: &Path) -> Result<FileHash> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len();
    
    // Use memory-mapped parallel hashing for large files (> 1MB)
    if size > 1024 * 1024 {
        let data = std::fs::read(path)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update_rayon(&data);
        let hash = hasher.finalize();
        
        return Ok(FileHash::new(
            HashType::Blake3,
            hash.to_hex().to_string(),
            size,
        ));
    }
    
    // Standard sequential hashing for smaller files
    let mut file = file;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65536]; // 64KB buffer
    
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    
    let hash = hasher.finalize();
    
    Ok(FileHash::new(
        HashType::Blake3,
        hash.to_hex().to_string(),
        size,
    ))
}

/// Hash a file asynchronously using BLAKE3.
pub async fn hash_file_async(path: &Path) -> Result<FileHash> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || hash_file(&path)).await?
}

/// Rolling checksum for delta sync (Adler32-like).
#[derive(Debug, Clone)]
pub struct RollingChecksum {
    a: u32,
    b: u32,
    window_size: usize,
    buffer: Vec<u8>,
}

impl RollingChecksum {
    /// Create a new rolling checksum with the given window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            a: 0,
            b: 0,
            window_size,
            buffer: Vec::with_capacity(window_size),
        }
    }

    /// Initialize with a block of data.
    pub fn init(&mut self, data: &[u8]) {
        self.a = 0;
        self.b = 0;
        self.buffer.clear();
        
        for (i, &byte) in data.iter().enumerate().take(self.window_size) {
            self.a = self.a.wrapping_add(byte as u32);
            self.b = self.b.wrapping_add((self.window_size - i) as u32 * byte as u32);
            self.buffer.push(byte);
        }
    }

    /// Roll the checksum by removing old_byte and adding new_byte.
    pub fn roll(&mut self, old_byte: u8, new_byte: u8) {
        self.a = self.a.wrapping_sub(old_byte as u32).wrapping_add(new_byte as u32);
        self.b = self.b.wrapping_sub(self.window_size as u32 * old_byte as u32).wrapping_add(self.a);
        
        if !self.buffer.is_empty() {
            self.buffer.remove(0);
        }
        self.buffer.push(new_byte);
    }

    /// Get the current checksum value.
    pub fn value(&self) -> u32 {
        (self.b << 16) | (self.a & 0xFFFF)
    }

    /// Get the current checksum as hex string.
    pub fn hex(&self) -> String {
        format!("{:08x}", self.value())
    }
}

/// Chunk information for delta sync.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Offset in the file.
    pub offset: u64,
    /// Size of the chunk.
    pub size: usize,
    /// Rolling checksum.
    pub rolling: u32,
    /// Strong hash (BLAKE3) of the chunk.
    pub strong_hash: String,
}

/// Compute chunks for a file (for delta sync).
pub fn compute_chunks(path: &Path, chunk_size: usize) -> Result<Vec<Chunk>> {
    let mut file = std::fs::File::open(path)?;
    let mut chunks = Vec::new();
    let mut buffer = vec![0u8; chunk_size];
    let mut offset = 0u64;
    
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        
        let data = &buffer[..bytes_read];
        
        // Compute rolling checksum
        let mut rolling = RollingChecksum::new(bytes_read);
        rolling.init(data);
        
        // Compute strong hash
        let strong_hash = hash_bytes(data);
        
        chunks.push(Chunk {
            offset,
            size: bytes_read,
            rolling: rolling.value(),
            strong_hash,
        });
        
        offset += bytes_read as u64;
    }
    
    Ok(chunks)
}

/// Build a lookup table from chunks for matching.
pub fn build_chunk_lookup(chunks: &[Chunk]) -> std::collections::HashMap<u32, Vec<usize>> {
    let mut lookup = std::collections::HashMap::new();
    
    for (idx, chunk) in chunks.iter().enumerate() {
        lookup.entry(chunk.rolling).or_insert_with(Vec::new).push(idx);
    }
    
    lookup
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hash_bytes() {
        let hash1 = hash_bytes(b"hello world");
        let hash2 = hash_bytes(b"hello world");
        let hash3 = hash_bytes(b"goodbye world");
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // BLAKE3 produces 256-bit hash
    }

    #[test]
    fn test_hash_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"test content").unwrap();
        
        let hash = hash_file(file.path()).unwrap();
        
        assert_eq!(hash.algorithm, HashType::Blake3);
        assert_eq!(hash.size, 12);
        assert_eq!(hash.value.len(), 64);
    }

    #[test]
    fn test_rolling_checksum() {
        let mut rolling = RollingChecksum::new(4);
        
        rolling.init(b"abcd");
        let v1 = rolling.value();
        
        rolling.roll(b'a', b'e');
        let v2 = rolling.value();
        
        // Values should differ after rolling
        assert_ne!(v1, v2);
        
        // Rolling should give same result as fresh init
        let mut fresh = RollingChecksum::new(4);
        fresh.init(b"bcde");
        assert_eq!(rolling.value(), fresh.value());
    }

    #[test]
    fn test_compute_chunks() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world, this is a test file with some content").unwrap();
        
        let chunks = compute_chunks(file.path(), 16).unwrap();
        
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].offset, 0);
    }
}
