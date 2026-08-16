#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
compile_error!("abyss supports macOS, Linux, and Windows only");

pub mod appledouble;
pub mod archive;
pub mod browser;
pub mod copy;
pub mod diff;
pub mod error;
pub mod frontend;
pub mod hashing;
pub mod inspect;
pub mod inventory;
pub mod jobs;
pub mod native;
pub mod operation;
pub mod progress;
#[cfg(feature = "tokio")]
pub mod remote_operation;
pub mod search;
pub mod storage;
pub mod sync;
pub mod tasks;
pub mod viewer;
pub mod workspace;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub use error::Error;

pub fn copy(source: &Path, destination: &Path) -> Result<(), Error> {
    let cancelled = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled))
        .map_err(|error| Error::io("install signal handler for", source, error))?;
    copy::run(source, destination, cancelled)
}

#[cfg(test)]
pub mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir(PathBuf);

    impl Default for TempDir {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TempDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "abyss-test-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
