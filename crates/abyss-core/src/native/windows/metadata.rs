use std::fs::{self, File};
use std::io::{self};
use std::mem::size_of;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FileBasicInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx, OPEN_EXISTING, SetFileInformationByHandle,
};

use super::WindowsFileIdentity;
use super::reparse::open_reparse;
use super::util::{OwnedHandle, wide};
use crate::Error;
use crate::inventory::Entry;
pub fn path_identity(path: &Path) -> io::Result<WindowsFileIdentity> {
    let handle = open_reparse(path, FILE_READ_ATTRIBUTES)?;
    identity_for_handle(handle.0)
}

pub(super) fn identity_for_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<WindowsFileIdentity> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(WindowsFileIdentity {
        volume: information.dwVolumeSerialNumber as u64,
        index: ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64,
        links: information.nNumberOfLinks as u64,
    })
}

pub fn apply_path_metadata(path: &Path, entry: &Entry) -> Result<(), Error> {
    let path_wide = wide(path);
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(Error::io(
            "open copied item",
            path,
            io::Error::last_os_error(),
        ));
    }
    let handle = OwnedHandle(handle);
    let mut basic = FILE_BASIC_INFO::default();
    let result = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(Error::io(
            "read Windows attributes for",
            path,
            io::Error::last_os_error(),
        ));
    }
    basic.LastAccessTime = entry.accessed_sec;
    basic.LastWriteTime = entry.modified_sec;
    basic.FileAttributes = entry.mode;
    let result = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileBasicInfo,
            (&basic as *const FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(Error::io(
            "set Windows metadata on",
            path,
            io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_source(source: &Path, file: &File, expected: &Entry) -> Result<(), Error> {
    let actual = file
        .metadata()
        .map_err(|error| Error::io("inspect open source", source, error))?;
    let identity = identity_for_handle(file.as_raw_handle() as _)
        .map_err(|error| Error::io("identify open source", source, error))?;
    if !actual.is_file()
        || identity.volume != expected.device
        || identity.index != expected.inode
        || actual.len() != expected.len
        || actual.last_write_time() != expected.modified_sec as u64
    {
        return Err(Error::message(format!(
            "source changed after it was scanned: {}",
            source.display()
        )));
    }
    Ok(())
}

pub(super) fn remove_file_readonly(path: &Path) -> io::Result<()> {
    clear_readonly(path)?;
    fs::remove_file(path)
}

pub(super) fn clear_readonly(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}
