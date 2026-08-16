use std::ffi::{CString, c_void};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::temp::temporary_path;

pub(super) const REMOVEFILE_RECURSIVE: u32 = 1 << 0;

unsafe extern "C" {
    #[link_name = "removefile"]
    fn macos_removefile(path: *const libc::c_char, state: *mut c_void, flags: u32) -> libc::c_int;
}
pub fn remove_directory_tree(path: &Path) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path contains a NUL byte",
        )
    })?;
    // SAFETY: `path` is NUL-terminated and remains alive for the call. A null
    // state requests removefile's normal recursive behavior without callbacks.
    let result =
        unsafe { macos_removefile(path.as_ptr(), std::ptr::null_mut(), REMOVEFILE_RECURSIVE) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn move_path(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

pub fn remove_path(path: &Path, directory: bool) -> io::Result<()> {
    if directory {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
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
        match fs::rename(path, &recovery) {
            Ok(()) => match remove_directory_tree(&recovery) {
                Ok(()) => return Ok(()),
                Err(delete_error) => {
                    return match fs::rename(&recovery, path) {
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
        "could not reserve an ASCII recovery name",
    ))
}
