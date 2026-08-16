mod create;
mod detect;
mod read;

use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;

use crate::archive::formats::sevenz::{LZMA2_CHUNK_SIZE, lzma2_workers};
use crate::archive::reader::normalize_member_path;
use crate::archive::types::{
    ArchiveContainer, ArchiveCreateOptions, ArchiveIndex, CompressionMethod, CompressionThreads,
};
use crate::archive::writer::{CreateEntry, create_archive, progress_reader};
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn rejects_archive_path_traversal() {
    assert_eq!(
        normalize_member_path("safe/f.txt"),
        Some("safe/f.txt".to_owned())
    );
    assert_eq!(
        normalize_member_path("safe\\f.txt"),
        Some("safe/f.txt".to_owned())
    );
    assert_eq!(normalize_member_path("../outside"), None);
    assert_eq!(normalize_member_path("/absolute"), None);
}

#[test]
fn lzma2_workers_scale_only_for_large_inputs() {
    assert_eq!(lzma2_workers(CompressionThreads::Count(8), 0), 1);
    assert_eq!(
        lzma2_workers(CompressionThreads::Count(8), LZMA2_CHUNK_SIZE * 2 - 1),
        1
    );
    let available = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1) as u32;
    assert_eq!(
        lzma2_workers(CompressionThreads::Count(8), LZMA2_CHUNK_SIZE * 2),
        available.min(2)
    );
    assert_eq!(
        lzma2_workers(CompressionThreads::Count(1), LZMA2_CHUNK_SIZE * 32),
        1
    );
    assert!(lzma2_workers(CompressionThreads::Auto, LZMA2_CHUNK_SIZE * 32) <= 4);
}

#[test]
fn archive_reader_reports_codec_finalization_after_eof() {
    let temp = TempDir::new();
    let source = temp.path().join("input.bin");
    std::fs::write(&source, b"finalize me").unwrap();
    let entry = CreateEntry {
        source,
        name: "input.bin".to_owned(),
        size: 11,
        is_directory: false,
    };
    let stats = CopyStats::default();
    stats.set_totals(1, entry.size);
    let cancelled = AtomicBool::new(false);
    let mut reader = progress_reader(&entry, &cancelled, &stats);
    let mut data = Vec::new();
    reader.read_to_end(&mut data).unwrap();

    let snapshot = stats.snapshot();
    assert_eq!(data, b"finalize me");
    assert_eq!(snapshot.phase, crate::progress::OperationPhase::Finalizing);
    assert_eq!(snapshot.logical_done, entry.size);
    assert_eq!(snapshot.objects_done, 1);
}

#[test]
#[ignore = "manual archive throughput probe"]
fn mixed_archive_throughput_probe() {
    let temp = TempDir::new();
    let source = temp.path().join("mixed");
    std::fs::create_dir(&source).unwrap();
    for index in 0..64 {
        std::fs::write(
            source.join(format!("small-{index:03}.bin")),
            vec![index as u8; 16 * 1024],
        )
        .unwrap();
    }
    let mut large = File::create(source.join("large.bin")).unwrap();
    let mut block = vec![0_u8; 1024 * 1024];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..32 {
        for bytes in block.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.copy_from_slice(&state.to_le_bytes());
        }
        large.write_all(&block).unwrap();
    }
    drop(large);

    for (name, filename, container, method, level) in [
        (
            "zstd",
            "probe.tar.zst",
            ArchiveContainer::Auto,
            CompressionMethod::Zstd,
            3,
        ),
        (
            "zip",
            "probe.zip",
            ArchiveContainer::Zip,
            CompressionMethod::Deflate,
            6,
        ),
        (
            "7z",
            "probe.7z",
            ArchiveContainer::SevenZip,
            CompressionMethod::Lzma2,
            5,
        ),
    ] {
        let destination = temp.path().join(filename);
        let mut options =
            ArchiveCreateOptions::zstd_default(vec![source.clone()], destination.clone(), 1 << 20);
        options.container = container;
        options.method = method;
        options.level = level;
        let started = std::time::Instant::now();
        create_archive(&options, &AtomicBool::new(false), &CopyStats::default()).unwrap();
        let elapsed = started.elapsed();
        let output_size = destination.metadata().unwrap().len();
        eprintln!(
            "{name}: {:.2}s, {:.1} MiB/s, {} bytes",
            elapsed.as_secs_f64(),
            33.0 / elapsed.as_secs_f64(),
            output_size
        );
        ArchiveIndex::open(&destination, None).unwrap();
    }
}
