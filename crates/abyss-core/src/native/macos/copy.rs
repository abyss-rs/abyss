use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, symlink};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::clone::{CloneAttempt, clone_to_temporary};
use super::copyfile::stream_to_temporary;
use super::metadata::{apply_path_metadata, apply_symlink_times, validate_source};
use super::temp::{Temporary, replace, temporary_path, usable_parent};
use super::{CloneCapabilities, CopyOutcome};
use crate::Error;
use crate::inventory::Entry;
use crate::progress::CopyStats;

pub fn copy_regular_file(
    source: &Path,
    destination: &Path,
    expected: &Entry,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    capabilities: &mut CloneCapabilities,
) -> Result<CopyOutcome, Error> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }

    let source_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(source)
        .map_err(|error| Error::io("open source", source, error))?;
    validate_source(source, &source_file, expected)?;
    unsafe {
        libc::fcntl(source_file.as_raw_fd(), libc::F_RDAHEAD, 1);
    };

    let parent = usable_parent(destination);
    let destination_dir = File::open(parent)
        .map_err(|error| Error::io("open destination directory", parent, error))?;
    let destination_device = destination_dir
        .metadata()
        .map_err(|error| Error::io("inspect destination directory", parent, error))?
        .dev();

    stats.begin_file(destination, expected.len);

    if capabilities.can_clone(expected.device, &destination_dir) {
        match clone_to_temporary(
            &source_file,
            &destination_dir,
            destination,
            expected,
            cancelled,
        ) {
            Ok(()) => {
                return Ok(CopyOutcome {
                    cloned: true,
                    ..CopyOutcome::default()
                });
            }
            Err(CloneAttempt::Unsupported(error))
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOTSUP) | Some(libc::EXDEV)
                ) =>
            {
                capabilities.disable(destination_device);
            }
            Err(CloneAttempt::Unsupported(error)) | Err(CloneAttempt::Fatal(error)) => {
                return Err(Error::io("clone to", destination, error));
            }
            Err(CloneAttempt::Cancelled) => return Err(Error::Cancelled),
        }
    }

    stream_to_temporary(&source_file, destination, expected, cancelled, stats).map(
        |physical_bytes| CopyOutcome {
            physical_bytes,
            ..CopyOutcome::default()
        },
    )
}

pub fn try_hard_link(
    existing_destination: &Path,
    destination: &Path,
    expected: &Entry,
) -> Result<Option<CopyOutcome>, Error> {
    for _ in 0..32 {
        let temporary = temporary_path(destination);
        match fs::hard_link(existing_destination, &temporary) {
            Ok(()) => {
                let guard = Temporary::new(temporary);
                apply_path_metadata(guard.path(), expected)?;
                replace(guard, destination)?;
                return Ok(Some(CopyOutcome {
                    linked: true,
                    ..CopyOutcome::default()
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOTSUP) | Some(libc::EXDEV) | Some(libc::EPERM)
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(Error::io("create hard link at", destination, error)),
        }
    }

    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}

pub fn copy_symlink(source: &Path, destination: &Path, expected: &Entry) -> Result<(), Error> {
    let value =
        fs::read_link(source).map_err(|error| Error::io("read symbolic link", source, error))?;

    for _ in 0..32 {
        let temporary = temporary_path(destination);
        match symlink(&value, &temporary) {
            Ok(()) => {
                let guard = Temporary::new(temporary);
                apply_symlink_times(guard.path(), expected)?;
                return replace(guard, destination);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::io("create symbolic link at", destination, error)),
        }
    }

    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}
