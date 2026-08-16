use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::metadata::set_file_times;
use super::temp::{Temporary, replace, temporary_path};
use crate::Error;
use crate::inventory::Entry;
use crate::progress::CopyStats;

pub(super) fn stream_to_temporary(
    source: &File,
    destination: &Path,
    expected: &Entry,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<u64, Error> {
    for _ in 0..32 {
        let temporary_path = temporary_path(destination);
        let temporary_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(&temporary_path)
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
        let guard = Temporary::new(temporary_path);
        let physical = fcopyfile(source, &temporary_file, cancelled, stats, destination)?;
        temporary_file
            .set_permissions(fs::Permissions::from_mode(expected.mode & 0o7777))
            .map_err(|error| Error::io("set permissions on", guard.path(), error))?;
        set_file_times(&temporary_file, guard.path(), expected)?;
        drop(temporary_file);
        replace(guard, destination)?;
        return Ok(physical);
    }

    Err(Error::message(format!(
        "could not reserve a temporary name beside {}",
        destination.display()
    )))
}

pub(super) struct CallbackContext<'a> {
    pub(super) cancelled: &'a AtomicBool,
    pub(super) copied: &'a AtomicU64,
    pub(super) stats: Option<&'a CopyStats>,
}

pub(super) extern "C" fn copy_callback(
    what: libc::c_int,
    _stage: libc::c_int,
    state: libc::copyfile_state_t,
    _source: *const libc::c_char,
    _destination: *const libc::c_char,
    context: *mut c_void,
) -> libc::c_int {
    if context.is_null() {
        return libc::COPYFILE_QUIT;
    }

    // SAFETY: `context` points to a CallbackContext that outlives fcopyfile.
    let context = unsafe { &*(context.cast::<CallbackContext<'_>>()) };
    if context.cancelled.load(Ordering::Relaxed) {
        return libc::COPYFILE_QUIT;
    }

    if what == libc::COPYFILE_COPY_DATA {
        let mut bytes: libc::off_t = 0;
        // SAFETY: `state` is supplied by copyfile and `bytes` has the documented type.
        let result = unsafe {
            libc::copyfile_state_get(
                state,
                libc::COPYFILE_STATE_COPIED as u32,
                (&raw mut bytes).cast(),
            )
        };
        if result == 0 && bytes >= 0 {
            let bytes = bytes as u64;
            let previous = context.copied.swap(bytes, Ordering::Relaxed);
            if let Some(stats) = context.stats
                && !stats.wait_for_transfer(context.cancelled, bytes.saturating_sub(previous))
            {
                return libc::COPYFILE_QUIT;
            }
        }
    }

    libc::COPYFILE_CONTINUE
}

pub(super) fn fcopyfile(
    source: &File,
    destination: &File,
    cancelled: &AtomicBool,
    stats: &CopyStats,
    destination_path: &Path,
) -> Result<u64, Error> {
    let state = CopyfileState::new()
        .map_err(|error| Error::io("allocate native copy state for", destination_path, error))?;
    let mut context = CallbackContext {
        cancelled,
        copied: &stats.current_copied,
        stats: Some(stats),
    };
    let callback = copy_callback as *const () as *const c_void;

    // SAFETY: The callback and its stack context remain valid until fcopyfile returns.
    let callback_result = unsafe {
        libc::copyfile_state_set(state.0, libc::COPYFILE_STATE_STATUS_CB as u32, callback)
    };
    if callback_result != 0 {
        return Err(Error::io(
            "configure native copy callback for",
            destination_path,
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: The context remains alive and pinned on this stack during fcopyfile.
    let context_result = unsafe {
        libc::copyfile_state_set(
            state.0,
            libc::COPYFILE_STATE_STATUS_CTX as u32,
            (&raw mut context).cast(),
        )
    };
    if context_result != 0 {
        return Err(Error::io(
            "configure native copy context for",
            destination_path,
            io::Error::last_os_error(),
        ));
    }

    let flags = libc::COPYFILE_DATA | libc::COPYFILE_DATA_SPARSE | libc::COPYFILE_XATTR;
    // SAFETY: The files and state are valid for the duration of this synchronous call.
    let result =
        unsafe { libc::fcopyfile(source.as_raw_fd(), destination.as_raw_fd(), state.0, flags) };
    if result != 0 {
        if cancelled.load(Ordering::Relaxed)
            || io::Error::last_os_error().raw_os_error() == Some(libc::ECANCELED)
        {
            return Err(Error::Cancelled);
        }
        return Err(Error::io(
            "copy data to",
            destination_path,
            io::Error::last_os_error(),
        ));
    }

    let mut physical: libc::off_t = 0;
    // SAFETY: The state remains valid and `physical` has the documented type.
    let result = unsafe {
        libc::copyfile_state_get(
            state.0,
            libc::COPYFILE_STATE_COPIED as u32,
            (&raw mut physical).cast(),
        )
    };
    if result != 0 {
        return Err(Error::io(
            "read native copy progress for",
            destination_path,
            io::Error::last_os_error(),
        ));
    }
    let physical = physical.max(0) as u64;
    stats.current_copied.store(physical, Ordering::Relaxed);
    Ok(physical)
}

pub(super) struct CopyfileState(libc::copyfile_state_t);

impl CopyfileState {
    fn new() -> io::Result<Self> {
        // SAFETY: Allocates a new opaque copyfile state.
        let state = unsafe { libc::copyfile_state_alloc() };
        if state.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(state))
        }
    }
}

impl Drop for CopyfileState {
    fn drop(&mut self) {
        // SAFETY: This state was returned by copyfile_state_alloc and is owned here.
        unsafe {
            libc::copyfile_state_free(self.0);
        }
    }
}
