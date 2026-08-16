use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows_sys::Win32::Storage::FileSystem::{
    COPY_FILE_FAIL_IF_EXISTS, COPYFILE2_CALLBACK_CHUNK_FINISHED,
    COPYFILE2_CALLBACK_STREAM_FINISHED, COPYFILE2_EXTENDED_PARAMETERS, COPYFILE2_MESSAGE,
    COPYFILE2_MESSAGE_ACTION, COPYFILE2_PROGRESS_CANCEL, COPYFILE2_PROGRESS_CONTINUE, CopyFile2,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    FILE_ALLOCATED_RANGE_BUFFER, FSCTL_QUERY_ALLOCATED_RANGES, FSCTL_SET_SPARSE,
};

use super::metadata::{apply_path_metadata, validate_source};
use super::util::{Temporary, check_cancelled, hresult_error, replace, temporary_path, wide};
use super::{CloneCapabilities, CopyOutcome};
use crate::Error;
use crate::inventory::Entry;
use crate::progress::CopyStats;
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
        .share_mode(1)
        .open(source)
        .map_err(|error| Error::io("open source", source, error))?;
    validate_source(source, &source_file, expected)?;
    capabilities.observe(expected.device);
    stats.begin_file(destination, expected.len);

    for _ in 0..32 {
        let temporary = temporary_path(destination);
        let source_wide = wide(source);
        let temporary_wide = wide(&temporary);
        let context = CopyContext {
            cancelled,
            copied: &stats.current_copied,
            stats: Some(stats),
        };
        let parameters = COPYFILE2_EXTENDED_PARAMETERS {
            dwSize: size_of::<COPYFILE2_EXTENDED_PARAMETERS>() as u32,
            dwCopyFlags: COPY_FILE_FAIL_IF_EXISTS,
            pfCancel: ptr::null_mut(),
            pProgressRoutine: Some(copy_progress),
            pvCallbackContext: (&context as *const CopyContext<'_>).cast_mut().cast(),
        };
        let result =
            unsafe { CopyFile2(source_wide.as_ptr(), temporary_wide.as_ptr(), &parameters) };
        if result >= 0 {
            let guard = Temporary::new(temporary);
            apply_path_metadata(guard.path(), expected)?;
            replace(guard, destination)?;
            return Ok(CopyOutcome {
                physical_bytes: expected.len,
                ..CopyOutcome::default()
            });
        }
        let error = hresult_error(result);
        if error.kind() == io::ErrorKind::AlreadyExists {
            continue;
        }
        if matches!(error.raw_os_error(), Some(1) | Some(50) | Some(120)) {
            return stream_to_temporary(&source_file, destination, expected, cancelled, stats);
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        return Err(Error::io("copy data to", destination, error));
    }
    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}

pub(super) fn stream_to_temporary(
    source: &File,
    destination: &Path,
    expected: &Entry,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<CopyOutcome, Error> {
    for _ in 0..32 {
        let temporary = temporary_path(destination);
        let target = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
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
        let mut returned = 0_u32;
        unsafe {
            DeviceIoControl(
                target.as_raw_handle() as _,
                FSCTL_SET_SPARSE,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            );
        }

        let physical = copy_allocated_ranges(source, &target, expected.len, cancelled, stats)
            .or_else(|_| copy_streaming_fallback(source, &target, expected.len, cancelled, stats))
            .map_err(|error| Error::io("copy data to", destination, error))?;

        target
            .set_len(expected.len)
            .map_err(|error| Error::io("set copied length on", destination, error))?;
        drop(target);
        apply_path_metadata(guard.path(), expected)?;
        replace(guard, destination)?;
        return Ok(CopyOutcome {
            physical_bytes: physical,
            ..CopyOutcome::default()
        });
    }
    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}

pub(super) fn copy_allocated_ranges(
    source: &File,
    target: &File,
    length: u64,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> io::Result<u64> {
    let query = FILE_ALLOCATED_RANGE_BUFFER {
        FileOffset: 0,
        Length: length as i64,
    };
    let mut ranges = vec![
        FILE_ALLOCATED_RANGE_BUFFER {
            FileOffset: 0,
            Length: 0
        };
        1024
    ];
    let mut returned = 0_u32;
    let result = unsafe {
        DeviceIoControl(
            source.as_raw_handle() as _,
            FSCTL_QUERY_ALLOCATED_RANGES,
            (&query as *const FILE_ALLOCATED_RANGE_BUFFER).cast(),
            size_of::<FILE_ALLOCATED_RANGE_BUFFER>() as u32,
            ranges.as_mut_ptr().cast(),
            (ranges.len() * size_of::<FILE_ALLOCATED_RANGE_BUFFER>()) as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let count = (returned as usize) / size_of::<FILE_ALLOCATED_RANGE_BUFFER>();
    if count == 0 && length > 0 {
        stats.current_copied.store(length, Ordering::Relaxed);
        return Ok(0);
    }

    let mut source_clone = source.try_clone()?;
    let mut target_clone = target.try_clone()?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut physical = 0_u64;

    for range in &ranges[..count] {
        check_cancelled_io(cancelled)?;
        let start = range.FileOffset as u64;
        let range_len = range.Length as u64;
        let mut offset = 0_u64;

        source_clone.seek(SeekFrom::Start(start))?;
        target_clone.seek(SeekFrom::Start(start))?;

        while offset < range_len {
            check_cancelled_io(cancelled)?;
            let to_read = usize::try_from((range_len - offset).min(buffer.len() as u64)).unwrap();
            let amount = source_clone.read(&mut buffer[..to_read])?;
            if amount == 0 {
                break;
            }
            if !stats.wait_for_transfer(cancelled, amount as u64) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "copy cancelled"));
            }
            target_clone.write_all(&buffer[..amount])?;
            physical = physical.saturating_add(amount as u64);
            offset = offset.saturating_add(amount as u64);
            stats
                .current_copied
                .store(start + offset, Ordering::Relaxed);
        }
    }

    stats.current_copied.store(length, Ordering::Relaxed);
    Ok(physical)
}

pub(super) fn copy_streaming_fallback(
    source: &File,
    target: &File,
    length: u64,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> io::Result<u64> {
    let mut source = source.try_clone()?;
    let mut target = target.try_clone()?;
    source.seek(SeekFrom::Start(0))?;
    target.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut logical = 0_u64;
    let mut physical = 0_u64;
    while logical < length {
        check_cancelled_io(cancelled)?;
        let amount = source.read(&mut buffer)?;
        if amount == 0 {
            break;
        }
        if !stats.wait_for_transfer(cancelled, amount as u64) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "copy cancelled"));
        }
        if buffer[..amount].iter().all(|byte| *byte == 0) {
            target.seek(SeekFrom::Current(amount as i64))?;
        } else {
            target.write_all(&buffer[..amount])?;
            physical = physical.saturating_add(amount as u64);
        }
        logical = logical.saturating_add(amount as u64);
        stats.current_copied.store(logical, Ordering::Relaxed);
    }
    if logical == length {
        Ok(physical)
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "source ended before its scanned length",
        ))
    }
}

pub(super) fn check_cancelled_io(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation cancelled",
        ))
    } else {
        Ok(())
    }
}

pub(super) struct CopyContext<'a> {
    cancelled: &'a AtomicBool,
    copied: &'a AtomicU64,
    stats: Option<&'a CopyStats>,
}

unsafe extern "system" fn copy_progress(
    message: *const COPYFILE2_MESSAGE,
    context: *const c_void,
) -> COPYFILE2_MESSAGE_ACTION {
    if context.is_null() {
        return COPYFILE2_PROGRESS_CANCEL;
    }
    let context = unsafe { &*context.cast::<CopyContext<'_>>() };
    if context.cancelled.load(Ordering::Relaxed) {
        return COPYFILE2_PROGRESS_CANCEL;
    }
    if !message.is_null() {
        let message = unsafe { &*message };
        let transferred = match message.Type {
            COPYFILE2_CALLBACK_CHUNK_FINISHED => {
                Some(unsafe { message.Info.ChunkFinished.uliTotalBytesTransferred })
            }
            COPYFILE2_CALLBACK_STREAM_FINISHED => {
                Some(unsafe { message.Info.StreamFinished.uliTotalBytesTransferred })
            }
            _ => None,
        };
        if let Some(transferred) = transferred {
            let previous = context.copied.swap(transferred, Ordering::Relaxed);
            if let Some(stats) = context.stats
                && !stats.wait_for_transfer(context.cancelled, transferred.saturating_sub(previous))
            {
                return COPYFILE2_PROGRESS_CANCEL;
            }
        }
    }
    COPYFILE2_PROGRESS_CONTINUE
}

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
                    error.kind(),
                    io::ErrorKind::Unsupported
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::CrossesDevices
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
