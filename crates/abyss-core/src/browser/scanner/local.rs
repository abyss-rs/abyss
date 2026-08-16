use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

#[cfg(target_os = "macos")]
use crate::browser::scanner::macos::{bulk_fallback_error, read_getattrlistbulk};
use crate::browser::types::{BrowserEntry, BrowserKind};

pub(crate) const STREAM_BATCH: usize = 128;

pub(crate) fn read_directory_streamed(
    path: &Path,
    hide_dot_underscore: bool,
    emit: impl Fn(Vec<BrowserEntry>) -> bool,
) -> std::io::Result<Vec<BrowserEntry>> {
    #[cfg(target_os = "macos")]
    match read_getattrlistbulk(path, hide_dot_underscore, &emit) {
        Ok(entries) => Ok(entries),
        Err(error) if bulk_fallback_error(&error) => {
            read_directory_fallback(path, hide_dot_underscore, emit)
        }
        Err(error) => Err(error),
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_directory_fallback(path, hide_dot_underscore, emit)
    }
}

pub(crate) fn is_dot_underscore(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b"._")
}

pub(crate) fn read_directory_fallback(
    path: &Path,
    hide_dot_underscore: bool,
    emit: impl Fn(Vec<BrowserEntry>) -> bool,
) -> std::io::Result<Vec<BrowserEntry>> {
    let mut all = Vec::new();
    let mut batch = Vec::with_capacity(STREAM_BATCH);
    let mut ordinal = 1_u64;
    for entry_res in fs::read_dir(path)? {
        let dir_entry = entry_res?;
        let name = dir_entry.file_name();
        if hide_dot_underscore && is_dot_underscore(&name) {
            continue;
        }
        let metadata = dir_entry.metadata().ok();
        let file_type = dir_entry.file_type().ok();
        let entry = entry_from_dir_entry(name, metadata.as_ref(), file_type.as_ref(), ordinal);
        ordinal += 1;
        batch.push(entry.clone());
        all.push(entry);
        if batch.len() == STREAM_BATCH {
            let chunk = std::mem::take(&mut batch);
            if !emit(chunk) {
                return Ok(all);
            }
            batch.reserve(STREAM_BATCH);
        }
    }
    if !batch.is_empty() && !emit(batch) {
        return Ok(all);
    }
    Ok(all)
}

pub(crate) fn entry_from_dir_entry(
    name: OsString,
    metadata: Option<&fs::Metadata>,
    file_type: Option<&fs::FileType>,
    ordinal: u64,
) -> BrowserEntry {
    let kind = if let Some(ft) = file_type {
        if ft.is_dir() {
            BrowserKind::Directory
        } else if ft.is_symlink() {
            BrowserKind::Symlink
        } else if ft.is_file() {
            BrowserKind::File
        } else {
            BrowserKind::Other
        }
    } else if let Some(md) = metadata {
        if md.is_dir() {
            BrowserKind::Directory
        } else if md.is_symlink() {
            BrowserKind::Symlink
        } else if md.is_file() {
            BrowserKind::File
        } else {
            BrowserKind::Other
        }
    } else {
        BrowserKind::Unknown
    };
    let size = metadata.and_then(|md| if md.is_file() { Some(md.len()) } else { None });
    let modified = metadata.and_then(|md| md.modified().ok());
    #[cfg(unix)]
    let mode = metadata.map(|md| {
        use std::os::unix::fs::PermissionsExt;
        md.permissions().mode()
    });
    #[cfg(not(unix))]
    let mode = None;

    BrowserEntry {
        name,
        raw_name: None,
        kind,
        size,
        modified,
        mode,
        ordinal,
    }
}

#[cfg(unix)]
pub(crate) fn os_string_from_external(value: Vec<u8>) -> OsString {
    OsString::from_vec(value)
}

#[cfg(windows)]
pub(crate) fn os_string_from_external(value: Vec<u8>) -> OsString {
    match String::from_utf8(value) {
        Ok(value) => value.into(),
        Err(error) => format!(
            "<raw:{}>",
            percent_encoding::percent_encode(error.as_bytes(), percent_encoding::NON_ALPHANUMERIC,)
        )
        .into(),
    }
}
