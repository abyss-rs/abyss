use std::ffi::CString;
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::CloneCapabilities;
use super::metadata::apply_path_metadata;
use super::temp::{Temporary, error_to_io, replace, temporary_path};
use crate::inventory::Entry;

impl CloneCapabilities {
    pub(super) fn can_clone(&mut self, source_device: u64, destination_dir: &File) -> bool {
        let Ok(metadata) = destination_dir.metadata() else {
            return false;
        };
        let destination_device = metadata.dev();
        if source_device != destination_device {
            return false;
        }

        *self
            .by_destination_device
            .entry(destination_device)
            .or_insert_with(|| query_clone_capability(destination_dir).unwrap_or(false))
    }

    pub(super) fn disable(&mut self, destination_device: u64) {
        self.by_destination_device.insert(destination_device, false);
    }
}

pub(super) enum CloneAttempt {
    Unsupported(io::Error),
    Fatal(io::Error),
    Cancelled,
}

pub(super) fn clone_to_temporary(
    source: &File,
    destination_dir: &File,
    destination: &Path,
    expected: &Entry,
    cancelled: &AtomicBool,
) -> Result<(), CloneAttempt> {
    for _ in 0..32 {
        if cancelled.load(Ordering::Relaxed) {
            return Err(CloneAttempt::Cancelled);
        }

        let temporary = temporary_path(destination);
        let name = temporary
            .file_name()
            .ok_or_else(|| {
                CloneAttempt::Fatal(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary path has no file name",
                ))
            })
            .and_then(|name| {
                CString::new(name.as_bytes()).map_err(|_| {
                    CloneAttempt::Fatal(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "destination name contains a NUL byte",
                    ))
                })
            })?;

        // SAFETY: Both descriptors remain open for the call and `name` is NUL-terminated.
        let result = unsafe {
            libc::fclonefileat(
                source.as_raw_fd(),
                destination_dir.as_raw_fd(),
                name.as_ptr(),
                0,
            )
        };
        if result == 0 {
            let guard = Temporary::new(temporary);
            apply_path_metadata(guard.path(), expected)
                .map_err(|error| CloneAttempt::Fatal(error_to_io(error)))?;
            replace(guard, destination).map_err(|error| CloneAttempt::Fatal(error_to_io(error)))?;
            return Ok(());
        }

        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EEXIST) => continue,
            Some(libc::ENOTSUP) | Some(libc::EXDEV) => {
                return Err(CloneAttempt::Unsupported(error));
            }
            _ => return Err(CloneAttempt::Fatal(error)),
        }
    }

    Err(CloneAttempt::Fatal(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary name",
    )))
}

#[repr(C)]
pub(super) struct CapabilityBuffer {
    length: u32,
    capabilities: libc::vol_capabilities_attr_t,
}

pub(super) fn query_clone_capability(directory: &File) -> io::Result<bool> {
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buffer = MaybeUninit::<CapabilityBuffer>::zeroed();

    // SAFETY: The attribute list and output buffer have the ABI layouts from sys/attr.h.
    let result = unsafe {
        libc::fgetattrlist(
            directory.as_raw_fd(),
            (&raw mut attributes).cast(),
            buffer.as_mut_ptr().cast(),
            size_of::<CapabilityBuffer>(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: fgetattrlist succeeded and initialized the requested fixed-size value.
    let buffer = unsafe { buffer.assume_init() };
    Ok(clone_supported(&buffer.capabilities))
}

pub(super) fn clone_supported(capabilities: &libc::vol_capabilities_attr_t) -> bool {
    let index = libc::VOL_CAPABILITIES_INTERFACES;
    capabilities.valid[index] & libc::VOL_CAP_INT_CLONE != 0
        && capabilities.capabilities[index] & libc::VOL_CAP_INT_CLONE != 0
}
