use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::Path;

use unarc_rs::ArchiveError;
use unarc_rs::unified::{ArchiveFormat as UnifiedFormat, ArchiveOptions, UnifiedArchive};

use super::normalize_member_path;
use crate::archive::types::{ArchiveFormat, ArchiveMember, ArchiveOpenError};
pub(crate) fn list_unified(
    path: &Path,
    format: UnifiedFormat,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let file = File::open(path).map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let mut options = ArchiveOptions::new().with_verify_crc(true);
    if let Some(password) = password {
        options = options.with_password(password);
    }
    let mut archive =
        UnifiedArchive::open_with_format_and_options(BufReader::new(file), format, options)
            .map_err(|error| map_unified_error_with_password(error, password.is_some()))?;
    if matches!(
        format,
        UnifiedFormat::Gz | UnifiedFormat::Bz2 | UnifiedFormat::Z
    ) {
        archive.set_single_file_name(single_unified_name(path, format));
    }
    let entries = archive
        .entries()
        .map_err(|error| map_unified_error_with_password(error, password.is_some()))?;
    let encrypted = entries
        .iter()
        .filter(|entry| entry.is_encrypted())
        .min_by_key(|entry| entry.original_size());
    if password.is_none() && encrypted.is_some() {
        return Err(ArchiveOpenError::PasswordRequired(format!(
            "Archive '{}' contains encrypted entries",
            path.display()
        )));
    }
    if let (Some(password), Some(encrypted)) = (password, encrypted) {
        validate_password(path, format, encrypted.name(), password)?;
    }
    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            let raw = entry.name();
            let directory = raw.ends_with('/') || raw.ends_with('\\');
            normalize_member_path(raw).map(|path| ArchiveMember {
                path,
                size: entry.original_size(),
                is_directory: directory,
            })
        })
        .collect())
}

pub(crate) fn single_unified_name(path: &Path, format: UnifiedFormat) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let suffix = match format {
        UnifiedFormat::Gz => ".gz",
        UnifiedFormat::Bz2 => ".bz2",
        UnifiedFormat::Z => ".z",
        _ => "",
    };
    name.get(..name.len().saturating_sub(suffix.len()))
        .filter(|name| !name.is_empty())
        .unwrap_or("data")
        .to_owned()
}

pub(crate) fn validate_password(
    path: &Path,
    format: UnifiedFormat,
    member_name: &str,
    password: &str,
) -> Result<(), ArchiveOpenError> {
    let file = File::open(path).map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let options = ArchiveOptions::new()
        .with_verify_crc(true)
        .with_password(password);
    let mut archive =
        UnifiedArchive::open_with_format_and_options(BufReader::new(file), format, options.clone())
            .map_err(map_unified_error)?;
    if matches!(
        format,
        UnifiedFormat::Gz | UnifiedFormat::Bz2 | UnifiedFormat::Z
    ) {
        archive.set_single_file_name(single_unified_name(path, format));
    }
    while let Some(entry) = archive.next_entry().map_err(map_unified_error)? {
        if entry.name() == member_name {
            archive
                .read_to_with_options(&entry, &mut io::sink(), &options)
                .map_err(map_unified_error)?;
            return Ok(());
        }
        archive.skip(&entry).map_err(map_unified_error)?;
    }
    Err(ArchiveOpenError::Other(
        "encrypted archive member disappeared while validating password".to_owned(),
    ))
}

pub(crate) fn extract_unified(
    source: &Path,
    format: UnifiedFormat,
    member_path: &str,
    password: Option<&str>,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    let file = File::open(source).map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let mut options = ArchiveOptions::new().with_verify_crc(true);
    if let Some(password) = password {
        options = options.with_password(password);
    }
    let mut archive =
        UnifiedArchive::open_with_format_and_options(BufReader::new(file), format, options.clone())
            .map_err(map_unified_error)?;
    if matches!(
        format,
        UnifiedFormat::Gz | UnifiedFormat::Bz2 | UnifiedFormat::Z
    ) {
        archive.set_single_file_name(single_unified_name(source, format));
    }
    while let Some(entry) = archive.next_entry().map_err(map_unified_error)? {
        if normalize_member_path(entry.name()).as_deref() == Some(member_path) {
            return archive
                .read_to_with_options(&entry, output, &options)
                .map_err(map_unified_error);
        }
        archive.skip(&entry).map_err(map_unified_error)?;
    }
    Err(ArchiveOpenError::Other(format!(
        "archive member not found: {member_path}"
    )))
}

pub(crate) fn single_file_name(path: &Path, format: ArchiveFormat) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let suffixes: &[&str] = match format {
        ArchiveFormat::Xz => &[".xz"],
        ArchiveFormat::Lzip => &[".lzip", ".lz"],
        ArchiveFormat::Zstd => &[".zstd", ".zst"],
        ArchiveFormat::Lz4 => &[".lz4"],
        ArchiveFormat::Brotli => &[".br"],
        _ => &[],
    };
    suffixes
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .filter(|name| !name.is_empty())
        .unwrap_or("data")
        .to_owned()
}
pub(crate) fn map_unified_error(error: ArchiveError) -> ArchiveOpenError {
    match error {
        ArchiveError::PasswordRequired { .. } | ArchiveError::EncryptionRequired { .. } => {
            ArchiveOpenError::PasswordRequired(error.to_string())
        }
        ArchiveError::InvalidPassword { .. } => {
            ArchiveOpenError::InvalidPassword(error.to_string())
        }
        other => {
            let message = other.to_string();
            if message.to_ascii_lowercase().contains("password") {
                ArchiveOpenError::InvalidPassword(message)
            } else {
                ArchiveOpenError::Other(message)
            }
        }
    }
}

pub(crate) fn map_unified_error_with_password(
    error: ArchiveError,
    password_supplied: bool,
) -> ArchiveOpenError {
    match map_unified_error(error) {
        ArchiveOpenError::InvalidPassword(message) if !password_supplied => {
            ArchiveOpenError::PasswordRequired(message)
        }
        error => error,
    }
}
