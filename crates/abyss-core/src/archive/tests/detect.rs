use crate::archive::reader::{detect, looks_like_archive};
use crate::archive::types::ArchiveFormat;
use crate::test_support::TempDir;
use unarc_rs::unified::{ArchiveFormat as UnifiedFormat, supported_extensions};

#[test]
fn looks_like_archive_covers_every_unarc_extension() {
    let temp = TempDir::new();
    for extension in supported_extensions() {
        let path = temp.path().join(format!("sample.{extension}"));
        std::fs::write(&path, []).unwrap();
        assert!(
            looks_like_archive(&path),
            "expected looks_like_archive for .{extension}"
        );
    }
}

#[test]
fn looks_like_archive_covers_abyss_extra_codecs() {
    let temp = TempDir::new();
    for name in [
        "a.xz",
        "a.lz",
        "a.lzip",
        "a.zst",
        "a.zstd",
        "a.lz4",
        "a.br",
        "a.tar.xz",
        "a.txz",
        "a.tar.lz",
        "a.tar.lzip",
        "a.tar.zst",
        "a.tar.zstd",
        "a.tar.lz4",
        "a.tar.br",
    ] {
        let path = temp.path().join(name);
        std::fs::write(&path, []).unwrap();
        assert!(
            looks_like_archive(&path),
            "expected looks_like_archive for {name}"
        );
    }
}

#[test]
fn detect_maps_unarc_extensions_to_unified_or_rar() {
    let temp = TempDir::new();
    for format in UnifiedFormat::ALL {
        // Prefer a stable primary extension; skip patterns that need real content.
        let extension = format.extension();
        let path = temp.path().join(format!("sample.{extension}"));
        std::fs::write(&path, []).unwrap();
        let detected = detect(&path).expect("detect by extension");
        match (format, detected) {
            (UnifiedFormat::Rar, ArchiveFormat::Rar) => {}
            (expected, ArchiveFormat::Unified(actual)) => {
                assert_eq!(actual, *expected, "extension .{extension}");
            }
            (expected, other) => {
                panic!(".{extension}: expected Unified({expected:?}) or Rar, got {other:?}")
            }
        }
    }
}

#[test]
fn detect_maps_abyss_extra_extensions() {
    let temp = TempDir::new();
    let cases = [
        ("a.xz", ArchiveFormat::Xz),
        ("a.zst", ArchiveFormat::Zstd),
        ("a.lz4", ArchiveFormat::Lz4),
        ("a.lzip", ArchiveFormat::Lzip),
        ("a.br", ArchiveFormat::Brotli),
        ("a.tar.xz", ArchiveFormat::TarXz),
        ("a.tar.zst", ArchiveFormat::TarZstd),
        ("a.tar.lz4", ArchiveFormat::TarLz4),
        ("a.tar.lzip", ArchiveFormat::TarLzip),
        ("a.tar.br", ArchiveFormat::TarBrotli),
    ];
    for (name, expected) in cases {
        let path = temp.path().join(name);
        std::fs::write(&path, []).unwrap();
        assert_eq!(detect(&path).unwrap(), expected, "{name}");
    }
}
