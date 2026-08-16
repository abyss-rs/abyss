use std::ffi::{CString, OsStr, OsString};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;

use crate::ROOT;
use crate::decompress::{BrotliBlockReader, DeflateBlockReader, Lz4BlockReader};
use crate::paths::{
    entry, metadata_kind, safe_mutation_path, safe_path, safe_relative_mutation_path,
    safe_relative_path,
};
use crate::tree::{install_file, list_tree, payload_size, write_tree};

use crate::helper_protocol::{HelperCompression, HelperEntryKind, HelperOperation, HelperResult};

pub(crate) fn execute(
    operation: &HelperOperation,
    input: &mut impl Read,
) -> io::Result<(HelperResult, Option<(fs::File, u64)>)> {
    match operation {
        HelperOperation::Capabilities => Ok((
            HelperResult::Capabilities {
                bulk_tree: true,
                usage: true,
            },
            None,
        )),
        HelperOperation::Usage => Ok((volume_usage()?, None)),
        HelperOperation::List { path } => {
            let path = safe_path(path)?;
            let mut entries = Vec::new();
            for item in fs::read_dir(path)? {
                let item = item?;
                if item.file_name().as_bytes().starts_with(b"._") {
                    continue;
                }
                entries.push(entry(item.file_name(), &item.path())?);
            }
            Ok((HelperResult::Entries(entries), None))
        }
        HelperOperation::Stat { path } => {
            let path = safe_path(path)?;
            let name = path
                .file_name()
                .map(OsStr::to_owned)
                .unwrap_or_else(|| OsString::from("/"));
            Ok((HelperResult::Entry(entry(name, &path)?), None))
        }
        HelperOperation::Read {
            path,
            offset,
            length,
        } => {
            let mut file = fs::File::open(safe_path(path)?)?;
            let available = file.metadata()?.len().saturating_sub(*offset);
            let size = length.map_or(available, |length| length.min(available));
            file.seek(SeekFrom::Start(*offset))?;
            Ok((HelperResult::Data { size }, Some((file, size))))
        }
        HelperOperation::Write {
            path,
            size,
            overwrite,
        } => {
            install_file(safe_mutation_path(path)?, *size, *overwrite, input, true)?;
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::CreateDir { path } => {
            fs::create_dir_all(safe_mutation_path(path)?)?;
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::Delete { path, recursive } => {
            let path = safe_mutation_path(path)?;
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                if *recursive {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_dir(path)?;
                }
            } else {
                fs::remove_file(path)?;
            }
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::Rename {
            source,
            destination,
            overwrite,
        } => {
            let source = safe_mutation_path(source)?;
            let destination = safe_mutation_path(destination)?;
            if !overwrite && destination.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination already exists",
                ));
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(source, destination)?;
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::ListTree { root } => {
            let root = safe_path(root)?;
            Ok((HelperResult::TreeEntries(list_tree(&root)?), None))
        }
        HelperOperation::InspectTree { root, entries } => {
            let root = safe_path(root)?;
            let mut states = Vec::with_capacity(entries.len());
            for item in entries {
                let path = safe_relative_path(&root, &item.path)?;
                states.push(match fs::symlink_metadata(path) {
                    Ok(metadata) => Some(metadata_kind(&metadata)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error),
                });
            }
            Ok((HelperResult::TreeStates(states), None))
        }
        HelperOperation::WriteTree {
            root,
            entries,
            compression,
        } => {
            let root = safe_mutation_path(root)?;
            fs::create_dir_all(&root)?;
            match compression {
                HelperCompression::None => write_tree(&root, entries, input)?,
                HelperCompression::Lz4 => {
                    let expected = payload_size(entries)?;
                    let mut decoded = Lz4BlockReader::new(input, expected);
                    write_tree(&root, entries, &mut decoded)?;
                    if decoded.remaining != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "bulk upload ended before all file data arrived",
                        ));
                    }
                }
                HelperCompression::Brotli => {
                    let expected = payload_size(entries)?;
                    let mut decoded = BrotliBlockReader::new(input, expected);
                    write_tree(&root, entries, &mut decoded)?;
                    if decoded.remaining != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "bulk upload ended before all file data arrived",
                        ));
                    }
                }
                HelperCompression::Deflate => {
                    let expected = payload_size(entries)?;
                    let mut decoded = DeflateBlockReader::new(input, expected);
                    write_tree(&root, entries, &mut decoded)?;
                    if decoded.remaining != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "bulk upload ended before all file data arrived",
                        ));
                    }
                }
            }
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::CopyTree {
            source,
            destination,
            entries,
        } => {
            let source = safe_path(source)?;
            let destination = safe_mutation_path(destination)?;
            fs::create_dir_all(&destination)?;
            for item in entries {
                let from = safe_relative_path(&source, &item.path)?;
                let to = safe_relative_mutation_path(&destination, &item.path)?;
                match item.kind {
                    HelperEntryKind::Directory => fs::create_dir_all(to)?,
                    HelperEntryKind::File => {
                        let mut file = fs::File::open(from)?;
                        install_file(to, item.size, item.overwrite, &mut file, false)?;
                    }
                    HelperEntryKind::Symlink | HelperEntryKind::Other => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "bulk copy supports only files and directories",
                        ));
                    }
                }
            }
            Ok((HelperResult::Ok, None))
        }
        HelperOperation::ReadTree { .. } => unreachable!("handled by serve"),
    }
}

#[allow(clippy::unnecessary_cast)]
pub(crate) fn volume_usage() -> io::Result<HelperResult> {
    let root = CString::new(ROOT)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid helper root"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    let result = unsafe { libc::statvfs(root.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize as u64;
    Ok(HelperResult::Usage {
        capacity_bytes: (stats.f_blocks as u64).saturating_mul(block_size),
        free_bytes: (stats.f_bavail as u64).saturating_mul(block_size),
        total_inodes: stats.f_files as u64,
        free_inodes: stats.f_favail as u64,
    })
}
