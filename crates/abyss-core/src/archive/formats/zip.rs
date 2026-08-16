use std::io::{Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use zip::write::SimpleFileOptions;

use crate::Error;
use crate::archive::types::{ArchiveCreateOptions, CompressionMethod};
use crate::archive::writer::{CreateEntry, copy_with_progress, progress_reader};
use crate::progress::CopyStats;

pub(crate) fn write_zip_archive<W: Write + Seek>(
    sink: W,
    options: &ArchiveCreateOptions,
    entries: &[CreateEntry],
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    let method = match options.method {
        CompressionMethod::Store => zip::CompressionMethod::Stored,
        CompressionMethod::Deflate => zip::CompressionMethod::Deflated,
        CompressionMethod::Bzip2 => zip::CompressionMethod::Bzip2,
        CompressionMethod::Zstd => zip::CompressionMethod::Zstd,
        CompressionMethod::Xz => zip::CompressionMethod::Xz,
        _ => return Err(Error::message("method is not valid for a ZIP archive")),
    };
    let level = match options.method {
        CompressionMethod::Deflate | CompressionMethod::Bzip2 | CompressionMethod::Zstd => {
            Some(i64::from(options.level))
        }
        _ => None,
    };
    let mut archive = zip::ZipWriter::new(sink);
    for entry in entries {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut file_options = SimpleFileOptions::default()
            .compression_method(method)
            .compression_level(level);
        if let Some(password) = options.password.as_deref() {
            file_options = file_options.with_aes_encryption(zip::AesMode::Aes256, password);
        }
        if entry.is_directory {
            let name = format!("{}/", entry.name.trim_end_matches('/'));
            archive
                .add_directory(name, file_options)
                .map_err(|error| Error::message(format!("archive {}: {error}", entry.name)))?;
            stats.complete_object(&entry.source);
        } else {
            archive
                .start_file(&entry.name, file_options)
                .map_err(|error| Error::message(format!("archive {}: {error}", entry.name)))?;
            let mut reader = progress_reader(entry, cancelled, stats);
            copy_with_progress(&mut reader, &mut archive, &entry.name, cancelled)?;
        }
    }
    archive
        .finish()
        .map_err(|error| Error::message(format!("finalize ZIP archive: {error}")))?;
    Ok(())
}
