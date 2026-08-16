#[cfg(target_os = "macos")]
use std::ffi::CStr;
use std::ffi::OsString;
use std::fs;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::browser::scanner::local::{STREAM_BATCH, is_dot_underscore};
use crate::browser::types::{BrowserEntry, BrowserKind};

pub(crate) fn filesystem_hides_dot_underscore(path: &Path) -> std::io::Result<bool> {
    let directory = fs::File::open(path)?;
    let mut information = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `directory` remains open and `information` points to writable,
    // correctly aligned storage for the duration of the call.
    if unsafe { libc::fstatfs(directory.as_raw_fd(), information.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: A successful `fstatfs` initialized the complete structure.
    let information = unsafe { information.assume_init() };
    // SAFETY: Darwin guarantees that `f_fstypename` is NUL-terminated.
    let filesystem = unsafe { CStr::from_ptr(information.f_fstypename.as_ptr()) };
    Ok(hide_dot_underscore_for_filesystem(filesystem.to_bytes()))
}

pub(crate) fn hide_dot_underscore_for_filesystem(filesystem: &[u8]) -> bool {
    !matches!(filesystem, b"apfs" | b"hfs")
}

pub(crate) fn bulk_fallback_error(error: &std::io::Error) -> bool {
    error.raw_os_error().is_some_and(|code| {
        code == libc::ENOTSUP
            || code == libc::ENOSYS
            || code == libc::EINVAL
            || code == libc::ENOTTY
            || code == libc::EOPNOTSUPP
    })
}

pub(crate) fn read_getattrlistbulk(
    path: &Path,
    hide_dot_underscore: bool,
    emit: &impl Fn(Vec<BrowserEntry>) -> bool,
) -> std::io::Result<Vec<BrowserEntry>> {
    let directory = fs::File::open(path)?;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS
            | libc::ATTR_CMN_NAME
            | libc::ATTR_CMN_OBJTYPE
            | libc::ATTR_CMN_MODTIME
            | libc::ATTR_CMN_ACCESSMASK,
        volattr: 0,
        dirattr: 0,
        fileattr: libc::ATTR_FILE_DATALENGTH,
        forkattr: 0,
    };
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut all = Vec::new();
    let mut ordinal = 1_u64;
    loop {
        let count = unsafe {
            libc::getattrlistbulk(
                directory.as_raw_fd(),
                (&mut attributes as *mut libc::attrlist).cast(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                libc::FSOPT_PACK_INVAL_ATTRS as u64,
            )
        };
        if count < 0 {
            if all.is_empty() {
                return Err(std::io::Error::last_os_error());
            }
            return Err(std::io::Error::last_os_error());
        }
        if count == 0 {
            break;
        }
        let mut cursor = 0_usize;
        let mut batch = Vec::with_capacity(STREAM_BATCH);
        for _ in 0..count {
            let length = read_unaligned::<u32>(&buffer, cursor)? as usize;
            if length < 4
                || cursor
                    .checked_add(length)
                    .is_none_or(|end| end > buffer.len())
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "getattrlistbulk returned an invalid record",
                ));
            }
            let record = &buffer[cursor..cursor + length];
            let mut entry = parse_bulk_record(record, ordinal)?;
            cursor += length;
            if hide_dot_underscore && is_dot_underscore(&entry.name) {
                continue;
            }
            entry.ordinal = ordinal;
            ordinal = ordinal.saturating_add(1);
            all.push(entry.clone());
            batch.push(entry);
            if batch.len() == STREAM_BATCH {
                if !emit(std::mem::take(&mut batch)) {
                    return Ok(all);
                }
                batch.reserve(STREAM_BATCH);
            }
        }
        if !batch.is_empty() && !emit(batch) {
            return Ok(all);
        }
    }
    Ok(all)
}

pub(crate) fn parse_bulk_record(record: &[u8], ordinal: u64) -> std::io::Result<BrowserEntry> {
    let mut cursor = 4_usize;
    let returned = read_unaligned::<libc::attribute_set_t>(record, cursor)?;
    cursor += std::mem::size_of::<libc::attribute_set_t>();

    let name = if returned.commonattr & libc::ATTR_CMN_NAME != 0 {
        let reference_offset = cursor;
        let reference = read_unaligned::<libc::attrreference_t>(record, cursor)?;
        cursor += std::mem::size_of::<libc::attrreference_t>();
        let start = reference_offset.checked_add_signed(reference.attr_dataoffset as isize);
        let end = start.and_then(|start| start.checked_add(reference.attr_length as usize));
        let Some((start, end)) = start.zip(end) else {
            return Err(invalid_bulk_name());
        };
        if end > record.len() || start >= end {
            return Err(invalid_bulk_name());
        }
        let bytes = &record[start..end];
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        OsString::from_vec(bytes.to_vec())
    } else {
        return Err(invalid_bulk_name());
    };

    let object_type = if returned.commonattr & libc::ATTR_CMN_OBJTYPE != 0 {
        let value = read_unaligned::<u32>(record, cursor)?;
        cursor += std::mem::size_of::<u32>();
        Some(value)
    } else {
        None
    };
    let modified = if returned.commonattr & libc::ATTR_CMN_MODTIME != 0 {
        let value = read_unaligned::<libc::timespec>(record, cursor)?;
        cursor += std::mem::size_of::<libc::timespec>();
        system_time(value.tv_sec, value.tv_nsec)
    } else {
        None
    };
    let mode = if returned.commonattr & libc::ATTR_CMN_ACCESSMASK != 0 {
        let value = read_unaligned::<u32>(record, cursor)?;
        cursor += std::mem::size_of::<u32>();
        Some(value)
    } else {
        None
    };
    let size = if returned.fileattr & libc::ATTR_FILE_DATALENGTH != 0 {
        Some(read_unaligned::<i64>(record, cursor)?.max(0) as u64)
    } else {
        None
    };
    let kind = match object_type {
        Some(1) => BrowserKind::File,
        Some(2) => BrowserKind::Directory,
        Some(5) => BrowserKind::Symlink,
        Some(_) => BrowserKind::Other,
        None => BrowserKind::Unknown,
    };
    Ok(BrowserEntry {
        name,
        raw_name: None,
        kind,
        size,
        modified,
        mode,
        ordinal,
    })
}

pub(crate) fn read_unaligned<T: Copy>(buffer: &[u8], offset: usize) -> std::io::Result<T> {
    let end = offset
        .checked_add(std::mem::size_of::<T>())
        .filter(|end| *end <= buffer.len())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated getattrlistbulk record",
            )
        })?;
    let _ = end;
    Ok(unsafe { (buffer.as_ptr().add(offset) as *const T).read_unaligned() })
}

pub(crate) fn invalid_bulk_name() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "getattrlistbulk returned an invalid name",
    )
}

pub(crate) fn system_time(seconds: i64, nanoseconds: i64) -> Option<SystemTime> {
    if !(0..1_000_000_000).contains(&nanoseconds) {
        return None;
    }
    let duration = Duration::new(seconds.unsigned_abs(), nanoseconds as u32);
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
}
