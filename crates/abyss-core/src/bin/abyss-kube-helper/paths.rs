use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::{MAX_FRAME, ROOT};

use crate::helper_protocol::{HelperEntry, HelperEntryKind};

pub(crate) fn metadata_kind(metadata: &fs::Metadata) -> HelperEntryKind {
    if metadata.file_type().is_symlink() {
        HelperEntryKind::Symlink
    } else if metadata.is_dir() {
        HelperEntryKind::Directory
    } else if metadata.is_file() {
        HelperEntryKind::File
    } else {
        HelperEntryKind::Other
    }
}

pub(crate) fn safe_relative_path(root: &Path, relative: &[Vec<u8>]) -> io::Result<PathBuf> {
    safe_path_beneath(root, relative)
}

pub(crate) fn safe_relative_mutation_path(
    root: &Path,
    relative: &[Vec<u8>],
) -> io::Result<PathBuf> {
    if relative.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing an empty bulk path",
        ));
    }
    safe_relative_path(root, relative)
}

pub(crate) fn safe_path(components: &[Vec<u8>]) -> io::Result<PathBuf> {
    safe_path_beneath(Path::new(ROOT), components)
}

pub(crate) fn safe_mutation_path(components: &[Vec<u8>]) -> io::Result<PathBuf> {
    if components.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to modify the PVC mount root",
        ));
    }
    safe_path(components)
}

pub(crate) fn safe_path_beneath(root: &Path, components: &[Vec<u8>]) -> io::Result<PathBuf> {
    let mut result = root.to_owned();
    for component in components {
        if component.is_empty()
            || component == b"."
            || component == b".."
            || component.contains(&b'/')
            || component.contains(&0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe path component",
            ));
        }
        result.push(OsString::from_vec(component.clone()));
        match fs::symlink_metadata(&result) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to traverse a symbolic link in the PVC",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if !result.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes the PVC root",
        ));
    }
    Ok(result)
}

pub(crate) fn entry(name: OsString, path: &Path) -> io::Result<HelperEntry> {
    let metadata = fs::symlink_metadata(path)?;
    let kind = if metadata.file_type().is_symlink() {
        HelperEntryKind::Symlink
    } else if metadata.is_dir() {
        HelperEntryKind::Directory
    } else if metadata.is_file() {
        HelperEntryKind::File
    } else {
        HelperEntryKind::Other
    };
    let modified_seconds = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs());
    Ok(HelperEntry {
        name: name.into_vec(),
        kind,
        size: metadata.is_file().then_some(metadata.len()),
        modified_seconds,
    })
}

pub(crate) fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "helper protocol frame is too large",
        ));
    }
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    ciborium::from_reader(data.as_slice())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

pub(crate) fn write_frame<T: serde::Serialize>(
    writer: &mut impl Write,
    value: &T,
) -> io::Result<()> {
    let mut data = Vec::new();
    ciborium::into_writer(value, &mut data)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let length = u32::try_from(data.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame is too large"))?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&data)
}
