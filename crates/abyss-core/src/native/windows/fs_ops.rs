use std::fs::{self};
use std::io::{self};
use std::mem::size_of;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfoEx,
    MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_EXISTING, SetFileInformationByHandle,
};

use super::metadata::{clear_readonly, remove_file_readonly};
use super::util::{OwnedHandle, temporary_path, wide};
pub fn remove_directory_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return remove_path(path, metadata.is_dir());
    }
    for child in fs::read_dir(path)? {
        let child = child?;
        let metadata = fs::symlink_metadata(child.path())?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            remove_directory_tree(&child.path())?;
        } else if metadata.is_dir() {
            remove_path(&child.path(), true)?;
        } else {
            remove_path(&child.path(), false)?;
        }
    }
    remove_path(path, true)
}

pub fn move_path(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source);
    let destination = wide(destination);
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn remove_path(path: &Path, directory: bool) -> io::Result<()> {
    match dispose_path(path) {
        Ok(()) => Ok(()),
        Err(native_error) => {
            let metadata = fs::symlink_metadata(path)?;
            let fallback = if directory || metadata.is_dir() {
                clear_readonly(path)?;
                fs::remove_dir(path)
            } else {
                remove_file_readonly(path)
            };
            fallback.map_err(|fallback_error| {
                io::Error::new(
                    fallback_error.kind(),
                    format!(
                        "native disposition failed ({native_error}); legacy deletion failed: {fallback_error}"
                    ),
                )
            })
        }
    }
}

fn dispose_path(path: &Path) -> io::Result<()> {
    let path_wide = wide(path);
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = OwnedHandle(handle);
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let result = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfoEx,
            (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn recover_unremovable_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recovery target is not a real directory",
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
