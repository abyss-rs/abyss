use std::sync::atomic::AtomicBool;

use crate::archive::formats::tar::read_tar_zstd_toc;
use crate::archive::reader::extract_member;
use crate::archive::types::{
    ArchiveContainer, ArchiveCreateOptions, ArchiveIndex, ArchiveOpenError, CompressionMethod,
};
use crate::archive::writer::create_archive;
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn creates_folder_as_tar_zst() {
    let temp = TempDir::new();
    let source = temp.path().join("folder");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("note.txt"), b"archive preset").unwrap();
    let destination = temp.path().join("folder.tar.zst");
    let outputs = create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 128 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();
    assert_eq!(outputs, vec![destination.clone()]);
    let index = ArchiveIndex::open(&destination, None).unwrap();
    assert!(index.member("folder/note.txt").is_some());
}

#[test]
fn creates_single_file_as_standalone_zst() {
    let temp = TempDir::new();
    let source = temp.path().join("note.txt");
    std::fs::write(&source, b"hello zstd").unwrap();
    let destination = temp.path().join("note.txt.zst");
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 128 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();
    let index = ArchiveIndex::open(&destination, None).unwrap();
    assert_eq!(index.members[0].path, "note.txt");
    let mut output = Vec::new();
    extract_member(&index, "note.txt", None, &mut output).unwrap();
    assert_eq!(output, b"hello zstd");
}

#[test]
fn creates_and_reopens_supported_container_families() {
    let cases = [
        (
            ArchiveContainer::Tar,
            CompressionMethod::Gzip,
            "sample.tar.gz",
        ),
        (
            ArchiveContainer::Zip,
            CompressionMethod::Deflate,
            "sample.zip",
        ),
        (
            ArchiveContainer::SevenZip,
            CompressionMethod::Lzma2,
            "sample.7z",
        ),
    ];
    for (container, method, name) in cases {
        let temp = TempDir::new();
        let source = temp.path().join("folder");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("note.txt"), b"multi format payload").unwrap();
        let destination = temp.path().join(name);
        let mut options =
            ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20);
        options.container = container;
        options.method = method;
        options.level = 3;
        create_archive(&options, &AtomicBool::new(false), &CopyStats::default()).unwrap();

        let index = ArchiveIndex::open(&destination, None).unwrap();
        assert!(index.member("folder/note.txt").is_some(), "{name}");
        let mut output = Vec::new();
        extract_member(&index, "folder/note.txt", None, &mut output).unwrap();
        assert_eq!(output, b"multi format payload", "{name}");
    }
}

#[test]
fn creates_encrypted_7z_and_zip_archives() {
    for (container, method, name) in [
        (
            ArchiveContainer::SevenZip,
            CompressionMethod::Lzma2,
            "secret.7z",
        ),
        (
            ArchiveContainer::Zip,
            CompressionMethod::Deflate,
            "secret.zip",
        ),
    ] {
        let temp = TempDir::new();
        let source = temp.path().join("secret.txt");
        std::fs::write(&source, b"classified").unwrap();
        let destination = temp.path().join(name);
        let mut options =
            ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20);
        options.container = container;
        options.method = method;
        options.password = Some(zeroize::Zeroizing::new("correct horse".to_owned()));
        create_archive(&options, &AtomicBool::new(false), &CopyStats::default()).unwrap();

        if container == ArchiveContainer::Zip {
            let bytes = std::fs::read(&destination).unwrap();
            assert!(
                bytes
                    .windows(b"secret.txt".len())
                    .any(|value| value == b"secret.txt")
            );
        }

        assert!(matches!(
            ArchiveIndex::open(&destination, None),
            Err(ArchiveOpenError::PasswordRequired(_))
        ));
        let index = ArchiveIndex::open(&destination, Some("correct horse")).unwrap();
        let mut output = Vec::new();
        extract_member(&index, "secret.txt", Some("correct horse"), &mut output).unwrap();
        assert_eq!(output, b"classified", "{name}");
    }
}

#[test]
fn encrypted_7z_header_hides_member_names() {
    let temp = TempDir::new();
    let source = temp.path().join("private-filename-938475.txt");
    std::fs::write(&source, b"private data").unwrap();
    let destination = temp.path().join("private.7z");
    let mut options =
        ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20);
    options.container = ArchiveContainer::SevenZip;
    options.method = CompressionMethod::Lzma2;
    options.password = Some(zeroize::Zeroizing::new("header secret".to_owned()));
    create_archive(&options, &AtomicBool::new(false), &CopyStats::default()).unwrap();

    assert!(matches!(
        sevenz_rust2::Archive::open(&destination),
        Err(sevenz_rust2::Error::PasswordRequired)
            | Err(sevenz_rust2::Error::MaybeBadPassword(_))
            | Err(sevenz_rust2::Error::ChecksumVerificationFailed)
    ));
    let password = sevenz_rust2::Password::from("header secret");
    let archive = sevenz_rust2::Archive::open_with_password(&destination, &password).unwrap();
    assert!(
        archive
            .files
            .iter()
            .any(|entry| entry.name == "private-filename-938475.txt")
    );

    let bytes = std::fs::read(destination).unwrap();
    let utf16_name = "private-filename-938475.txt"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(
        !bytes
            .windows(utf16_name.len())
            .any(|window| window == utf16_name)
    );
}

#[test]
fn every_dialog_method_round_trips() {
    let families: &[(ArchiveContainer, &[CompressionMethod], &str)] = &[
        (
            ArchiveContainer::Tar,
            &[
                CompressionMethod::Store,
                CompressionMethod::Zstd,
                CompressionMethod::Gzip,
                CompressionMethod::Xz,
                CompressionMethod::Bzip2,
                CompressionMethod::Lz4,
                CompressionMethod::Brotli,
            ],
            "tar",
        ),
        (
            ArchiveContainer::Zip,
            &[
                CompressionMethod::Store,
                CompressionMethod::Deflate,
                CompressionMethod::Bzip2,
                CompressionMethod::Zstd,
                CompressionMethod::Xz,
            ],
            "zip",
        ),
        (
            ArchiveContainer::SevenZip,
            &[
                CompressionMethod::Store,
                CompressionMethod::Lzma2,
                CompressionMethod::Lzma,
                CompressionMethod::Ppmd,
                CompressionMethod::Bzip2,
            ],
            "7z",
        ),
    ];
    for (container, methods, family) in families {
        for method in *methods {
            let temp = TempDir::new();
            let source = temp.path().join("folder");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("note.txt"), b"codec matrix").unwrap();
            let suffix = crate::archive::writer::create_suffix(*container, *method, true);
            let destination = temp.path().join(format!("matrix{suffix}"));
            let mut options =
                ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20);
            options.container = *container;
            options.method = *method;
            options.level = 3;
            options.solid = *container == ArchiveContainer::SevenZip;
            create_archive(&options, &AtomicBool::new(false), &CopyStats::default())
                .unwrap_or_else(|error| panic!("{family}/{method:?}: {error}"));
            let index = ArchiveIndex::open(&destination, None)
                .unwrap_or_else(|error| panic!("open {family}/{method:?}: {error}"));
            let mut output = Vec::new();
            extract_member(&index, "folder/note.txt", None, &mut output)
                .unwrap_or_else(|error| panic!("extract {family}/{method:?}: {error}"));
            assert_eq!(output, b"codec matrix", "{family}/{method:?}");
        }
    }
}

#[test]
fn creates_tar_zst_with_gnu_long_member_paths() {
    let temp = TempDir::new();
    let source = temp.path().join("folder");
    let mut nested = source.clone();
    // Build a relative member path well past the 100-byte ustar name limit.
    for index in 0..8 {
        nested = nested.join(format!(
            "segment-with-a-very-long-directory-name-{index:02}"
        ));
    }
    std::fs::create_dir_all(&nested).unwrap();
    let file_name = "file-with-an-extremely-long-name-that-also-pushes-the-limit.txt";
    std::fs::write(nested.join(file_name), b"long path payload").unwrap();
    let destination = temp.path().join("folder.tar.zst");
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();
    let index = ArchiveIndex::open(&destination, None).unwrap();
    let member = index
        .members
        .iter()
        .find(|member| member.path.ends_with(file_name))
        .expect("long-named member present");
    assert!(member.path.len() > 100, "path len {}", member.path.len());
    let mut output = Vec::new();
    extract_member(&index, &member.path, None, &mut output).unwrap();
    assert_eq!(output, b"long path payload");
}

#[test]
fn embeds_skippable_toc_in_created_tar_zst() {
    let temp = TempDir::new();
    let source = temp.path().join("folder");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("note.txt"), b"toc payload").unwrap();
    let destination = temp.path().join("folder.tar.zst");
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination.clone(), 1 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();

    let toc = read_tar_zstd_toc(&destination).expect("embedded TOC present");
    assert!(
        toc.iter().any(|member| member.path == "folder/note.txt"),
        "{toc:?}"
    );

    let index = ArchiveIndex::open(&destination, None).unwrap();
    assert!(index.member("folder/note.txt").is_some());
    let mut output = Vec::new();
    extract_member(&index, "folder/note.txt", None, &mut output).unwrap();
    assert_eq!(output, b"toc payload");
}

#[test]
fn standard_zstd_and_tar_can_read_abyss_tar_zst_with_toc() {
    let zstd = std::process::Command::new("zstd").arg("--version").output();
    if !zstd.map(|output| output.status.success()).unwrap_or(false) {
        eprintln!("skipping: zstd CLI not available");
        return;
    }

    let temp = TempDir::new();
    let source = temp.path().join("folder");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("note.txt"), b"external tools ok").unwrap();
    let archive = temp.path().join("folder.tar.zst");
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], archive.clone(), 1 << 20),
        &AtomicBool::new(false),
        &CopyStats::default(),
    )
    .unwrap();
    assert!(read_tar_zstd_toc(&archive).is_some());

    let decoded_tar = temp.path().join("decoded.tar");
    let status = std::process::Command::new("zstd")
        .args(["-d", "-f", "-o"])
        .arg(&decoded_tar)
        .arg(&archive)
        .status()
        .expect("run zstd -d");
    assert!(status.success(), "zstd -d failed: {status}");

    let list = std::process::Command::new("tar")
        .args(["tf"])
        .arg(&decoded_tar)
        .output()
        .expect("run tar tf");
    assert!(list.status.success(), "tar tf failed");
    let listing = String::from_utf8_lossy(&list.stdout);
    assert!(
        listing.contains("folder/note.txt"),
        "unexpected tar listing: {listing}"
    );

    let extract_dir = temp.path().join("out");
    std::fs::create_dir(&extract_dir).unwrap();
    let status = std::process::Command::new("tar")
        .args(["xf"])
        .arg(&decoded_tar)
        .args(["-C"])
        .arg(&extract_dir)
        .status()
        .expect("run tar xf");
    assert!(status.success(), "tar xf failed: {status}");
    assert_eq!(
        std::fs::read(extract_dir.join("folder/note.txt")).unwrap(),
        b"external tools ok"
    );

    let mut piped = std::process::Command::new("zstd")
        .args(["-dc"])
        .arg(&archive)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn zstd -dc");
    let list = std::process::Command::new("tar")
        .args(["tf", "-"])
        .stdin(piped.stdout.take().expect("zstd stdout"))
        .output()
        .expect("run tar tf -");
    let _ = piped.wait();
    assert!(list.status.success(), "zstd|tar tf failed");
    let listing = String::from_utf8_lossy(&list.stdout);
    assert!(
        listing.contains("folder/note.txt"),
        "unexpected piped tar listing: {listing}"
    );
}

#[test]
fn compression_progress_tracks_original_and_compressed_bytes() {
    let temp = TempDir::new();
    let source = temp.path().join("blob.bin");
    let payload = vec![b'a'; 1 << 20];
    std::fs::write(&source, &payload).unwrap();
    let destination = temp.path().join("blob.bin.zst");
    let stats = CopyStats::default();
    create_archive(
        &ArchiveCreateOptions::zstd_default(vec![source], destination, 128 << 20),
        &AtomicBool::new(false),
        &stats,
    )
    .unwrap();
    let snapshot = stats.snapshot();
    assert!(snapshot.logical_done >= payload.len() as u64);
    assert!(
        snapshot.physical_done > 0,
        "wire={}",
        snapshot.physical_done
    );
    assert!(
        snapshot.physical_done < snapshot.logical_done,
        "compressed={} original={}",
        snapshot.physical_done,
        snapshot.logical_done
    );
}
