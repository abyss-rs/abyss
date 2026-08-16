use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use crate::Error;
use crate::inventory::Entry;

pub fn apply_path_metadata(path: &Path, entry: &Entry) -> Result<(), Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| Error::io("open copied item", path, error))?;
    file.set_permissions(fs::Permissions::from_mode(entry.mode & 0o7777))
        .map_err(|error| Error::io("set permissions on", path, error))?;
    set_file_times(&file, path, entry)
}

pub(super) fn validate_source(source: &Path, file: &File, expected: &Entry) -> Result<(), Error> {
    let actual = file
        .metadata()
        .map_err(|error| Error::io("inspect open source", source, error))?;
    if !actual.is_file()
        || actual.dev() != expected.device
        || actual.ino() != expected.inode
        || actual.len() != expected.len
        || actual.mtime() != expected.modified_sec
        || actual.mtime_nsec() != expected.modified_nsec
    {
        return Err(Error::message(format!(
            "source changed after it was scanned: {}",
            source.display()
        )));
    }
    Ok(())
}

pub(super) fn set_file_times(file: &File, path: &Path, entry: &Entry) -> Result<(), Error> {
    let times = [
        libc::timespec {
            tv_sec: entry.accessed_sec,
            tv_nsec: entry.accessed_nsec,
        },
        libc::timespec {
            tv_sec: entry.modified_sec,
            tv_nsec: entry.modified_nsec,
        },
    ];
    // SAFETY: `file` is open and `times` contains exactly the two required timestamps.
    if unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(Error::io(
            "set timestamps on",
            path,
            io::Error::last_os_error(),
        ))
    }
}

pub(super) fn apply_symlink_times(path: &Path, entry: &Entry) -> Result<(), Error> {
    let path_c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::message(format!("path contains a NUL byte: {}", path.display())))?;
    let times = [
        libc::timespec {
            tv_sec: entry.accessed_sec,
            tv_nsec: entry.accessed_nsec,
        },
        libc::timespec {
            tv_sec: entry.modified_sec,
            tv_nsec: entry.modified_nsec,
        },
    ];
    // SAFETY: `path_c` and `times` are valid for the duration of the call.
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path_c.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOTSUP) | Some(libc::EPERM)
    ) {
        Ok(())
    } else {
        Err(Error::io("set symbolic-link timestamps on", path, error))
    }
}
