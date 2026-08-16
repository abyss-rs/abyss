use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use lz4_flex::frame::FrameEncoder;
use lzma_rust2::{XzOptions, XzWriter};

use super::CREATE_IO_BUFFER;
use super::entries::CreateEntry;
use crate::Error;
use crate::archive::formats::sevenz::write_7z_archive;
use crate::archive::formats::tar::write_tar_archive;
use crate::archive::formats::zip::write_zip_archive;
use crate::archive::types::{
    ArchiveContainer, ArchiveCreateOptions, CompressionMethod, CompressionThreads,
};
use crate::progress::{CopyStats, OperationPhase};
pub(crate) fn create_zstd_encoder<W: Write>(
    sink: W,
    level: i32,
    threads: CompressionThreads,
) -> Result<zstd::stream::write::Encoder<'static, W>, Error> {
    let mut encoder = zstd::stream::write::Encoder::new(sink, level)
        .map_err(|error| Error::message(format!("initialize zstd encoder: {error}")))?;
    // libzstd MT: one job thread per worker; keep a core free for I/O/UI.
    let available = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let workers = match threads {
        CompressionThreads::Auto => available.saturating_sub(1).clamp(1, 8),
        CompressionThreads::Count(count) => usize::from(count).clamp(1, available),
    } as u32;
    if workers > 1 {
        encoder
            .multithread(workers)
            .map_err(|error| Error::message(format!("enable zstd multithreading: {error}")))?;
    }
    Ok(encoder)
}

/// Counts compressed bytes as they leave the encoder (before disk buffering).
pub(crate) struct CompressedSizeTracker<'a, W> {
    pub(crate) inner: W,
    pub(crate) stats: &'a CopyStats,
    pub(crate) written: u64,
}

impl<W: Write> Write for CompressedSizeTracker<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        if written > 0 {
            self.written = self.written.saturating_add(written as u64);
            self.stats.set_wire(self.written);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write + Seek> Seek for CompressedSizeTracker<'_, W> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

pub(crate) fn copy_with_progress(
    reader: &mut ArchiveProgressReader<'_>,
    writer: &mut impl Write,
    name: &str,
    cancelled: &AtomicBool,
) -> Result<(), Error> {
    let mut buffer = vec![0_u8; CREATE_IO_BUFFER];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_error) if cancelled.load(Ordering::Relaxed) => return Err(Error::Cancelled),
            Err(error) => {
                return Err(Error::message(format!("read {name}: {error}")));
            }
        };
        if let Err(error) = writer.write_all(&buffer[..read]) {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            return Err(Error::message(format!("compress {name}: {error}")));
        }
    }
    Ok(())
}

pub(crate) fn write_archive_payload<W: Write + Seek>(
    sink: W,
    options: &ArchiveCreateOptions,
    entries: &[CreateEntry],
    pack_tar: bool,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    match options.container {
        ArchiveContainer::SevenZip => write_7z_archive(sink, options, entries, cancelled, stats),
        ArchiveContainer::Zip => write_zip_archive(sink, options, entries, cancelled, stats),
        ArchiveContainer::Auto | ArchiveContainer::Tar if pack_tar => {
            write_tar_archive(sink, entries, options, cancelled, stats)
        }
        ArchiveContainer::Auto => {
            let Some(file) = entries.iter().find(|entry| !entry.is_directory) else {
                return Err(Error::message("nothing to compress"));
            };
            write_single_stream(sink, file, options, cancelled, stats)
        }
        ArchiveContainer::Tar => unreachable!("tar container always packs a tar stream"),
    }
}

pub(crate) fn progress_reader<'a>(
    entry: &CreateEntry,
    cancelled: &'a AtomicBool,
    stats: &'a CopyStats,
) -> ArchiveProgressReader<'a> {
    ArchiveProgressReader {
        path: entry.source.clone(),
        cancelled,
        stats,
        buffer_capacity: CREATE_IO_BUFFER,
        size: entry.size,
        finished: false,
        inner: None,
    }
}

pub(crate) fn write_single_stream(
    sink: impl Write,
    entry: &CreateEntry,
    options: &ArchiveCreateOptions,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    let mut reader = progress_reader(entry, cancelled, stats);
    match options.method {
        CompressionMethod::Zstd => {
            let mut encoder = create_zstd_encoder(sink, i32::from(options.level), options.threads)?;
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .finish()
                .map_err(|error| Error::io("finalize zstd stream", &entry.source, error))?;
        }
        CompressionMethod::Gzip => {
            let mut encoder = flate2::write::GzEncoder::new(
                sink,
                flate2::Compression::new(u32::from(options.level.min(9))),
            );
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .finish()
                .map_err(|error| Error::io("finalize gzip stream", &entry.source, error))?;
        }
        CompressionMethod::Bzip2 => {
            let mut encoder = bzip2::write::BzEncoder::new(
                sink,
                bzip2::Compression::new(u32::from(options.level.clamp(1, 9))),
            );
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .finish()
                .map_err(|error| Error::io("finalize bzip2 stream", &entry.source, error))?;
        }
        CompressionMethod::Xz => {
            let mut encoder = XzWriter::new(sink, XzOptions::with_preset(u32::from(options.level)))
                .map_err(|error| Error::message(format!("initialize xz encoder: {error}")))?;
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .finish()
                .map_err(|error| Error::message(format!("finalize xz stream: {error}")))?;
        }
        CompressionMethod::Lz4 => {
            let mut encoder = FrameEncoder::new(sink);
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .finish()
                .map_err(|error| Error::message(format!("finalize lz4 stream: {error}")))?;
        }
        CompressionMethod::Brotli => {
            let mut encoder = brotli::CompressorWriter::new(
                sink,
                CREATE_IO_BUFFER,
                u32::from(options.level.min(11)),
                22,
            );
            copy_with_progress(&mut reader, &mut encoder, &entry.name, cancelled)?;
            encoder
                .flush()
                .map_err(|error| Error::message(format!("finalize brotli stream: {error}")))?;
        }
        _ => {
            return Err(Error::message(
                "method is not valid for a compression stream",
            ));
        }
    }
    Ok(())
}
pub(crate) struct ArchiveProgressReader<'a> {
    pub(crate) path: PathBuf,
    pub(crate) cancelled: &'a AtomicBool,
    pub(crate) stats: &'a CopyStats,
    pub(crate) buffer_capacity: usize,
    pub(crate) size: u64,
    pub(crate) finished: bool,
    pub(crate) inner: Option<std::io::BufReader<File>>,
}

impl Read for ArchiveProgressReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.finished {
            return Ok(0);
        }
        let reader = match &mut self.inner {
            Some(r) => r,
            None => {
                self.stats.set_phase(OperationPhase::Compressing);
                self.stats.begin_file(&self.path, self.size);
                let file = File::open(&self.path).map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!("open archive input {}: {error}", self.path.display()),
                    )
                })?;
                self.inner = Some(std::io::BufReader::with_capacity(
                    self.buffer_capacity,
                    file,
                ));
                self.inner.as_mut().unwrap()
            }
        };
        let read = reader.read(buffer)?;
        if read > 0 {
            if !self.stats.wait_for_transfer(self.cancelled, read as u64) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            self.stats
                .current_copied
                .fetch_add(read as u64, Ordering::Relaxed);
        } else {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            self.stats.set_phase(OperationPhase::Finalizing);
            self.finished = true;
            // Compressed size is tracked by CompressedSizeTracker on the encoder sink.
            self.stats.complete_file(self.size, 0, false, false);
            self.inner = None;
        }
        Ok(read)
    }
}
