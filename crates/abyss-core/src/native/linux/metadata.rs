use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;

use super::CopyOutcome;
use super::util::{Temporary, replace, temporary_path};
use crate::Error;
use crate::inventory::Entry;

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
                    Some(libc::EOPNOTSUPP) | Some(libc::EXDEV) | Some(libc::EPERM)
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(Error::io("create hard link at", destination, error)),
        }
    }
    Err(Error::message(
        "could not reserve a temporary hard-link name",
    ))
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
    Err(Error::message("could not reserve a temporary symlink name"))
}

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
            tv_sec: entry.accessed_sec as libc::time_t,
            tv_nsec: entry.accessed_nsec as libc::c_long,
        },
        libc::timespec {
            tv_sec: entry.modified_sec as libc::time_t,
            tv_nsec: entry.modified_nsec as libc::c_long,
        },
    ];
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
    use std::os::unix::ffi::OsStrExt;
    let value = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::message(format!("path contains a NUL byte: {}", path.display())))?;
    let times = [
        libc::timespec {
            tv_sec: entry.accessed_sec as libc::time_t,
            tv_nsec: entry.accessed_nsec as libc::c_long,
        },
        libc::timespec {
            tv_sec: entry.modified_sec as libc::time_t,
            tv_nsec: entry.modified_nsec as libc::c_long,
        },
    ];
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            value.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::EOPNOTSUPP) | Some(libc::EPERM)
        ) {
            Ok(())
        } else {
            Err(Error::io("set symbolic-link timestamps on", path, error))
        }
    }
}
