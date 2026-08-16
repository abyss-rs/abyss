use std::fs;
use std::io;
use std::path::Path;

use super::util::temporary_path;

pub fn remove_directory_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return remove_path(path, metadata.is_dir());
    }
    for child in fs::read_dir(path)? {
        let child = child?;
        let metadata = fs::symlink_metadata(child.path())?;
        if metadata.file_type().is_dir() {
            remove_directory_tree(&child.path())?;
        } else {
            remove_path(&child.path(), false)?;
        }
    }
    remove_path(path, true)
}

pub fn move_path(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source contains NUL"))?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination contains NUL"))?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS) | Some(libc::EINVAL)
    ) {
        fs::rename(
            std::ffi::OsStr::from_bytes(source.to_bytes()),
            std::ffi::OsStr::from_bytes(destination.to_bytes()),
        )
    } else {
        Err(error)
    }
}

pub fn remove_path(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(libc::AT_FDCWD, path.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn recover_unremovable_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recovery target is not a directory",
        ));
    }
    for _ in 0..32 {
        let recovery = temporary_path(path);
        match move_path(path, &recovery) {
            Ok(()) => match remove_directory_tree(&recovery) {
                Ok(()) => return Ok(()),
                Err(delete_error) => {
                    return match move_path(&recovery, path) {
                        Ok(()) => Err(delete_error),
                        Err(rollback_error) => Err(io::Error::new(
                            rollback_error.kind(),
                            format!(
                                "deletion failed ({delete_error}); rollback from {} also failed: {rollback_error}",
                                recovery.display()
                            ),
                        )),
                    };
                }
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a recovery name",
    ))
}
