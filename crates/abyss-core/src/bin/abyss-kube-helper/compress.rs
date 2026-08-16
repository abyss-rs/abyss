use std::io::{self, Write};

use crate::STORED_BLOCK;

use crate::helper_protocol::HelperCompression;

pub(crate) fn write_compressed_block(
    output: &mut impl Write,
    block: &[u8],
    compression: HelperCompression,
) -> io::Result<()> {
    match compression {
        HelperCompression::Lz4 => write_lz4_block(output, block),
        HelperCompression::Brotli => write_brotli_block(output, block),
        HelperCompression::Deflate => write_deflate_block(output, block),
        HelperCompression::None => unreachable!(),
    }
}

pub(crate) fn write_lz4_block(output: &mut impl Write, block: &[u8]) -> io::Result<()> {
    let compressed = lz4_flex::block::compress(block);
    let raw_len = u32::try_from(block.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "LZ4 block is too large"))?;
    if compressed.len() < block.len() {
        let stored_len = u32::try_from(compressed.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "LZ4 block is too large"))?;
        output.write_all(&raw_len.to_be_bytes())?;
        output.write_all(&stored_len.to_be_bytes())?;
        output.write_all(&compressed)
    } else {
        output.write_all(&raw_len.to_be_bytes())?;
        output.write_all(&(raw_len | STORED_BLOCK).to_be_bytes())?;
        output.write_all(block)
    }
}

pub(crate) fn write_brotli_block(output: &mut impl Write, block: &[u8]) -> io::Result<()> {
    let mut writer = brotli::CompressorWriter::new(Vec::new(), 256 * 1024, 1, 24);
    writer.write_all(block)?;
    writer.flush()?;
    let compressed = writer.into_inner();
    let raw_len = u32::try_from(block.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Brotli block is too large"))?;
    output.write_all(&raw_len.to_be_bytes())?;
    if compressed.len() < block.len() {
        let compressed_len = u32::try_from(compressed.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Brotli block is too large"))?;
        output.write_all(&compressed_len.to_be_bytes())?;
        output.write_all(&compressed)
    } else {
        output.write_all(&(raw_len | STORED_BLOCK).to_be_bytes())?;
        output.write_all(block)
    }
}

pub(crate) fn write_deflate_block(output: &mut impl Write, block: &[u8]) -> io::Result<()> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(block)?;
    let compressed = encoder.finish()?;
    let raw_len = u32::try_from(block.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Deflate block is too large"))?;
    output.write_all(&raw_len.to_be_bytes())?;
    if compressed.len() < block.len() {
        let compressed_len = u32::try_from(compressed.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Deflate block is too large")
        })?;
        output.write_all(&compressed_len.to_be_bytes())?;
        output.write_all(&compressed)
    } else {
        output.write_all(&(raw_len | STORED_BLOCK).to_be_bytes())?;
        output.write_all(block)
    }
}
