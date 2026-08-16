use std::fs::{self, File};
use std::io::{self};
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_REPARSE_POINT, FSCTL_SET_REPARSE_POINT};
use windows_sys::Win32::System::SystemServices::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
};

use super::util::{OwnedHandle, Temporary, replace, temporary_path, wide};
use crate::Error;
use crate::inventory::Entry;
pub fn copy_symlink(source: &Path, destination: &Path, _expected: &Entry) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| Error::io("inspect Windows reparse point", source, error))?;
    let is_directory = metadata.file_attributes() & 0x10 != 0;
    let reparse = read_reparse_data(source)
        .map_err(|error| Error::io("read Windows reparse point", source, error))?;
    let tag = u32::from_le_bytes(reparse[..4].try_into().unwrap());
    if !matches!(tag, IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT) {
        return Err(Error::message(format!(
            "unsupported Windows reparse point tag 0x{tag:08X}: {}",
            source.display()
        )));
    }
    for _ in 0..32 {
        let temporary = temporary_path(destination);
        let result = if is_directory {
            fs::create_dir(&temporary)
        } else {
            File::create(&temporary).map(|_| ())
        };
        match result {
            Ok(()) => {
                let guard = Temporary::new(temporary);
                write_reparse_data(guard.path(), &reparse).map_err(|error| {
                    Error::io(
                        "create Windows symlink or junction (enable Developer Mode or grant symlink privilege) at",
                        destination,
                        error,
                    )
                })?;
                return replace(guard, destination);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(Error::io(
                    "create symbolic link (enable Windows Developer Mode or run with symlink privilege) at",
                    destination,
                    error,
                ));
            }
        }
    }
    Err(Error::message("could not reserve a temporary symlink name"))
}

pub(super) fn read_reparse_data(path: &Path) -> io::Result<Vec<u8>> {
    let handle = open_reparse(path, 0)?;
    let mut buffer = vec![0_u8; 16 * 1024];
    let mut returned = 0_u32;
    let result = unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_GET_REPARSE_POINT,
            ptr::null(),
            0,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    if returned < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated Windows reparse data",
        ));
    }
    buffer.truncate(returned as usize);
    Ok(buffer)
}

pub(super) fn write_reparse_data(path: &Path, data: &[u8]) -> io::Result<()> {
    let handle = open_reparse(path, GENERIC_WRITE)?;
    let input_len = 8_usize
        .checked_add(u16::from_le_bytes(data[4..6].try_into().unwrap()) as usize)
        .filter(|length| *length <= data.len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid reparse data length"))?;
    let mut returned = 0_u32;
    if unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_SET_REPARSE_POINT,
            data.as_ptr().cast(),
            input_len as u32,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn open_reparse(path: &Path, access: u32) -> io::Result<OwnedHandle> {
    let path = wide(path);
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(OwnedHandle(handle))
    }
}
