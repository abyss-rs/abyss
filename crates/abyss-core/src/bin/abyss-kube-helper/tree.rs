use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::compress::write_compressed_block;
use crate::paths::{
    metadata_kind, safe_path, safe_relative_mutation_path, safe_relative_path, write_frame,
};
use crate::{BROTLI_BLOCK, LZ4_BLOCK};

use crate::helper_protocol::{HelperCompression, HelperEntryKind, HelperResult, HelperTreeEntry};

pub(crate) fn write_tree(
    root: &Path,
    entries: &[HelperTreeEntry],
    input: &mut impl Read,
) -> io::Result<()> {
    for item in entries {
        let path = safe_relative_mutation_path(root, &item.path)?;
        match item.kind {
            HelperEntryKind::Directory => fs::create_dir_all(path)?,
            HelperEntryKind::File => {
                if let Some(source) = &item.clone_from {
                    let source = safe_relative_path(root, source)?;
                    install_clone(path, item.size, item.overwrite, &source)?;
                } else {
                    install_file(path, item.size, item.overwrite, input, false)?;
                }
            }
            HelperEntryKind::Symlink | HelperEntryKind::Other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "bulk transfer supports only files and directories",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn install_clone(
    path: PathBuf,
    size: u64,
    overwrite: bool,
    source: &Path,
) -> io::Result<()> {
    if !overwrite && path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let temporary = parent.join(format!(
        ".abyss-clone-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    let mut destination = options.open(&temporary)?;
    let source = fs::File::open(source)?;
    if source.metadata()?.len() != size {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "verified clone source changed size before materialization",
        ));
    }
    #[cfg(target_os = "linux")]
    let cloned = rustix::fs::ioctl_ficlone(&destination, &source).is_ok();
    #[cfg(not(target_os = "linux"))]
    let cloned = false;
    if !cloned {
        let copied = io::copy(&mut source.take(size), &mut destination)?;
        if copied != size {
            let _ = fs::remove_file(&temporary);
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("clone source ended after {copied} of {size} bytes"),
            ));
        }
    }
    drop(destination);
    let result = if overwrite {
        fs::rename(&temporary, &path)
    } else {
        fs::hard_link(&temporary, &path).and_then(|_| fs::remove_file(&temporary))
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn payload_size(entries: &[HelperTreeEntry]) -> io::Result<u64> {
    entries
        .iter()
        .filter(|entry| matches!(entry.kind, HelperEntryKind::File) && entry.clone_from.is_none())
        .try_fold(0_u64, |total, entry| total.checked_add(entry.size))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree is too large"))
}

pub(crate) fn install_file(
    path: PathBuf,
    size: u64,
    overwrite: bool,
    input: &mut impl Read,
    sync: bool,
) -> io::Result<()> {
    if !overwrite && path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let temporary = parent.join(format!(
        ".abyss-upload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary)?;
    let copied = io::copy(&mut input.take(size), &mut file)?;
    if copied != size {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("upload ended after {copied} of {size} bytes"),
        ));
    }
    if sync {
        file.sync_all()?;
    }
    drop(file);
    let result = if overwrite {
        fs::rename(&temporary, &path)
    } else {
        fs::hard_link(&temporary, &path).and_then(|_| fs::remove_file(&temporary))
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn list_tree(root: &Path) -> io::Result<Vec<HelperTreeEntry>> {
    let mut result = Vec::new();
    let mut stack = vec![(root.to_owned(), Vec::<Vec<u8>>::new())];
    while let Some((directory, relative)) = stack.pop() {
        for item in fs::read_dir(directory)? {
            let item = item?;
            let name = item.file_name().into_vec();
            if name.starts_with(b"._") {
                continue;
            }
            let metadata = fs::symlink_metadata(item.path())?;
            let kind = metadata_kind(&metadata);
            if matches!(kind, HelperEntryKind::Symlink | HelperEntryKind::Other) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "bulk transfer refuses symbolic links and special files",
                ));
            }
            let mut path = relative.clone();
            path.push(name);
            result.push(HelperTreeEntry {
                path: path.clone(),
                kind,
                size: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
                overwrite: false,
                clone_from: None,
            });
            if metadata.is_dir() {
                stack.push((item.path(), path));
            }
        }
    }
    Ok(result)
}

pub(crate) fn read_tree(
    root: &[Vec<u8>],
    entries: &[HelperTreeEntry],
    compression: HelperCompression,
    output: &mut impl Write,
) -> io::Result<()> {
    let root = safe_path(root)?;
    let total = entries
        .iter()
        .filter(|entry| matches!(entry.kind, HelperEntryKind::File))
        .try_fold(0_u64, |total, entry| total.checked_add(entry.size))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree is too large"))?;
    write_frame(output, &HelperResult::Data { size: total })?;
    if matches!(compression, HelperCompression::None) {
        for item in entries {
            if !matches!(item.kind, HelperEntryKind::File) {
                continue;
            }
            let file = fs::File::open(safe_relative_path(&root, &item.path)?)?;
            let copied = io::copy(&mut file.take(item.size), output)?;
            if copied != item.size {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("file ended after {copied} of {} bytes", item.size),
                ));
            }
        }
        return output.flush();
    }
    let block_size = match compression {
        HelperCompression::None => unreachable!(),
        HelperCompression::Lz4 => LZ4_BLOCK,
        HelperCompression::Brotli => BROTLI_BLOCK,
        HelperCompression::Deflate => BROTLI_BLOCK,
    };
    let mut buffer = vec![0_u8; block_size];
    let mut buffered = 0;
    for item in entries {
        if !matches!(item.kind, HelperEntryKind::File) {
            continue;
        }
        let mut file = fs::File::open(safe_relative_path(&root, &item.path)?)?;
        let mut remaining = item.size;
        while remaining > 0 {
            let limit = (buffer.len() - buffered).min(remaining as usize);
            file.read_exact(&mut buffer[buffered..buffered + limit])?;
            buffered += limit;
            remaining -= limit as u64;
            if buffered == buffer.len() {
                write_compressed_block(output, &buffer, compression)?;
                buffered = 0;
            }
        }
    }
    if buffered > 0 {
        write_compressed_block(output, &buffer[..buffered], compression)?;
    }
    output.flush()
}
