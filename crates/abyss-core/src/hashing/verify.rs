use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use quichash_core::database::{DatabaseFormat, DatabaseHandler};
use quichash_core::{Algorithm, FailurePolicy, OperationObserver, ProgressEvent, verify_folder};

use crate::Error;
use crate::hashing::create::core_error;
use crate::progress::{CopyStats, OperationPhase};

pub fn verify_database(
    database: &Path,
    root: &Path,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    stats.reset();
    stats.set_phase(OperationPhase::VerifyingHashes);
    let mut manifest = if checksum_algorithm_from_path(database).is_some() {
        DatabaseHandler::read_checksum_manifest(database).map_err(core_error)?
    } else {
        DatabaseHandler::read_manifest(database).map_err(core_error)?
    };
    let database_canonical = database.canonicalize().ok();
    manifest
        .entries
        .retain(|entry| root.join(&entry.relative_path).canonicalize().ok() != database_canonical);
    if manifest.entries.is_empty() {
        return Err(Error::message(format!(
            "hash database contains no files to verify: {}",
            database.display()
        )));
    }
    let total_bytes = manifest.entries.iter().map(|entry| entry.size).sum();
    stats.set_totals(manifest.entries.len() as u64, total_bytes);
    let observer = VerifyObserver::new(cancelled, stats);
    let report =
        verify_folder(&manifest, root, FailurePolicy::Continue, &observer).map_err(core_error)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }

    stats.objects_done.store(
        (report.matches + report.mismatches.len() + report.missing_files.len()) as u64,
        Ordering::Relaxed,
    );
    if report.mismatches.is_empty() && report.missing_files.is_empty() && report.issues.is_empty() {
        return Ok(());
    }

    let mut details = Vec::new();
    details.extend(
        report
            .mismatches
            .iter()
            .take(3)
            .map(|item| format!("changed {}", item.path.display())),
    );
    details.extend(
        report
            .missing_files
            .iter()
            .take(3)
            .map(|path| format!("missing {}", path.display())),
    );
    details.extend(report.issues.iter().take(3).map(|issue| {
        issue.path.as_ref().map_or_else(
            || issue.message.clone(),
            |path| format!("{}: {}", path.display(), issue.message),
        )
    }));
    Err(Error::message(format!(
        "{} matched, {} changed, {} missing, {} errors{}{}",
        report.matches,
        report.mismatches.len(),
        report.missing_files.len(),
        report.issues.len(),
        if details.is_empty() { "" } else { ": " },
        details.join("; ")
    )))
}

pub fn is_verification_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if checksum_algorithm_from_path(path).is_some() || has_database_extension(path) {
        return true;
    }
    DatabaseHandler::read_manifest(path).is_ok_and(|manifest| !manifest.entries.is_empty())
}

pub fn default_database_name(sources: &[PathBuf], root: &Path) -> String {
    let base = if sources.len() == 1 {
        sources[0].file_name()
    } else {
        root.file_name()
    }
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty())
    .unwrap_or("checks");
    format!("{base}.qh")
}

pub fn database_suffix(format: DatabaseFormat, compressed: bool) -> &'static str {
    match (format, compressed) {
        (DatabaseFormat::Quichash, false) => ".qh",
        (DatabaseFormat::Quichash, true) => ".qh.xz",
        (DatabaseFormat::Hashdeep, _) => ".hashdeep",
    }
}

fn has_database_extension(path: &Path) -> bool {
    let mut path = path.to_owned();
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("xz"))
    {
        path.set_extension("");
    }
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("qh") || value.eq_ignore_ascii_case("hashdeep")
        })
}

fn checksum_algorithm_from_path(path: &Path) -> Option<Algorithm> {
    let mut path = path.to_owned();
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("xz"))
    {
        path.set_extension("");
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "md5" => Some(Algorithm::Md5),
        "sha1" | "sha-1" => Some(Algorithm::Sha1),
        "sha224" | "sha-224" => Some(Algorithm::Sha224),
        "sha256" | "sha-256" => Some(Algorithm::Sha256),
        "sha384" | "sha-384" => Some(Algorithm::Sha384),
        "sha512" | "sha-512" => Some(Algorithm::Sha512),
        "sha3-224" => Some(Algorithm::Sha3_224),
        "sha3-256" => Some(Algorithm::Sha3_256),
        "sha3-384" => Some(Algorithm::Sha3_384),
        "sha3-512" => Some(Algorithm::Sha3_512),
        "blake2b" | "blake2b-512" => Some(Algorithm::Blake2b512),
        "blake2s" | "blake2s-256" => Some(Algorithm::Blake2s256),
        "blake3" => Some(Algorithm::Blake3),
        "xxh3" => Some(Algorithm::Xxh3),
        "xxh128" => Some(Algorithm::Xxh128),
        _ => None,
    }
}

struct VerifyObserver<'a> {
    cancelled: &'a AtomicBool,
    stats: &'a CopyStats,
    last_completed: Mutex<u64>,
}

impl<'a> VerifyObserver<'a> {
    fn new(cancelled: &'a AtomicBool, stats: &'a CopyStats) -> Self {
        Self {
            cancelled,
            stats,
            last_completed: Mutex::new(0),
        }
    }
}

impl OperationObserver for VerifyObserver<'_> {
    fn on_progress(&self, event: &ProgressEvent) {
        self.stats.set_phase(OperationPhase::VerifyingHashes);
        let mut previous = self
            .last_completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if event.completed > *previous {
            self.stats
                .logical_done
                .fetch_add(event.bytes_processed, Ordering::Relaxed);
            self.stats
                .physical_done
                .fetch_add(event.bytes_processed, Ordering::Relaxed);
            *previous = event.completed;
        }
        self.stats
            .objects_done
            .store(event.completed, Ordering::Relaxed);
        if let Some(path) = &event.path {
            self.stats.observe_transfer(path);
        }
    }

    fn is_cancelled(&self) -> bool {
        !self.stats.wait_for_transfer(self.cancelled, 0)
    }
}
