use std::collections::HashSet;
use std::io::{self, Write};
use std::path::Path;

use unarc_rs::rar::rar_archive::RarArchive;

use crate::archive::reader::{map_unified_error, normalize_member_path};
use crate::archive::types::{ArchiveIndex, ArchiveMember, ArchiveOpenError};

pub(crate) fn list_rar(
    path: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let mut archive =
        RarArchive::<io::Cursor<Vec<u8>>>::from_path(path).map_err(map_unified_error)?;
    let mut headers = Vec::new();
    while let Some(header) = archive.get_next_entry().map_err(map_unified_error)? {
        headers.push(header);
    }
    if let Some(encrypted) = headers
        .iter()
        .filter(|header| header.is_encrypted && !header.is_directory)
        .min_by_key(|header| header.original_size)
    {
        let Some(password) = password else {
            return Err(ArchiveOpenError::PasswordRequired(format!(
                "Archive '{}' contains encrypted entries",
                path.display()
            )));
        };
        archive
            .read_with_password(encrypted, Some(password.to_owned()))
            .map_err(|error| ArchiveOpenError::InvalidPassword(error.to_string()))?;
    }
    Ok(headers
        .into_iter()
        .filter_map(|header| {
            normalize_member_path(&header.name).map(|path| ArchiveMember {
                path,
                size: header.original_size,
                is_directory: header.is_directory,
            })
        })
        .collect())
}

pub(crate) fn extract_rar(
    source: &Path,
    member_path: &str,
    password: Option<&str>,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    let mut archive =
        RarArchive::<io::Cursor<Vec<u8>>>::from_path(source).map_err(map_unified_error)?;
    while let Some(header) = archive.get_next_entry().map_err(map_unified_error)? {
        if normalize_member_path(&header.name).as_deref() != Some(member_path) {
            continue;
        }
        if header.is_encrypted && password.is_none() {
            return Err(ArchiveOpenError::PasswordRequired(format!(
                "Archive member '{member_path}' is encrypted"
            )));
        }
        let data = archive
            .read_with_password(&header, password.map(ToOwned::to_owned))
            .map_err(map_unified_error)?;
        output
            .write_all(&data)
            .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
        return Ok(data.len() as u64);
    }
    Err(ArchiveOpenError::Other(format!(
        "archive member not found: {member_path}"
    )))
}

pub(crate) fn read_selected_rar(
    index: &ArchiveIndex,
    selected: &HashSet<String>,
    password: Option<&str>,
    mut consume: impl FnMut(&ArchiveMember, &mut dyn io::Read) -> Result<(), ArchiveOpenError>,
) -> Result<(), ArchiveOpenError> {
    let mut archive =
        RarArchive::<io::Cursor<Vec<u8>>>::from_path(&index.source).map_err(map_unified_error)?;
    let mut delivered = HashSet::new();
    while let Some(header) = archive.get_next_entry().map_err(map_unified_error)? {
        let Some(path) = normalize_member_path(&header.name) else {
            continue;
        };
        if !selected.contains(&path) || !delivered.insert(path.clone()) {
            continue;
        }
        let Some(member) = index.member(&path) else {
            continue;
        };
        let data = archive
            .read_with_password(&header, password.map(ToOwned::to_owned))
            .map_err(map_unified_error)?;
        let mut reader = io::Cursor::new(data);
        consume(member, &mut reader)?;
    }
    Ok(())
}
