use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use super::util::check_cancelled_io;
use crate::progress::CopyStats;

pub(super) fn buffered_sparse_copy(
    source: &File,
    target: &File,
    length: u64,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> io::Result<u64> {
    seek_sparse_copy(source, target, length, cancelled, stats)
        .or_else(|_| scan_sparse_copy(source, target, length, cancelled, stats))
}

pub(super) fn seek_sparse_copy(
    source: &File,
    target: &File,
    length: u64,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> io::Result<u64> {
    let mut current_offset: libc::off_t = 0;
    let mut physical = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];

    while (current_offset as u64) < length {
        check_cancelled_io(cancelled)?;
        let data_start =
            unsafe { libc::lseek(source.as_raw_fd(), current_offset, libc::SEEK_DATA) };
        if data_start < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENXIO) {
                break;
            }
            return Err(error);
        }

        let data_end = unsafe { libc::lseek(source.as_raw_fd(), data_start, libc::SEEK_HOLE) };
        if data_end < 0 {
            return Err(io::Error::last_os_error());
        }

        let segment_end = (data_end as u64).min(length);
        let mut segment_offset = data_start as u64;

        let mut source_clone = source.try_clone()?;
        let mut target_clone = target.try_clone()?;
        source_clone.seek(SeekFrom::Start(segment_offset))?;
        target_clone.seek(SeekFrom::Start(segment_offset))?;

        while segment_offset < segment_end {
            check_cancelled_io(cancelled)?;
            let to_read =
                usize::try_from((segment_end - segment_offset).min(buffer.len() as u64)).unwrap();
            let amount = source_clone.read(&mut buffer[..to_read])?;
            if amount == 0 {
                break;
            }
            if !stats.wait_for_transfer(cancelled, amount as u64) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "copy cancelled"));
            }
            target_clone.write_all(&buffer[..amount])?;
            physical = physical.saturating_add(amount as u64);
            segment_offset = segment_offset.saturating_add(amount as u64);
            stats
                .current_copied
                .store(segment_offset, Ordering::Relaxed);
        }

        current_offset = segment_end as libc::off_t;
    }

    target.set_len(length)?;
    stats.current_copied.store(length, Ordering::Relaxed);
    Ok(physical)
}

pub(super) fn scan_sparse_copy(
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
            physical += amount as u64;
        }
        logical += amount as u64;
        stats.current_copied.store(logical, Ordering::Relaxed);
    }
    target.set_len(length)?;
    if logical == length {
        Ok(physical)
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "source ended before its scanned length",
        ))
    }
}
