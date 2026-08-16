mod entries;
mod stream;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use self::entries::{CreateEntry, collect_create_entries, ensure_create_space};
pub(crate) use self::stream::{
    CompressedSizeTracker, copy_with_progress, create_zstd_encoder, progress_reader,
    write_archive_payload,
};
use crate::Error;
use crate::archive::formats::tar::append_tar_zstd_toc;
use crate::archive::types::{
    ArchiveContainer, ArchiveCreateOptions, ArchiveMember, CompressionMethod,
};
use crate::progress::{CopyStats, OperationPhase};
pub(crate) const CREATE_IO_BUFFER: usize = 1024 * 1024;
pub(crate) const CREATE_WRITE_BUFFER_CAP: usize = 8 * 1024 * 1024;

/// Creates an archive using the selected container and codec. Auto mode preserves
/// the original defaults: one file becomes `.zst`; a directory or selection is `.tar.zst`.
pub fn create_archive(
    options: &ArchiveCreateOptions,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<Vec<PathBuf>, Error> {
    let _selected_preset = options.preset;
    if options.sources.is_empty() {
        return Err(Error::message("nothing selected for archive creation"));
    }
    let pack_tar = match options.container {
        ArchiveContainer::Auto => should_pack_as_tar(&options.sources),
        ArchiveContainer::Tar => true,
        ArchiveContainer::SevenZip | ArchiveContainer::Zip => false,
    };
    validate_create_destination(options, pack_tar)?;

    let parent = options
        .destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = std::fs::metadata(parent)
        .map_err(|error| Error::io("inspect archive destination", parent, error))?;
    if !metadata.is_dir() {
        return Err(Error::message(format!(
            "archive destination is not a directory: {}",
            parent.display()
        )));
    }
    if options.destination.exists() {
        return Err(Error::message(format!(
            "archive already exists: {}",
            options.destination.display()
        )));
    }

    stats.set_phase(OperationPhase::Scanning);
    let entries = collect_create_entries(&options.sources, &options.destination, cancelled, stats)?;
    let total_bytes = entries.iter().map(|entry| entry.size).sum::<u64>();
    stats.set_totals(entries.len() as u64, total_bytes);
    ensure_create_space(parent, total_bytes)?;
    stats.set_phase(OperationPhase::Compressing);

    let write_capacity = options
        .buffer_capacity
        .clamp(CREATE_IO_BUFFER, CREATE_WRITE_BUFFER_CAP);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| Error::io("create archive staging file in", parent, error))?;
    {
        // Tracker wraps the BufWriter so every encoder write updates compressed size
        // immediately (not only when the buffer flushes to disk).
        let sink = CompressedSizeTracker {
            inner: std::io::BufWriter::with_capacity(write_capacity, temporary.as_file_mut()),
            stats,
            written: 0,
        };
        write_archive_payload(sink, options, &entries, pack_tar, cancelled, stats)?;
    }
    if pack_tar && options.method == CompressionMethod::Zstd {
        let members = entries
            .iter()
            .map(|entry| ArchiveMember {
                path: entry.name.clone(),
                size: entry.size,
                is_directory: entry.is_directory,
            })
            .collect::<Vec<_>>();
        append_tar_zstd_toc(temporary.as_file_mut(), &members)
            .map_err(|error| Error::io("append archive TOC to", temporary.path(), error))?;
    }
    if let Ok(compressed) = temporary.as_file().metadata().map(|meta| meta.len()) {
        stats.set_wire(compressed);
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    stats.begin_file(Path::new("Publishing archive to destination..."), 0);
    temporary
        .persist_noclobber(&options.destination)
        .map_err(|error| Error::io("publish archive", &options.destination, error.error))?;
    Ok(vec![options.destination.clone()])
}

pub(crate) fn should_pack_as_tar(sources: &[PathBuf]) -> bool {
    sources.len() != 1
        || sources[0]
            .symlink_metadata()
            .map(|meta| meta.is_dir())
            .unwrap_or(true)
}

pub(crate) fn validate_create_destination(
    options: &ArchiveCreateOptions,
    pack_tar: bool,
) -> Result<(), Error> {
    let name = options
        .destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let suffix = create_suffix(options.container, options.method, pack_tar);
    if name.to_ascii_lowercase().ends_with(suffix) {
        return Ok(());
    }
    Err(Error::message(format!(
        "archive destination must end in {suffix}"
    )))
}

pub fn create_suffix(
    container: ArchiveContainer,
    method: CompressionMethod,
    pack_tar: bool,
) -> &'static str {
    match container {
        ArchiveContainer::SevenZip => ".7z",
        ArchiveContainer::Zip => ".zip",
        ArchiveContainer::Auto | ArchiveContainer::Tar => match (pack_tar, method) {
            (true, CompressionMethod::Store) => ".tar",
            (true, CompressionMethod::Zstd) => ".tar.zst",
            (true, CompressionMethod::Gzip) => ".tar.gz",
            (true, CompressionMethod::Xz) => ".tar.xz",
            (true, CompressionMethod::Bzip2) => ".tar.bz2",
            (true, CompressionMethod::Lz4) => ".tar.lz4",
            (true, CompressionMethod::Brotli) => ".tar.br",
            (false, CompressionMethod::Zstd) => ".zst",
            (false, CompressionMethod::Gzip) => ".gz",
            (false, CompressionMethod::Xz) => ".xz",
            (false, CompressionMethod::Bzip2) => ".bz2",
            (false, CompressionMethod::Lz4) => ".lz4",
            (false, CompressionMethod::Brotli) => ".br",
            _ => ".tar",
        },
    }
}
