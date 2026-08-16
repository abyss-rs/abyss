use std::io::{self, Read};

use crate::{BROTLI_BLOCK, LZ4_BLOCK, STORED_BLOCK};

pub(crate) struct Lz4BlockReader<'a, R> {
    input: &'a mut R,
    decoded: Vec<u8>,
    offset: usize,
    pub(crate) remaining: u64,
}

impl<'a, R: Read> Lz4BlockReader<'a, R> {
    pub(crate) fn new(input: &'a mut R, remaining: u64) -> Self {
        Self {
            input,
            decoded: Vec::new(),
            offset: 0,
            remaining,
        }
    }

    fn next_block(&mut self) -> io::Result<()> {
        let mut header = [0_u8; 8];
        self.input.read_exact(&mut header)?;
        let raw_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let encoded = u32::from_be_bytes(header[4..].try_into().unwrap());
        let stored = encoded & STORED_BLOCK != 0;
        let encoded_len = (encoded & !STORED_BLOCK) as usize;
        if raw_len == 0
            || raw_len > LZ4_BLOCK
            || encoded_len == 0
            || encoded_len > LZ4_BLOCK.saturating_add(1024)
            || raw_len as u64 > self.remaining
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid LZ4 transport block",
            ));
        }
        let mut data = vec![0_u8; encoded_len];
        self.input.read_exact(&mut data)?;
        self.decoded = if stored {
            if encoded_len != raw_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stored LZ4 block length differs from raw length",
                ));
            }
            data
        } else {
            lz4_flex::block::decompress(&data, raw_len)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
        };
        self.offset = 0;
        self.remaining -= raw_len as u64;
        Ok(())
    }
}

impl<R: Read> Read for Lz4BlockReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.offset == self.decoded.len() {
            if self.remaining == 0 {
                return Ok(0);
            }
            self.next_block()?;
        }
        let length = output.len().min(self.decoded.len() - self.offset);
        output[..length].copy_from_slice(&self.decoded[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

pub(crate) struct BrotliBlockReader<'a, R> {
    input: &'a mut R,
    decoded: Vec<u8>,
    offset: usize,
    pub(crate) remaining: u64,
}

impl<'a, R: Read> BrotliBlockReader<'a, R> {
    pub(crate) fn new(input: &'a mut R, remaining: u64) -> Self {
        Self {
            input,
            decoded: Vec::new(),
            offset: 0,
            remaining,
        }
    }

    fn next_block(&mut self) -> io::Result<()> {
        let mut header = [0_u8; 8];
        self.input.read_exact(&mut header)?;
        let raw_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let encoded = u32::from_be_bytes(header[4..].try_into().unwrap());
        let stored = encoded & STORED_BLOCK != 0;
        let encoded_len = (encoded & !STORED_BLOCK) as usize;
        if raw_len == 0
            || raw_len > BROTLI_BLOCK
            || encoded_len == 0
            || encoded_len > BROTLI_BLOCK.saturating_add(1024)
            || raw_len as u64 > self.remaining
            || (stored && encoded_len != raw_len)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Brotli transport block",
            ));
        }
        let mut data = vec![0_u8; encoded_len];
        self.input.read_exact(&mut data)?;
        self.decoded = if stored {
            data
        } else {
            let mut decoder = brotli::Decompressor::new(io::Cursor::new(data), 256 * 1024);
            let mut decoded = Vec::with_capacity(raw_len);
            decoder.read_to_end(&mut decoded)?;
            if decoded.len() != raw_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Brotli transport block length differs from raw length",
                ));
            }
            decoded
        };
        self.offset = 0;
        self.remaining -= raw_len as u64;
        Ok(())
    }
}

impl<R: Read> Read for BrotliBlockReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.offset == self.decoded.len() {
            if self.remaining == 0 {
                return Ok(0);
            }
            self.next_block()?;
        }
        let length = output.len().min(self.decoded.len() - self.offset);
        output[..length].copy_from_slice(&self.decoded[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

pub(crate) struct DeflateBlockReader<'a, R> {
    input: &'a mut R,
    decoded: Vec<u8>,
    offset: usize,
    pub(crate) remaining: u64,
}

impl<'a, R: Read> DeflateBlockReader<'a, R> {
    pub(crate) fn new(input: &'a mut R, remaining: u64) -> Self {
        Self {
            input,
            decoded: Vec::new(),
            offset: 0,
            remaining,
        }
    }

    fn next_block(&mut self) -> io::Result<()> {
        let mut header = [0_u8; 8];
        self.input.read_exact(&mut header)?;
        let raw_len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let encoded = u32::from_be_bytes(header[4..].try_into().unwrap());
        let stored = encoded & STORED_BLOCK != 0;
        let encoded_len = (encoded & !STORED_BLOCK) as usize;
        if raw_len == 0
            || raw_len > BROTLI_BLOCK
            || encoded_len == 0
            || encoded_len > BROTLI_BLOCK.saturating_add(1024)
            || raw_len as u64 > self.remaining
            || (stored && encoded_len != raw_len)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Deflate transport block",
            ));
        }
        let mut data = vec![0_u8; encoded_len];
        self.input.read_exact(&mut data)?;
        self.decoded = if stored {
            data
        } else {
            let mut decoder = flate2::read::DeflateDecoder::new(io::Cursor::new(data));
            let mut decoded = Vec::with_capacity(raw_len);
            decoder.read_to_end(&mut decoded)?;
            if decoded.len() != raw_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Deflate transport block length differs from raw length",
                ));
            }
            decoded
        };
        self.offset = 0;
        self.remaining -= raw_len as u64;
        Ok(())
    }
}

impl<R: Read> Read for DeflateBlockReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.offset == self.decoded.len() {
            if self.remaining == 0 {
                return Ok(0);
            }
            self.next_block()?;
        }
        let length = output.len().min(self.decoded.len() - self.offset);
        output[..length].copy_from_slice(&self.decoded[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}
