mod request;
mod unified;

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Component, Path};

use unarc_rs::unified::{ArchiveFormat as UnifiedFormat, ArchiveOptions, UnifiedArchive};

pub(crate) use self::request::load_request;
use self::request::multipart_base;
use self::unified::{extract_unified, single_file_name, single_unified_name};
pub(crate) use self::unified::{list_unified, map_unified_error};
use crate::archive::formats::sevenz::list_7z;
use crate::archive::formats::tar::{
    compressed_reader, extract_compressed_tar, list_compressed_tar, list_tar_zstd,
};
use crate::archive::formats::unrar::{extract_rar, list_rar, read_selected_rar};
use crate::archive::types::{ArchiveFormat, ArchiveIndex, ArchiveMember, ArchiveOpenError};

pub(crate) fn open_index(
    path: &Path,
    password: Option<&str>,
) -> Result<ArchiveIndex, ArchiveOpenError> {
    let format = detect(path)?;
    let members = match format {
        ArchiveFormat::Unified(UnifiedFormat::SevenZ) => list_7z(path, password)?,
        ArchiveFormat::Unified(format) => list_unified(path, format, password)?,
        ArchiveFormat::Rar => list_rar(path, password)?,
        ArchiveFormat::TarXz
        | ArchiveFormat::TarLzip
        | ArchiveFormat::TarLz4
        | ArchiveFormat::TarBrotli => list_compressed_tar(path, format)?,
        ArchiveFormat::TarZstd => list_tar_zstd(path)?,
        ArchiveFormat::Xz
        | ArchiveFormat::Lzip
        | ArchiveFormat::Zstd
        | ArchiveFormat::Lz4
        | ArchiveFormat::Brotli => vec![ArchiveMember {
            path: single_file_name(path, format),
            size: 0,
            is_directory: false,
        }],
    };
    Ok(ArchiveIndex {
        source: path.to_owned(),
        format,
        members,
    })
}

pub fn looks_like_archive(path: &Path) -> bool {
    special_format(path).is_some()
        || UnifiedFormat::from_path(path).is_some()
        || multipart_base(path).is_some_and(|base| {
            special_format(&base).is_some() || UnifiedFormat::from_path(&base).is_some()
        })
}

pub fn extract_member(
    index: &ArchiveIndex,
    member_path: &str,
    password: Option<&str>,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    match index.format {
        ArchiveFormat::Unified(format) => {
            extract_unified(&index.source, format, member_path, password, output)
        }
        ArchiveFormat::Rar => extract_rar(&index.source, member_path, password, output),
        ArchiveFormat::TarXz
        | ArchiveFormat::TarLzip
        | ArchiveFormat::TarZstd
        | ArchiveFormat::TarLz4
        | ArchiveFormat::TarBrotli => {
            extract_compressed_tar(&index.source, index.format, member_path, output)
        }
        ArchiveFormat::Xz
        | ArchiveFormat::Lzip
        | ArchiveFormat::Zstd
        | ArchiveFormat::Lz4
        | ArchiveFormat::Brotli => {
            let mut reader = compressed_reader(&index.source, index.format)
                .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
            io::copy(&mut reader, output)
                .map_err(|error| ArchiveOpenError::Other(error.to_string()))
        }
    }
}

pub fn read_selected(
    index: &ArchiveIndex,
    selected: &HashSet<String>,
    password: Option<&str>,
    mut consume: impl FnMut(&ArchiveMember, &mut dyn Read) -> Result<(), ArchiveOpenError>,
) -> Result<(), ArchiveOpenError> {
    match index.format {
        ArchiveFormat::Unified(format) => {
            let file = File::open(&index.source)
                .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
            let mut options = ArchiveOptions::new().with_verify_crc(true);
            if let Some(password) = password {
                options = options.with_password(password);
            }
            let mut archive = UnifiedArchive::open_with_format_and_options(
                BufReader::new(file),
                format,
                options.clone(),
            )
            .map_err(map_unified_error)?;
            if matches!(
                format,
                UnifiedFormat::Gz | UnifiedFormat::Bz2 | UnifiedFormat::Z
            ) {
                archive.set_single_file_name(single_unified_name(&index.source, format));
            }
            let mut delivered = HashSet::new();
            while let Some(entry) = archive.next_entry().map_err(map_unified_error)? {
                let normalized = normalize_member_path(entry.name());
                if let Some(path) = normalized.as_deref()
                    && selected.contains(path)
                    && delivered.insert(path.to_owned())
                    && let Some(member) = index.member(path)
                {
                    let data = archive
                        .read_with_options(&entry, &options)
                        .map_err(map_unified_error)?;
                    let mut reader = io::Cursor::new(data);
                    consume(member, &mut reader)?;
                } else {
                    archive.skip(&entry).map_err(map_unified_error)?;
                }
            }
            Ok(())
        }
        ArchiveFormat::Rar => read_selected_rar(index, selected, password, consume),

        ArchiveFormat::TarXz
        | ArchiveFormat::TarLzip
        | ArchiveFormat::TarZstd
        | ArchiveFormat::TarLz4
        | ArchiveFormat::TarBrotli => {
            let reader = compressed_reader(&index.source, index.format)
                .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
            let mut archive = tar::Archive::new(reader);
            let mut delivered = HashSet::new();
            for entry in archive
                .entries()
                .map_err(|error| ArchiveOpenError::Other(error.to_string()))?
            {
                let mut entry =
                    entry.map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
                let path = entry
                    .path()
                    .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
                let Some(path) = normalize_path(&path) else {
                    continue;
                };
                if selected.contains(&path)
                    && delivered.insert(path.clone())
                    && let Some(member) = index.member(&path)
                {
                    consume(member, &mut entry)?;
                }
            }
            Ok(())
        }
        ArchiveFormat::Xz
        | ArchiveFormat::Lzip
        | ArchiveFormat::Zstd
        | ArchiveFormat::Lz4
        | ArchiveFormat::Brotli => {
            let Some(member) = index.members.first() else {
                return Ok(());
            };
            if selected.contains(&member.path) {
                let mut reader = compressed_reader(&index.source, index.format)
                    .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
                consume(member, &mut reader)?;
            }
            Ok(())
        }
    }
}

pub(crate) fn detect(path: &Path) -> Result<ArchiveFormat, ArchiveOpenError> {
    if let Some(format) = special_format(path) {
        return Ok(format);
    }
    let mut file = File::open(path).map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let mut signature = [0_u8; 8];
    let count = file
        .read(&mut signature)
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let signature = &signature[..count];
    if signature.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        return Ok(ArchiveFormat::Xz);
    }
    if signature.starts_with(b"LZIP") {
        return Ok(ArchiveFormat::Lzip);
    }
    if signature.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Ok(ArchiveFormat::Zstd);
    }
    if signature.starts_with(&[0x04, 0x22, 0x4d, 0x18]) {
        return Ok(ArchiveFormat::Lz4);
    }
    file.rewind()
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?;
    let format = UnifiedFormat::detect(&mut file, Some(path))
        .map_err(|error| ArchiveOpenError::Other(error.to_string()))?
        .ok_or(ArchiveOpenError::NotArchive)?;
    if format == UnifiedFormat::Rar {
        Ok(ArchiveFormat::Rar)
    } else {
        Ok(ArchiveFormat::Unified(format))
    }
}

pub(crate) fn special_format(path: &Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let formats = [
        (".rar", ArchiveFormat::Rar),
        (".tar.gz", ArchiveFormat::Unified(UnifiedFormat::Tgz)),
        (".tgz", ArchiveFormat::Unified(UnifiedFormat::Tgz)),
        (".tar.bz2", ArchiveFormat::Unified(UnifiedFormat::Tbz)),
        (".tbz2", ArchiveFormat::Unified(UnifiedFormat::Tbz)),
        (".tbz", ArchiveFormat::Unified(UnifiedFormat::Tbz)),
        (".tar.z", ArchiveFormat::Unified(UnifiedFormat::TarZ)),
        (".tar.xz", ArchiveFormat::TarXz),
        (".txz", ArchiveFormat::TarXz),
        (".tar.lz", ArchiveFormat::TarLzip),
        (".tar.lzip", ArchiveFormat::TarLzip),
        (".tar.zst", ArchiveFormat::TarZstd),
        (".tar.zstd", ArchiveFormat::TarZstd),
        (".tar.lz4", ArchiveFormat::TarLz4),
        (".tar.br", ArchiveFormat::TarBrotli),
        (".xz", ArchiveFormat::Xz),
        (".lz", ArchiveFormat::Lzip),
        (".lzip", ArchiveFormat::Lzip),
        (".zst", ArchiveFormat::Zstd),
        (".zstd", ArchiveFormat::Zstd),
        (".lz4", ArchiveFormat::Lz4),
        (".br", ArchiveFormat::Brotli),
    ];
    formats
        .into_iter()
        .find_map(|(suffix, format)| name.ends_with(suffix).then_some(format))
}
pub fn normalize_member_path(value: &str) -> Option<String> {
    let replaced = value.replace('\\', "/");
    normalize_path(Path::new(&replaced))
}

pub(crate) fn normalize_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                if !part.is_empty() {
                    parts.push(part);
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
