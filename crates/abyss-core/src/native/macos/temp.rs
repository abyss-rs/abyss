use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Error;

pub(super) static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn temporary_path(destination: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    destination.with_file_name(format!(".abyss.{}.{}.tmp", std::process::id(), sequence))
}

pub(super) fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
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
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn replace(mut temporary: Temporary, destination: &Path) -> Result<(), Error> {
    fs::rename(&temporary.path, destination)
        .map_err(|error| Error::io("replace", destination, error))?;
    temporary.committed = true;
    Ok(())
}

pub(super) fn error_to_io(error: Error) -> io::Error {
    match error {
        Error::Io { source, .. } => source,
        Error::Cancelled => io::Error::from_raw_os_error(libc::ECANCELED),
        Error::Message(message) => io::Error::other(message),
    }
}
