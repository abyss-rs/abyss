use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::fs_ops::remove_path;
use crate::Error;

pub(super) static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub(super) struct OwnedHandle(pub(super) windows_sys::Win32::Foundation::HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(super) fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub(super) fn temporary_path(destination: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    destination.with_file_name(format!(".abyss.{}.{}.tmp", std::process::id(), sequence))
}

pub(super) struct Temporary {
    path: PathBuf,
    committed: bool,
}

impl Temporary {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.committed {
            let directory = fs::symlink_metadata(&self.path)
                .is_ok_and(|metadata| metadata.file_attributes() & 0x10 != 0);
            let _ = remove_path(&self.path, directory);
        }
    }
}

pub(super) fn replace(mut temporary: Temporary, destination: &Path) -> Result<(), Error> {
    let temporary_wide = wide(&temporary.path);
    let destination_wide = wide(destination);
    let result = unsafe {
        MoveFileExW(
            temporary_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(Error::io(
            "replace",
            destination,
            io::Error::last_os_error(),
        ));
    }
    temporary.committed = true;
    Ok(())
}

pub(super) fn hresult_error(result: i32) -> io::Error {
    io::Error::from_raw_os_error(result & 0xffff)
}

pub(super) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), Error> {
    if cancelled.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
