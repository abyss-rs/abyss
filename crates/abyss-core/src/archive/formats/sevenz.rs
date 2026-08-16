use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use sevenz_rust2::encoder_options::EncoderOptions;
use sevenz_rust2::encoder_options::{
    AesEncoderOptions, Bzip2Options, Lzma2Options, LzmaOptions, PpmdOptions,
};
use sevenz_rust2::{
    Archive, ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, Password,
    SourceReader,
};
use unarc_rs::unified::ArchiveFormat as UnifiedFormat;

use crate::Error;
use crate::archive::reader::{list_unified, normalize_member_path};
use crate::archive::types::{
    ArchiveCreateOptions, ArchiveMember, ArchiveOpenError, CompressionMethod, CompressionThreads,
};
use crate::archive::writer::{CreateEntry, progress_reader};
use crate::progress::CopyStats;

pub(crate) const LZMA2_CHUNK_SIZE: u64 = 16 * 1024 * 1024;

pub(crate) fn sevenz_members(archive: &Archive) -> Vec<ArchiveMember> {
    archive
        .files
        .iter()
        .filter_map(|entry| {
            normalize_member_path(&entry.name).map(|path| ArchiveMember {
                path,
                size: entry.size,
                is_directory: entry.is_directory,
            })
        })
        .collect()
}

pub(crate) fn sevenz_has_encrypted_header(path: &Path) -> bool {
    const SIGNATURE_HEADER_SIZE: usize = 32;
    const MAX_ENCODED_HEADER_DESCRIPTOR: usize = 64 * 1024;

    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut signature_header = [0_u8; SIGNATURE_HEADER_SIZE];
    if file.read_exact(&mut signature_header).is_err()
        || signature_header[..6] != [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]
    {
        return false;
    }
    let next_header_offset = u64::from_le_bytes(signature_header[12..20].try_into().unwrap());
    let next_header_size = u64::from_le_bytes(signature_header[20..28].try_into().unwrap());
    let Some(next_header_position) = 32_u64.checked_add(next_header_offset) else {
        return false;
    };
    let descriptor_size = next_header_size.min(MAX_ENCODED_HEADER_DESCRIPTOR as u64) as usize;
    if descriptor_size == 0 || file.seek(SeekFrom::Start(next_header_position)).is_err() {
        return false;
    }
    let mut descriptor = vec![0_u8; descriptor_size];
    if file.read_exact(&mut descriptor).is_err() || descriptor[0] != 0x17 {
        return false;
    }
    descriptor
        .windows(sevenz_rust2::EncoderMethod::ID_AES256_SHA256.len())
        .any(|bytes| bytes == sevenz_rust2::EncoderMethod::ID_AES256_SHA256)
}

pub(crate) fn list_7z(
    path: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let encrypted_header = sevenz_has_encrypted_header(path);
    if let Some(password_text) = password {
        let password = Password::from(password_text);
        let archive =
            Archive::open_with_password(path, &password).map_err(|error| match error {
                sevenz_rust2::Error::PasswordRequired
                | sevenz_rust2::Error::MaybeBadPassword(_)
                | sevenz_rust2::Error::ChecksumVerificationFailed => {
                    ArchiveOpenError::InvalidPassword("Wrong 7z archive password".to_owned())
                }
                error => ArchiveOpenError::Other(error.to_string()),
            })?;

        // The encrypted header CRC authenticates the password. Inspecting its small,
        // plaintext descriptor avoids a second password derivation and avoids
        // decompressing an entry, which is especially expensive for solid 7z.
        if encrypted_header {
            return Ok(sevenz_members(&archive));
        }

        // External archives may expose their headers while encrypting only file data.
        // Keep the stronger content-validation path for those archives.
        let encrypted_content = archive.blocks.iter().any(|block| {
            block.coders.iter().any(|coder| {
                coder.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
            })
        });
        return if encrypted_content {
            list_unified(path, UnifiedFormat::SevenZ, Some(password_text))
        } else {
            Ok(sevenz_members(&archive))
        };
    }

    if encrypted_header {
        return Err(ArchiveOpenError::PasswordRequired(format!(
            "Archive '{}' has encrypted file names",
            path.display()
        )));
    }

    match Archive::open(path) {
        Ok(archive) => {
            let encrypted = archive.blocks.iter().any(|block| {
                block.coders.iter().any(|coder| {
                    coder.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256
                })
            });
            if encrypted {
                Err(ArchiveOpenError::PasswordRequired(format!(
                    "Archive '{}' contains encrypted entries",
                    path.display()
                )))
            } else {
                Ok(sevenz_members(&archive))
            }
        }
        Err(sevenz_rust2::Error::PasswordRequired)
        | Err(sevenz_rust2::Error::MaybeBadPassword(_))
        | Err(sevenz_rust2::Error::ChecksumVerificationFailed) => {
            Err(ArchiveOpenError::PasswordRequired(format!(
                "Archive '{}' has encrypted file names",
                path.display()
            )))
        }
        Err(error) => Err(ArchiveOpenError::Other(error.to_string())),
    }
}

pub(crate) fn sevenz_methods(
    options: &ArchiveCreateOptions,
    input_bytes: u64,
) -> Result<Vec<sevenz_rust2::EncoderConfiguration>, Error> {
    let compression = match options.method {
        CompressionMethod::Store => EncoderConfiguration::new(EncoderMethod::COPY),
        CompressionMethod::Lzma2 => {
            let workers = lzma2_workers(options.threads, input_bytes);
            if workers > 1 {
                Lzma2Options::from_level_mt(u32::from(options.level), workers, LZMA2_CHUNK_SIZE)
                    .into()
            } else {
                Lzma2Options::from_level(u32::from(options.level)).into()
            }
        }
        CompressionMethod::Lzma => EncoderConfiguration::new(EncoderMethod::LZMA).with_options(
            EncoderOptions::Lzma(LzmaOptions::from_level(u32::from(options.level))),
        ),
        CompressionMethod::Ppmd => PpmdOptions::from_level(u32::from(options.level)).into(),
        CompressionMethod::Bzip2 => Bzip2Options::from_level(u32::from(options.level)).into(),
        _ => return Err(Error::message("method is not valid for a 7z archive")),
    };
    let mut methods = Vec::with_capacity(2);
    if let Some(password) = options.password.as_deref() {
        methods.push(AesEncoderOptions::new(Password::from(password.as_str())).into());
    }
    methods.push(compression);
    Ok(methods)
}

pub(crate) fn lzma2_workers(threads: CompressionThreads, input_bytes: u64) -> u32 {
    let available = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let requested = match threads {
        // LZMA2 workers are memory-heavy, and up to three archive jobs can run
        // concurrently. Four gives useful parallelism without excessive pressure.
        CompressionThreads::Auto => available.saturating_sub(1).clamp(1, 4),
        CompressionThreads::Count(count) => usize::from(count).clamp(1, available),
    };
    if input_bytes < LZMA2_CHUNK_SIZE * 2 {
        return 1;
    }
    requested
        .min((input_bytes / LZMA2_CHUNK_SIZE) as usize)
        .max(1) as u32
}

pub(crate) fn write_7z_archive<W: Write + Seek>(
    sink: W,
    options: &ArchiveCreateOptions,
    entries: &[CreateEntry],
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    const SOLID_BLOCK_LIMIT: u64 = 512 * 1024 * 1024;
    let total_file_bytes = entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| entry.size)
        .sum();
    let mut archive = ArchiveWriter::new(sink)
        .map_err(|error| Error::message(format!("initialize 7z writer: {error}")))?;
    archive.set_content_methods(sevenz_methods(options, total_file_bytes)?);
    archive.set_encrypt_header(options.password.is_some());

    for entry in entries.iter().filter(|entry| entry.is_directory) {
        archive
            .push_archive_entry::<&[u8]>(ArchiveEntry::new_directory(&entry.name), None)
            .map_err(|error| Error::message(format!("archive {}: {error}", entry.name)))?;
        stats.complete_object(&entry.source);
    }

    let files = entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .collect::<Vec<_>>();
    if options.solid {
        let mut start = 0;
        while start < files.len() {
            let mut end = start;
            let mut bytes = 0_u64;
            while end < files.len() && (end == start || bytes < SOLID_BLOCK_LIMIT) {
                bytes = bytes.saturating_add(files[end].size);
                end += 1;
            }
            let batch = &files[start..end];
            let archive_entries = batch
                .iter()
                .map(|entry| ArchiveEntry::from_path(&entry.source, entry.name.clone()))
                .collect::<Vec<_>>();
            let readers = batch
                .iter()
                .map(|entry| SourceReader::new(progress_reader(entry, cancelled, stats)))
                .collect::<Vec<_>>();
            archive.set_content_methods(sevenz_methods(options, bytes)?);
            archive
                .push_archive_entries(archive_entries, readers)
                .map_err(|error| {
                    if cancelled.load(Ordering::Relaxed) {
                        Error::Cancelled
                    } else {
                        Error::message(format!("write solid 7z block: {error}"))
                    }
                })?;
            start = end;
        }
    } else {
        for entry in files {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            archive.set_content_methods(sevenz_methods(options, entry.size)?);
            archive
                .push_archive_entry(
                    ArchiveEntry::from_path(&entry.source, entry.name.clone()),
                    Some(progress_reader(entry, cancelled, stats)),
                )
                .map_err(|error| {
                    if cancelled.load(Ordering::Relaxed) {
                        Error::Cancelled
                    } else {
                        Error::message(format!("archive {}: {error}", entry.name))
                    }
                })?;
        }
    }
    archive
        .finish()
        .map_err(|error| Error::message(format!("finalize 7z archive: {error}")))?;
    Ok(())
}
