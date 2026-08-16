use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::metadata::{apply_path_metadata, set_file_times, validate_source};
use super::sparse::buffered_sparse_copy;
use super::util::{
    Temporary, check_cancelled, check_cancelled_io, error_to_io, replace, temporary_path,
    usable_parent,
};
use super::xattr::copy_xattrs;
use super::{CloneCapabilities, CopyOutcome};
use crate::Error;
use crate::inventory::Entry;
use crate::progress::CopyStats;

pub(super) const FICLONE: libc::c_ulong = 0x4004_9409;
impl CloneCapabilities {
    fn can_clone(&mut self, source_device: u64, destination: &File) -> bool {
        let Ok(metadata) = destination.metadata() else {
            return false;
        };
        source_device == metadata.dev()
            && *self
                .by_destination_device
                .entry(metadata.dev())
                .or_insert(true)
    }

    fn disable(&mut self, device: u64) {
        self.by_destination_device.insert(device, false);
    }
}

pub fn copy_regular_file(
    source: &Path,
    destination: &Path,
    expected: &Entry,
    cancelled: &Arc<AtomicBool>,
    stats: &Arc<CopyStats>,
    capabilities: &mut CloneCapabilities,
) -> Result<CopyOutcome, Error> {
    check_cancelled(cancelled)?;
    let source_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(source)
        .map_err(|error| Error::io("open source", source, error))?;
    validate_source(source, &source_file, expected)?;
    unsafe {
        libc::posix_fadvise(source_file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
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
        match clone_to_temporary(&source_file, destination, expected, cancelled) {
            Ok(()) => {
                return Ok(CopyOutcome {
                    cloned: true,
                    ..CopyOutcome::default()
                });
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::EOPNOTSUPP)
                        | Some(libc::ENOTTY)
                        | Some(libc::EINVAL)
                        | Some(libc::EXDEV)
                ) =>
            {
                capabilities.disable(destination_device);
            }
            Err(error) => return Err(Error::io("clone to", destination, error)),
        }
    }

    stream_to_temporary(&source_file, destination, expected, cancelled, stats).map(
        |physical_bytes| CopyOutcome {
            physical_bytes,
            ..CopyOutcome::default()
        },
    )
}

pub(super) fn clone_to_temporary(
    source: &File,
    destination: &Path,
    expected: &Entry,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    check_cancelled_io(cancelled)?;
    for _ in 0..32 {
        let temporary = temporary_path(destination);
        let target = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let guard = Temporary::new(temporary);
        let result = unsafe { libc::ioctl(target.as_raw_fd(), FICLONE, source.as_raw_fd()) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        apply_path_metadata(guard.path(), expected).map_err(error_to_io)?;
        drop(target);
        replace(guard, destination).map_err(error_to_io)?;
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary name",
    ))
}

pub(super) fn stream_to_temporary(
    source: &File,
    destination: &Path,
    expected: &Entry,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<u64, Error> {
    for _ in 0..32 {
        let temporary = temporary_path(destination);
        let target = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(Error::io(
                    "create temporary file beside",
                    destination,
                    error,
                ));
            }
        };
        let guard = Temporary::new(temporary);
        let physical = copy_file_ranges(source, &target, expected.len, cancelled, stats)
            .or_else(|error| {
                if fallback_copy_error(&error) {
                    buffered_sparse_copy(source, &target, expected.len, cancelled, stats)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| Error::io("copy data to", destination, error))?;
        let _ = copy_xattrs(source, &target);
        target
            .set_permissions(fs::Permissions::from_mode(expected.mode & 0o7777))
            .map_err(|error| Error::io("set permissions on", guard.path(), error))?;
        set_file_times(&target, guard.path(), expected)?;
        drop(target);
        replace(guard, destination)?;
        return Ok(physical);
    }
    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}

pub(super) fn copy_file_ranges(
    source: &File,
    target: &File,
    length: u64,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> io::Result<u64> {
    let mut source_offset: libc::loff_t = 0;
    let mut target_offset: libc::loff_t = 0;
    let mut total = 0_u64;
    while total < length {
        check_cancelled_io(cancelled)?;
        let amount = usize::try_from((length - total).min(8 * 1024 * 1024)).unwrap();
        let result = unsafe {
            libc::copy_file_range(
                source.as_raw_fd(),
                &mut source_offset,
                target.as_raw_fd(),
                &mut target_offset,
                amount,
                0,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if result == 0 {
            break;
        }
        if !stats.wait_for_transfer(cancelled, result as u64) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy cancelled"));
        }
        total += result as u64;
        stats.current_copied.store(total, Ordering::Relaxed);
    }
    if total == length {
        Ok(total)
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "kernel copy stopped before end of file",
        ))
    }
}
pub(super) fn fallback_copy_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS)
            | Some(libc::EXDEV)
            | Some(libc::EINVAL)
            | Some(libc::EOPNOTSUPP)
            | Some(libc::EPERM)
    )
}
