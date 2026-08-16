use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use quichash_core::database::DatabaseHandler;
use quichash_core::{
    FailurePolicy, HashMode, HashUtilityError, Manifest, ManifestEntry, OperationObserver,
    ProgressEvent, ProgressPhase, ScanOptions, hash_file_mode, scan_folder,
};

use crate::Error;
use crate::hashing::types::HashCreateOptions;
use crate::progress::{CopyStats, OperationPhase};

pub fn create_database(
    options: &HashCreateOptions,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<PathBuf, Error> {
    if options.sources.is_empty() {
        return Err(Error::message("nothing selected for hashing"));
    }
    let final_path = DatabaseHandler::canonical_output_path(
        &options.destination,
        options.format,
        options.compressed,
    )
    .map_err(core_error)?;
    let plain_path =
        DatabaseHandler::canonical_output_path(&options.destination, options.format, false)
            .map_err(core_error)?;
    if final_path.exists() || (options.compressed && plain_path.exists()) {
        return Err(Error::message(format!(
            "hash database already exists: {}",
            final_path.display()
        )));
    }

    stats.reset();
    stats.set_phase(OperationPhase::Scanning);
    let canonical_output = final_path.canonicalize().ok();
    let mut manifest = Manifest::default();
    let mut objects = 0_u64;
    let mut bytes = 0_u64;

    for source in &options.sources {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let metadata = source
            .symlink_metadata()
            .map_err(|error| Error::io("read hashing source metadata for", source, error))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if source.canonicalize().ok() == canonical_output {
                continue;
            }
            let size = metadata.len();
            stats.set_phase(OperationPhase::Hashing);
            stats.set_totals(objects.saturating_add(1), bytes.saturating_add(size));
            stats.begin_file(source, size);
            let digests =
                hash_file_mode(source, &[options.algorithm], HashMode::Full).map_err(core_error)?;
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            manifest.entries.push(ManifestEntry {
                relative_path: relative_to_root(source, &options.root),
                size,
                mode: HashMode::Full,
                digests,
            });
            stats.complete_file(size, size, false, false);
            objects = objects.saturating_add(1);
            bytes = bytes.saturating_add(size);
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }

        let observer = ScanObserver::new(cancelled, stats, objects, bytes);
        let report = scan_folder(
            source,
            &ScanOptions {
                algorithms: vec![options.algorithm],
                mode: HashMode::Full,
                parallel: options.parallel,
                use_hashignore: true,
                failure_policy: FailurePolicy::FailFast,
                exclude: Some(final_path.clone()),
            },
            &observer,
        )
        .map_err(core_error)?;
        let prefix = relative_to_root(source, &options.root);
        manifest
            .entries
            .extend(report.manifest.entries.into_iter().map(|mut entry| {
                entry.relative_path = prefix.join(entry.relative_path);
                entry
            }));
        objects = objects.saturating_add(report.files_processed as u64);
        bytes = bytes.saturating_add(report.total_bytes);
        stats.objects_done.store(objects, Ordering::Relaxed);
        stats.logical_done.store(bytes, Ordering::Relaxed);
        stats.physical_done.store(bytes, Ordering::Relaxed);
        stats.set_totals(objects, bytes);
    }

    if manifest.entries.is_empty() {
        return Err(Error::message("selection contains no regular files"));
    }
    manifest.canonicalize();
    stats.set_phase(OperationPhase::WritingHashes);
    DatabaseHandler::write_manifest_file(
        &options.destination,
        &manifest,
        options.format,
        options.compressed,
    )
    .map_err(core_error)
}

pub(crate) fn relative_to_root(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_owned)
        .or_else(|| path.file_name().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("item"))
}

pub(crate) fn core_error(error: HashUtilityError) -> Error {
    if matches!(error, HashUtilityError::Cancelled) {
        Error::Cancelled
    } else {
        Error::message(error.to_string())
    }
}

struct ScanObserver<'a> {
    cancelled: &'a AtomicBool,
    stats: &'a CopyStats,
    object_offset: u64,
    byte_offset: u64,
}

impl<'a> ScanObserver<'a> {
    fn new(
        cancelled: &'a AtomicBool,
        stats: &'a CopyStats,
        object_offset: u64,
        byte_offset: u64,
    ) -> Self {
        Self {
            cancelled,
            stats,
            object_offset,
            byte_offset,
        }
    }
}

impl OperationObserver for ScanObserver<'_> {
    fn on_progress(&self, event: &ProgressEvent) {
        match event.phase {
            ProgressPhase::Discovering => {
                self.stats.set_phase(OperationPhase::Scanning);
                self.stats.scanned_objects.store(
                    self.object_offset.saturating_add(event.completed),
                    Ordering::Relaxed,
                );
                if let Some(path) = &event.path {
                    self.stats.observe_scan(path);
                }
            }
            ProgressPhase::Hashing => {
                self.stats.set_phase(OperationPhase::Hashing);
                self.stats.objects_done.store(
                    self.object_offset.saturating_add(event.completed),
                    Ordering::Relaxed,
                );
                let bytes = self.byte_offset.saturating_add(event.bytes_processed);
                self.stats.logical_done.store(bytes, Ordering::Relaxed);
                self.stats.physical_done.store(bytes, Ordering::Relaxed);
                if let Some(total) = event.total {
                    self.stats
                        .total_objects
                        .store(self.object_offset.saturating_add(total), Ordering::Relaxed);
                }
                if let Some(path) = &event.path {
                    self.stats.observe_transfer(path);
                }
            }
            ProgressPhase::Writing | ProgressPhase::Verifying => {}
        }
    }

    fn is_cancelled(&self) -> bool {
        !self.stats.wait_for_transfer(self.cancelled, 0)
    }
}
