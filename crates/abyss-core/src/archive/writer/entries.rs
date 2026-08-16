use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::progress::{CopyStats, human_bytes};
pub(crate) fn ensure_create_space(parent: &Path, input_bytes: u64) -> Result<(), Error> {
    // Worst case for already-compressed input is ~input size; keep a small margin for tar headers.
    let needed = input_bytes
        .saturating_add(input_bytes / 100)
        .saturating_add(16 << 20);
    let Some(available) = available_bytes(parent) else {
        return Ok(());
    };
    if available >= needed {
        return Ok(());
    }
    Err(Error::message(format!(
        "not enough free space to create archive in {}: need about {}, have {}",
        parent.display(),
        human_bytes(needed),
        human_bytes(available),
    )))
}

#[allow(clippy::useless_conversion)]
pub(crate) fn available_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: path is NUL-terminated; stats is written only on success.
        let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
        if result != 0 {
            return None;
        }
        let stats = unsafe { stats.assume_init() };
        Some(u64::from(stats.f_bavail).saturating_mul(u64::from(stats.f_frsize)))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[derive(Debug)]
pub(crate) struct CreateEntry {
    pub(crate) source: PathBuf,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) is_directory: bool,
}

pub(crate) fn collect_create_entries(
    sources: &[PathBuf],
    destination: &Path,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<Vec<CreateEntry>, Error> {
    let mut entries = Vec::new();
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let name = source
            .file_name()
            .ok_or_else(|| Error::message(format!("cannot archive {}", source.display())))?
            .to_string_lossy()
            .into_owned();
        collect_create_entry(source, &name, destination, cancelled, stats, &mut entries)?;
    }
    Ok(entries)
}

pub(crate) fn collect_create_entry(
    source: &Path,
    name: &str,
    destination: &Path,
    cancelled: &AtomicBool,
    stats: &CopyStats,
    entries: &mut Vec<CreateEntry>,
) -> Result<(), Error> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if source == destination {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|error| Error::io("inspect archive input", source, error))?;
    stats.observe_scan(source);
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let is_directory = metadata.is_dir();
    if !is_directory && !metadata.is_file() {
        return Err(Error::message(format!(
            "unsupported archive input: {}",
            source.display()
        )));
    }
    entries.push(CreateEntry {
        source: source.to_owned(),
        name: name.replace('\\', "/"),
        size: if is_directory { 0 } else { metadata.len() },
        is_directory,
    });
    if is_directory {
        let mut children = std::fs::read_dir(source)
            .map_err(|error| Error::io("read archive input directory", source, error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| Error::io("read archive input directory", source, error))?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let child_name = format!("{name}/{}", child.file_name().to_string_lossy());
            collect_create_entry(
                &child.path(),
                &child_name,
                destination,
                cancelled,
                stats,
                entries,
            )?;
        }
    }
    Ok(())
}
