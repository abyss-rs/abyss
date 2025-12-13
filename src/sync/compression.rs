//! Stream compression for sync transfers.
//!
//! Provides transparent compression/decompression using gzip or zstd.

use anyhow::Result;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use std::io::{Read, Write};

/// Compression algorithm type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    /// No compression.
    #[default]
    None,
    /// Gzip compression (widely compatible).
    Gzip,
    /// Zstd compression (fast, good ratio).
    Zstd,
}

impl CompressionType {
    /// Get a human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Zstd => "zstd",
        }
    }

    /// Detect compression from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "gz" | "gzip" => Some(Self::Gzip),
            "zst" | "zstd" => Some(Self::Zstd),
            _ => None,
        }
    }

    /// Check if a file is already compressed based on extension.
    pub fn is_already_compressed(path: &str) -> bool {
        let lower = path.to_lowercase();
        // Common compressed formats
        lower.ends_with(".gz") ||
        lower.ends_with(".gzip") ||
        lower.ends_with(".zst") ||
        lower.ends_with(".zstd") ||
        lower.ends_with(".zip") ||
        lower.ends_with(".7z") ||
        lower.ends_with(".rar") ||
        lower.ends_with(".xz") ||
        lower.ends_with(".bz2") ||
        lower.ends_with(".lz4") ||
        lower.ends_with(".lzma") ||
        // Compressed media
        lower.ends_with(".jpg") ||
        lower.ends_with(".jpeg") ||
        lower.ends_with(".png") ||
        lower.ends_with(".gif") ||
        lower.ends_with(".webp") ||
        lower.ends_with(".mp3") ||
        lower.ends_with(".mp4") ||
        lower.ends_with(".mkv") ||
        lower.ends_with(".avi") ||
        lower.ends_with(".mov") ||
        lower.ends_with(".webm") ||
        lower.ends_with(".flac") ||
        lower.ends_with(".aac") ||
        lower.ends_with(".ogg") ||
        // Other compressed formats
        lower.ends_with(".pdf") ||
        lower.ends_with(".docx") ||
        lower.ends_with(".xlsx") ||
        lower.ends_with(".pptx")
    }
}

/// Compression level (1-9, where 1 is fastest-lowest and 9 is slowest-highest).
#[derive(Debug, Clone, Copy)]
pub struct CompressionLevel(u8);

impl Default for CompressionLevel {
    fn default() -> Self {
        Self(3) // Balanced default
    }
}

impl CompressionLevel {
    /// Create a new compression level (clamped to 1-9).
    pub fn new(level: u8) -> Self {
        Self(level.clamp(1, 9))
    }

    /// Fastest compression (level 1).
    pub fn fast() -> Self {
        Self(1)
    }

    /// Balanced compression (level 3).
    pub fn balanced() -> Self {
        Self(3)
    }

    /// Maximum compression (level 9).
    pub fn max() -> Self {
        Self(9)
    }

    /// Get the level value.
    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Compressed reader wrapper.
pub struct CompressedReader<R: Read> {
    inner: CompressedReaderInner<R>,
}

enum CompressedReaderInner<R: Read> {
    None(R),
    Gzip(GzDecoder<R>),
    Zstd(zstd::Decoder<'static, std::io::BufReader<R>>),
}

impl<R: Read> CompressedReader<R> {
    /// Create a new compressed reader.
    pub fn new(reader: R, compression: CompressionType) -> Result<Self> {
        let inner = match compression {
            CompressionType::None => CompressedReaderInner::None(reader),
            CompressionType::Gzip => CompressedReaderInner::Gzip(GzDecoder::new(reader)),
            CompressionType::Zstd => {
                let decoder = zstd::Decoder::new(reader)?;
                CompressedReaderInner::Zstd(decoder)
            }
        };
        Ok(Self { inner })
    }
}

impl<R: Read> Read for CompressedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.inner {
            CompressedReaderInner::None(r) => r.read(buf),
            CompressedReaderInner::Gzip(r) => r.read(buf),
            CompressedReaderInner::Zstd(r) => r.read(buf),
        }
    }
}

/// Compressed writer wrapper.
pub struct CompressedWriter<W: Write> {
    inner: CompressedWriterInner<W>,
}

enum CompressedWriterInner<W: Write> {
    None(W),
    Gzip(GzEncoder<W>),
    Zstd(zstd::Encoder<'static, W>),
}

impl<W: Write> CompressedWriter<W> {
    /// Create a new compressed writer.
    pub fn new(writer: W, compression: CompressionType, level: CompressionLevel) -> Result<Self> {
        let inner = match compression {
            CompressionType::None => CompressedWriterInner::None(writer),
            CompressionType::Gzip => {
                let gzip_level = GzipCompression::new(level.value() as u32);
                CompressedWriterInner::Gzip(GzEncoder::new(writer, gzip_level))
            }
            CompressionType::Zstd => {
                let encoder = zstd::Encoder::new(writer, level.value() as i32)?;
                CompressedWriterInner::Zstd(encoder)
            }
        };
        Ok(Self { inner })
    }

    /// Finish writing and get the inner writer back.
    pub fn finish(self) -> Result<W> {
        match self.inner {
            CompressedWriterInner::None(w) => Ok(w),
            CompressedWriterInner::Gzip(w) => Ok(w.finish()?),
            CompressedWriterInner::Zstd(w) => Ok(w.finish()?),
        }
    }
}

impl<W: Write> Write for CompressedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.inner {
            CompressedWriterInner::None(w) => w.write(buf),
            CompressedWriterInner::Gzip(w) => w.write(buf),
            CompressedWriterInner::Zstd(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.inner {
            CompressedWriterInner::None(w) => w.flush(),
            CompressedWriterInner::Gzip(w) => w.flush(),
            CompressedWriterInner::Zstd(w) => w.flush(),
        }
    }
}

/// Compress data in memory.
pub fn compress(data: &[u8], compression: CompressionType, level: CompressionLevel) -> Result<Vec<u8>> {
    if compression == CompressionType::None {
        return Ok(data.to_vec());
    }
    
    let mut output = Vec::new();
    let mut writer = CompressedWriter::new(&mut output, compression, level)?;
    writer.write_all(data)?;
    writer.finish()?;
    
    Ok(output)
}

/// Decompress data in memory.
pub fn decompress(data: &[u8], compression: CompressionType) -> Result<Vec<u8>> {
    if compression == CompressionType::None {
        return Ok(data.to_vec());
    }
    
    let mut output = Vec::new();
    let mut reader = CompressedReader::new(data, compression)?;
    reader.read_to_end(&mut output)?;
    
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gzip_roundtrip() {
        let original = b"Hello, world! This is a test of compression.";
        
        let compressed = compress(original, CompressionType::Gzip, CompressionLevel::balanced()).unwrap();
        let decompressed = decompress(&compressed, CompressionType::Gzip).unwrap();
        
        assert_eq!(original.as_slice(), decompressed.as_slice());
        assert!(compressed.len() < original.len() * 2); // Sanity check
    }

    #[test]
    fn test_zstd_roundtrip() {
        let original = b"Hello, world! This is a test of compression.";
        
        let compressed = compress(original, CompressionType::Zstd, CompressionLevel::balanced()).unwrap();
        let decompressed = decompress(&compressed, CompressionType::Zstd).unwrap();
        
        assert_eq!(original.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_no_compression() {
        let original = b"Hello, world!";
        
        let result = compress(original, CompressionType::None, CompressionLevel::default()).unwrap();
        
        assert_eq!(original.as_slice(), result.as_slice());
    }

    #[test]
    fn test_already_compressed_detection() {
        assert!(CompressionType::is_already_compressed("file.gz"));
        assert!(CompressionType::is_already_compressed("photo.jpg"));
        assert!(CompressionType::is_already_compressed("video.mp4"));
        assert!(CompressionType::is_already_compressed("archive.zip"));
        
        assert!(!CompressionType::is_already_compressed("document.txt"));
        assert!(!CompressionType::is_already_compressed("source.rs"));
        assert!(!CompressionType::is_already_compressed("data.json"));
    }
}
