mod app;
mod console;
mod highlight;
mod monitor;
mod ui;

#[cfg(feature = "remote")]
pub use abyss_core::remote_operation;
pub use abyss_core::{
    Error, appledouble, archive, browser, copy, diff, error, frontend, hashing, inspect, inventory,
    jobs, native, operation, progress, search, storage, sync, tasks, viewer, workspace,
};

pub fn run(left: Option<String>, right: Option<String>) -> Result<(), Error> {
    app::run(left, right)
}

#[cfg(test)]
mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "abyss-tui-test-{}-{}",
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
