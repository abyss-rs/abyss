use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use brotli::Decompressor;
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use lz4_flex::frame::{FrameDecoder, FrameEncoder};
use lzma_rust2::{LzipReader, XzOptions, XzReader, XzWriter};
use unarc_rs::unified::ArchiveFormat as UnifiedFormat;

use crate::Error;
use crate::archive::reader::normalize_path;
use crate::archive::types::{
    ArchiveCreateOptions, ArchiveFormat, ArchiveMember, ArchiveOpenError, CompressionMethod,
};
use crate::archive::writer::{CREATE_IO_BUFFER, CreateEntry, create_zstd_encoder, progress_reader};
use crate::progress::CopyStats;

pub(crate) const ZSTD_SKIPPABLE_MAGIC_TOC: u32 = 0x184D2A50;
pub(crate) const ZSTD_SKIPPABLE_MAGIC_PTR: u32 = 0x184D2A51;
pub(crate) const TAR_ZST_TOC_MAGIC: &[u8; 8] = b"ABYSSTOC";
pub(crate) const TAR_ZST_PTR_MAGIC: &[u8; 8] = b"ABYSSPTR";
pub(crate) const TAR_ZST_TOC_VERSION: u32 = 1;
pub(crate) const TAR_ZST_PTR_PAYLOAD_LEN: u32 = 16;

pub(crate) fn write_tar_archive(
    sink: impl Write,
    entries: &[CreateEntry],
    options: &ArchiveCreateOptions,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    match options.method {
        CompressionMethod::Store => {
            append_tar_entries(sink, entries, cancelled, stats)?;
        }
        CompressionMethod::Zstd => {
            let encoder = create_zstd_encoder(sink, i32::from(options.level), options.threads)?;
            append_tar_entries(encoder, entries, cancelled, stats)?
                .finish()
                .map_err(|error| Error::message(format!("finalize zstd stream: {error}")))?;
        }
        CompressionMethod::Gzip => {
            let encoder = flate2::write::GzEncoder::new(
                sink,
                flate2::Compression::new(u32::from(options.level.min(9))),
            );
            append_tar_entries(encoder, entries, cancelled, stats)?
                .finish()
                .map_err(|error| Error::message(format!("finalize gzip stream: {error}")))?;
        }
        CompressionMethod::Bzip2 => {
            let encoder = bzip2::write::BzEncoder::new(
                sink,
                bzip2::Compression::new(u32::from(options.level.clamp(1, 9))),
            );
            append_tar_entries(encoder, entries, cancelled, stats)?
                .finish()
                .map_err(|error| Error::message(format!("finalize bzip2 stream: {error}")))?;
        }
        CompressionMethod::Xz => {
            let encoder = XzWriter::new(sink, XzOptions::with_preset(u32::from(options.level)))
                .map_err(|error| Error::message(format!("initialize xz encoder: {error}")))?;
            append_tar_entries(encoder, entries, cancelled, stats)?
                .finish()
                .map_err(|error| Error::message(format!("finalize xz stream: {error}")))?;
        }
        CompressionMethod::Lz4 => {
            append_tar_entries(FrameEncoder::new(sink), entries, cancelled, stats)?
                .finish()
                .map_err(|error| Error::message(format!("finalize lz4 stream: {error}")))?;
        }
        CompressionMethod::Brotli => {
            let encoder = brotli::CompressorWriter::new(
                sink,
                CREATE_IO_BUFFER,
                u32::from(options.level.min(11)),
                22,
            );
            let mut encoder = append_tar_entries(encoder, entries, cancelled, stats)?;
            encoder
                .flush()
                .map_err(|error| Error::message(format!("finalize brotli stream: {error}")))?;
        }
        _ => return Err(Error::message("method is not valid for a tar archive")),
    }
    Ok(())
}

pub(crate) fn append_tar_entries<W: Write>(
    sink: W,
    entries: &[CreateEntry],
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<W, Error> {
    let mut builder = tar::Builder::new(sink);
    for entry in entries {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        // Use append_data (not Header::set_path) so GNU long-name extensions
        // cover paths that exceed the 100-byte ustar name field.
        let mut header = tar::Header::new_gnu();
        if entry.is_directory {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            if let Err(error) = builder.append_data(&mut header, &entry.name, io::empty()) {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                return Err(Error::message(format!("archive {}: {error}", entry.name)));
            }
            stats.complete_object(&entry.source);
            continue;
        }
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(entry.size);
        header.set_mode(0o644);
        let reader = progress_reader(entry, cancelled, stats);
        if let Err(error) = builder.append_data(&mut header, &entry.name, reader) {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            return Err(Error::message(format!("archive {}: {error}", entry.name)));
        }
    }
    builder
        .into_inner()
        .map_err(|error| Error::message(format!("finalize tar archive: {error}")))
}

pub(crate) fn list_tar_zstd(path: &Path) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    if let Some(members) = read_tar_zstd_toc(path) {
        return Ok(members);
    }
    list_compressed_tar(path, ArchiveFormat::TarZstd)
}

pub(crate) fn list_compressed_tar(
    path: &Path,
    format: ArchiveFormat,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let reader = compressed_reader(path, format)
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let mut archive = tar::Archive::new(reader);
    let mut members = Vec::new();
    for entry in archive
        .entries()
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?
    {
        let entry = entry.map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
        let path = entry
            .path()
            .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
        let Some(path) = normalize_path(&path) else {
            continue;
        };
        members.push(ArchiveMember {
            path,
            size: entry.size(),
            is_directory: entry.header().entry_type().is_dir(),
        });
    }
    Ok(members)
}

pub(crate) fn append_tar_zstd_toc(file: &mut File, members: &[ArchiveMember]) -> io::Result<()> {
    let toc_offset = file.seek(SeekFrom::End(0))?;
    let payload = encode_tar_zstd_toc(members)?;
    write_zstd_skippable(file, ZSTD_SKIPPABLE_MAGIC_TOC, &payload)?;

    let mut pointer = Vec::with_capacity(TAR_ZST_PTR_PAYLOAD_LEN as usize);
    pointer.extend_from_slice(TAR_ZST_PTR_MAGIC);
    pointer.extend_from_slice(&toc_offset.to_le_bytes());
    write_zstd_skippable(file, ZSTD_SKIPPABLE_MAGIC_PTR, &pointer)?;
    Ok(())
}

pub(crate) fn write_zstd_skippable(file: &mut File, magic: u32, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "TOC payload too large"))?;
    file.write_all(&magic.to_le_bytes())?;
    file.write_all(&len.to_le_bytes())?;
    file.write_all(payload)?;
    Ok(())
}

pub(crate) fn encode_tar_zstd_toc(members: &[ArchiveMember]) -> io::Result<Vec<u8>> {
    let count = u32::try_from(members.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many TOC members"))?;
    let mut payload = Vec::new();
    payload.extend_from_slice(TAR_ZST_TOC_MAGIC);
    payload.extend_from_slice(&TAR_ZST_TOC_VERSION.to_le_bytes());
    payload.extend_from_slice(&count.to_le_bytes());
    for member in members {
        let path = member.path.as_bytes();
        let path_len = u32::try_from(path.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "TOC path too long"))?;
        payload.extend_from_slice(&path_len.to_le_bytes());
        payload.extend_from_slice(path);
        payload.extend_from_slice(&member.size.to_le_bytes());
        payload.push(u8::from(member.is_directory));
    }
    Ok(payload)
}

pub(crate) fn read_tar_zstd_toc(path: &Path) -> Option<Vec<ArchiveMember>> {
    let mut file = File::open(path).ok()?;
    let file_len = file.seek(SeekFrom::End(0)).ok()?;
    let pointer_frame_len = 8 + u64::from(TAR_ZST_PTR_PAYLOAD_LEN);
    if file_len < pointer_frame_len {
        return None;
    }
    file.seek(SeekFrom::End(-(pointer_frame_len as i64))).ok()?;
    let mut header = [0_u8; 8];
    file.read_exact(&mut header).ok()?;
    let magic = u32::from_le_bytes(header[0..4].try_into().ok()?);
    let size = u32::from_le_bytes(header[4..8].try_into().ok()?);
    if magic != ZSTD_SKIPPABLE_MAGIC_PTR || size != TAR_ZST_PTR_PAYLOAD_LEN {
        return None;
    }
    let mut pointer = [0_u8; TAR_ZST_PTR_PAYLOAD_LEN as usize];
    file.read_exact(&mut pointer).ok()?;
    if &pointer[0..8] != TAR_ZST_PTR_MAGIC {
        return None;
    }
    let toc_offset = u64::from_le_bytes(pointer[8..16].try_into().ok()?);
    if toc_offset.saturating_add(8) >= file_len {
        return None;
    }
    file.seek(SeekFrom::Start(toc_offset)).ok()?;
    let mut toc_header = [0_u8; 8];
    file.read_exact(&mut toc_header).ok()?;
    let toc_magic = u32::from_le_bytes(toc_header[0..4].try_into().ok()?);
    let toc_size = u32::from_le_bytes(toc_header[4..8].try_into().ok()?) as usize;
    if toc_magic != ZSTD_SKIPPABLE_MAGIC_TOC {
        return None;
    }
    let mut payload = vec![0_u8; toc_size];
    file.read_exact(&mut payload).ok()?;
    decode_tar_zstd_toc(&payload)
}

pub(crate) fn decode_tar_zstd_toc(payload: &[u8]) -> Option<Vec<ArchiveMember>> {
    if payload.len() < 16 || &payload[0..8] != TAR_ZST_TOC_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(payload[8..12].try_into().ok()?);
    if version != TAR_ZST_TOC_VERSION {
        return None;
    }
    let count = u32::from_le_bytes(payload[12..16].try_into().ok()?) as usize;
    let mut offset = 16;
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 4 > payload.len() {
            return None;
        }
        let path_len = u32::from_le_bytes(payload[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        if offset + path_len + 9 > payload.len() {
            return None;
        }
        let path = std::str::from_utf8(&payload[offset..offset + path_len])
            .ok()?
            .to_owned();
        offset += path_len;
        let size = u64::from_le_bytes(payload[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let is_directory = payload[offset] != 0;
        offset += 1;
        members.push(ArchiveMember {
            path,
            size,
            is_directory,
        });
    }
    if offset != payload.len() {
        return None;
    }
    Some(members)
}

pub(crate) fn extract_compressed_tar(
    source: &Path,
    format: ArchiveFormat,
    member_path: &str,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    let reader = compressed_reader(source, format)
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries()
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?
    {
        let mut entry = entry.map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
        let path = entry
            .path()
            .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
        if normalize_path(&path).as_deref() == Some(member_path) {
            return io::copy(&mut entry, output)
                .map_err(|error| ArchiveOpenError::Other(error.to_string()));
        }
    }
    Err(ArchiveOpenError::Other(format!(
        "archive member not found: {member_path}"
    )))
}

pub(crate) fn compressed_reader(path: &Path, format: ArchiveFormat) -> io::Result<Box<dyn Read>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    match format {
        ArchiveFormat::TarXz | ArchiveFormat::Xz => Ok(Box::new(XzReader::new(reader, true))),
        ArchiveFormat::TarLzip | ArchiveFormat::Lzip => Ok(Box::new(LzipReader::new(reader))),
        ArchiveFormat::TarZstd | ArchiveFormat::Zstd => {
            // structured-zstd skips trailing skippable frames (embedded TOC) correctly.
            let decoder = structured_zstd::decoding::StreamingDecoder::new(reader)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("{err:?}")))?;
            Ok(Box::new(decoder))
        }
        ArchiveFormat::TarLz4 | ArchiveFormat::Lz4 => Ok(Box::new(FrameDecoder::new(reader))),
        ArchiveFormat::TarBrotli | ArchiveFormat::Brotli => {
            Ok(Box::new(Decompressor::new(reader, 128 * 1024)))
        }
        ArchiveFormat::Unified(UnifiedFormat::Gz) => Ok(Box::new(GzDecoder::new(reader))),
        ArchiveFormat::Unified(UnifiedFormat::Bz2) => Ok(Box::new(BzDecoder::new(reader))),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "format is not a compression stream",
        )),
    }
}
