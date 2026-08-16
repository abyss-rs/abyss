//! High-performance native BLAKE3 SIMD delta synchronization engine.
//!
//! Implements the Tridgell-Mackerras (1996) rsync sliding-window delta algorithm
//! accelerated by multi-core Rayon chunking and SIMD BLAKE3 cryptographic verification.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Cursor, Read};

use rayon::prelude::*;

pub const DEFAULT_BLOCK_SIZE: u32 = 2048;
const DELTA_MAGIC: &[u8; 8] = b"ABDEL1\0\0";
const TAG_COPY: u8 = 0x01;
const TAG_LITERAL: u8 = 0x02;
const TAG_END: u8 = 0x00;

/// Fast 32-bit O(1) sliding window rolling checksum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Rollsum {
    s1: u16,
    s2: u16,
    window_size: usize,
}

impl Rollsum {
    #[inline]
    pub const fn new() -> Self {
        Self {
            s1: 0,
            s2: 0,
            window_size: 0,
        }
    }

    #[inline]
    pub fn digest(&self) -> u32 {
        ((self.s2 as u32) << 16) | (self.s1 as u32)
    }

    #[inline]
    pub fn digest_of(bytes: &[u8]) -> u32 {
        let mut r = Self::new();
        r.init(bytes);
        r.digest()
    }

    #[inline]
    pub fn init(&mut self, bytes: &[u8]) {
        let mut s1: u32 = 0;
        let mut s2: u32 = 0;
        let len = bytes.len();
        for (i, &b) in bytes.iter().enumerate() {
            let val = (b as u32).wrapping_add(31);
            s1 = s1.wrapping_add(val);
            s2 = s2.wrapping_add(((len - i) as u32).wrapping_mul(val));
        }
        self.s1 = s1 as u16;
        self.s2 = s2 as u16;
        self.window_size = len;
    }

    #[inline]
    pub fn roll(&mut self, out_byte: u8, in_byte: u8) {
        let out_val = (out_byte as u32).wrapping_add(31);
        let in_val = (in_byte as u32).wrapping_add(31);
        let w = self.window_size as u32;

        let s1 = (self.s1 as u32).wrapping_sub(out_val).wrapping_add(in_val);
        let s2 = (self.s2 as u32)
            .wrapping_sub(w.wrapping_mul(out_val))
            .wrapping_add(s1);

        self.s1 = s1 as u16;
        self.s2 = s2 as u16;
    }
}

/// A block signature entry in the lookup table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEntry {
    pub index: u32,
    pub offset: u64,
    pub length: u32,
    pub blake3_hash: [u8; 16],
}

/// Block signature table computed from a base file.
#[derive(Clone, Debug)]
pub struct Signature {
    pub block_size: u32,
    pub total_len: usize,
    pub table: HashMap<u32, Vec<BlockEntry>>,
}

/// Errors that can occur during delta application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaError {
    InvalidMagic,
    UnexpectedEof,
    CorruptData(String),
    BaseOutOfRange {
        offset: u64,
        length: u32,
        base_len: usize,
    },
    ChecksumMismatch,
    Io(String),
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid delta format header"),
            Self::UnexpectedEof => write!(f, "unexpected end of delta stream"),
            Self::CorruptData(msg) => write!(f, "corrupt delta data: {msg}"),
            Self::BaseOutOfRange {
                offset,
                length,
                base_len,
            } => {
                write!(
                    f,
                    "base offset {offset} + length {length} exceeds base length {base_len}"
                )
            }
            Self::ChecksumMismatch => write!(f, "reconstructed target BLAKE3 checksum mismatch"),
            Self::Io(msg) => write!(f, "I/O error during delta application: {msg}"),
        }
    }
}

impl std::error::Error for DeltaError {}

impl From<io::Error> for DeltaError {
    fn from(err: io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// Compute signature blocks from base data using parallel BLAKE3 chunking.
pub fn compute_signature(data: &[u8], block_size: u32) -> Signature {
    let block_size = block_size.max(16);
    let total_len = data.len();
    if total_len == 0 {
        return Signature {
            block_size,
            total_len: 0,
            table: HashMap::new(),
        };
    }

    let chunks: Vec<(u32, u64, u32, u32, [u8; 16])> = data
        .par_chunks(block_size as usize)
        .enumerate()
        .map(|(idx, chunk)| {
            let roll = Rollsum::digest_of(chunk);
            let b3 = blake3::hash(chunk);
            let mut hash = [0u8; 16];
            hash.copy_from_slice(&b3.as_bytes()[..16]);
            let offset = (idx as u64) * (block_size as u64);
            (idx as u32, offset, chunk.len() as u32, roll, hash)
        })
        .collect();

    let mut table: HashMap<u32, Vec<BlockEntry>> = HashMap::with_capacity(chunks.len());
    for (index, offset, length, roll, blake3_hash) in chunks {
        table.entry(roll).or_default().push(BlockEntry {
            index,
            offset,
            length,
            blake3_hash,
        });
    }

    Signature {
        block_size,
        total_len,
        table,
    }
}

/// Compute delta commands between a base signature and target data.
pub fn compute_delta(signature: &Signature, target: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(target.len().min(1024 * 1024));
    out.extend_from_slice(DELTA_MAGIC);

    if target.is_empty() {
        emit_end(&mut out, 0, &blake3::hash(b""));
        return out;
    }

    let bsize = signature.block_size as usize;
    let tlen = target.len();

    let mut pos = 0;
    let mut literal_start = 0;
    let mut rollsum = Rollsum::new();
    let mut roll_initialized = false;

    while pos + bsize <= tlen {
        let chunk = &target[pos..pos + bsize];
        if !roll_initialized {
            rollsum.init(chunk);
            roll_initialized = true;
        }

        let digest = rollsum.digest();
        let mut matched_entry = None;

        if let Some(entries) = signature.table.get(&digest) {
            let b3 = blake3::hash(chunk);
            let b3_slice = &b3.as_bytes()[..16];
            for entry in entries {
                if entry.length as usize == bsize && entry.blake3_hash == b3_slice {
                    matched_entry = Some(*entry);
                    break;
                }
            }
        }

        if let Some(entry) = matched_entry {
            if literal_start < pos {
                emit_literal(&mut out, &target[literal_start..pos]);
            }
            emit_copy(&mut out, entry.offset, entry.length);
            pos += bsize;
            literal_start = pos;
            roll_initialized = false;
        } else {
            if pos + bsize < tlen {
                rollsum.roll(target[pos], target[pos + bsize]);
            }
            pos += 1;
        }
    }

    if literal_start < tlen {
        emit_literal(&mut out, &target[literal_start..]);
    }

    let target_hash = blake3::hash(target);
    emit_end(&mut out, target.len() as u64, &target_hash);
    out
}

#[inline]
fn emit_literal(out: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    out.push(TAG_LITERAL);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

#[inline]
fn emit_copy(out: &mut Vec<u8>, offset: u64, length: u32) {
    out.push(TAG_COPY);
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
}

#[inline]
fn emit_end(out: &mut Vec<u8>, total_len: u64, hash: &blake3::Hash) {
    out.push(TAG_END);
    out.extend_from_slice(&total_len.to_le_bytes());
    out.extend_from_slice(hash.as_bytes());
}

/// Apply a binary delta onto base data to reconstruct target data.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>, DeltaError> {
    let mut cursor = Cursor::new(delta);
    let mut magic = [0u8; 8];
    cursor.read_exact(&mut magic)?;
    if &magic != DELTA_MAGIC {
        return Err(DeltaError::InvalidMagic);
    }

    let mut output = Vec::new();

    loop {
        let mut tag = [0u8; 1];
        if cursor.read_exact(&mut tag).is_err() {
            return Err(DeltaError::UnexpectedEof);
        }

        match tag[0] {
            TAG_COPY => {
                let mut offset_bytes = [0u8; 8];
                let mut length_bytes = [0u8; 4];
                cursor.read_exact(&mut offset_bytes)?;
                cursor.read_exact(&mut length_bytes)?;
                let offset = u64::from_le_bytes(offset_bytes);
                let length = u32::from_le_bytes(length_bytes);

                let start = offset as usize;
                let end = start.saturating_add(length as usize);
                if end > base.len() {
                    return Err(DeltaError::BaseOutOfRange {
                        offset,
                        length,
                        base_len: base.len(),
                    });
                }
                output.extend_from_slice(&base[start..end]);
            }
            TAG_LITERAL => {
                let mut len_bytes = [0u8; 4];
                cursor.read_exact(&mut len_bytes)?;
                let len = u32::from_le_bytes(len_bytes) as usize;
                let mut buf = vec![0u8; len];
                cursor.read_exact(&mut buf)?;
                output.extend_from_slice(&buf);
            }
            TAG_END => {
                let mut total_len_bytes = [0u8; 8];
                let mut expected_hash = [0u8; 32];
                cursor.read_exact(&mut total_len_bytes)?;
                cursor.read_exact(&mut expected_hash)?;
                let total_len = u64::from_le_bytes(total_len_bytes);

                if output.len() as u64 != total_len {
                    return Err(DeltaError::CorruptData(format!(
                        "length mismatch: expected {total_len}, got {}",
                        output.len()
                    )));
                }

                let actual_hash = blake3::hash(&output);
                if actual_hash.as_bytes() != &expected_hash {
                    return Err(DeltaError::ChecksumMismatch);
                }
                break;
            }
            other => {
                return Err(DeltaError::CorruptData(format!(
                    "unknown delta tag: {other:#x}"
                )));
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollsum_init_matches_rolling() {
        let data = b"The quick brown fox jumps over the lazy dog and runs away!";
        let window = 16;
        for i in 0..data.len() - window {
            let mut init_r = Rollsum::new();
            init_r.init(&data[i..i + window]);

            if i == 0 {
                let mut direct = Rollsum::new();
                direct.init(&data[0..window]);
                assert_eq!(init_r.digest(), direct.digest());
            }
        }

        // Test rolling step by step
        let mut r = Rollsum::new();
        r.init(&data[0..window]);
        for i in 0..data.len() - window - 1 {
            r.roll(data[i], data[i + window]);
            let mut check = Rollsum::new();
            check.init(&data[i + 1..i + 1 + window]);
            assert_eq!(r.digest(), check.digest(), "mismatch at roll step {i}");
        }
    }

    #[test]
    fn empty_file_delta_round_trip() {
        let base = b"";
        let target = b"";
        let sig = compute_signature(base, 16);
        let delta = compute_delta(&sig, target);
        let recon = apply_delta(base, &delta).unwrap();
        assert_eq!(recon, target);
    }

    #[test]
    fn identical_files_produce_all_copies() {
        let data = vec![42u8; 10000];
        let sig = compute_signature(&data, 256);
        let delta = compute_delta(&sig, &data);
        assert!(
            delta.len() < 1000,
            "delta should be compact for identical data"
        );
        let recon = apply_delta(&data, &delta).unwrap();
        assert_eq!(recon, data);
    }

    #[test]
    fn completely_different_files_round_trip() {
        let base = b"AAAAAA AAAAAA AAAAAA AAAAAA AAAAAA AAAAAA AAAAAA AAAAAA AAAAAA";
        let target = b"BBBBBB BBBBBB BBBBBB BBBBBB BBBBBB BBBBBB BBBBBB BBBBBB BBBBBB";
        let sig = compute_signature(base, 16);
        let delta = compute_delta(&sig, target);
        let recon = apply_delta(base, &delta).unwrap();
        assert_eq!(recon, target);
    }

    #[test]
    fn scattered_edits_and_insertions_round_trip() {
        let mut base = vec![0u8; 65536];
        for (i, b) in base.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }

        let mut target = base.clone();
        // Insert in middle
        target.splice(10000..10000, b"INSERTED_CHUNKS_OF_NEW_DATA".iter().copied());
        // Modify a single byte
        target[25000] = 255;
        // Delete a slice
        target.drain(40000..40500);
        // Append at end
        target.extend_from_slice(b"TRAILING_MODIFICATION");

        let sig = compute_signature(&base, 512);
        let delta = compute_delta(&sig, &target);
        assert!(
            delta.len() < target.len() / 2,
            "delta should be much smaller than full target"
        );

        let recon = apply_delta(&base, &delta).unwrap();
        assert_eq!(recon, target);
    }
}
