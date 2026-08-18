use std::fs::File;
use std::io::Write;

use crate::archive::reader::extract_member;
use crate::archive::types::{ArchiveIndex, ArchiveOpenError};
use crate::test_support::TempDir;
use flate2::Compression;
use flate2::write::GzEncoder;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

#[test]
fn lists_and_extracts_zip_members() {
    let temp = TempDir::new();
    let path = temp.path().join("sample.zip");
    let file = File::create(&path).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file("folder/001.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"first").unwrap();
    zip.start_file("folder/010.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"tenth").unwrap();
    zip.start_file("../outside.txt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"unsafe").unwrap();
    zip.finish().unwrap();

    let index = ArchiveIndex::open(&path, None).unwrap();
    assert!(index.member("folder/001.txt").is_some());
    assert!(index.member("folder/010.txt").is_some());
    assert_eq!(index.members.len(), 2);

    let mut output = Vec::new();
    extract_member(&index, "folder/010.txt", None, &mut output).unwrap();
    assert_eq!(output, b"tenth");
}

#[test]
fn lists_and_extracts_a_gzip_compressed_tar() {
    let temp = TempDir::new();
    let path = temp.path().join("sample.tar.gz");
    let encoder = GzEncoder::new(File::create(&path).unwrap(), Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    let data = b"from tar";
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "folder/file.txt", &data[..])
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap();

    let index = ArchiveIndex::open(&path, None).unwrap();
    assert!(
        index.member("folder/file.txt").is_some(),
        "members: {:?}",
        index.members
    );
    let mut output = Vec::new();
    extract_member(&index, "folder/file.txt", None, &mut output).unwrap();
    assert_eq!(output, data);
}

#[test]
fn lists_and_extracts_7z_members() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("episode-001.txt"), b"first episode").unwrap();
    let path = temp.path().join("sample.7z");
    sevenz_rust2::compress_to_path(&source, &path).unwrap();

    let index = ArchiveIndex::open(&path, None).unwrap();
    assert!(index.member("episode-001.txt").is_some());
    let mut output = Vec::new();
    extract_member(&index, "episode-001.txt", None, &mut output).unwrap();
    assert_eq!(output, b"first episode");
}

#[test]
fn encrypted_7z_requires_and_validates_a_password() {
    let temp = TempDir::new();
    let source = temp.path().join("secret.txt");
    std::fs::write(&source, b"classified").unwrap();
    let path = temp.path().join("secret.7z");
    sevenz_rust2::compress_to_path_encrypted(&source, &path, "correct horse".into()).unwrap();

    match ArchiveIndex::open(&path, None) {
        Err(ArchiveOpenError::PasswordRequired(_)) => {}
        other => panic!("expected password requirement, got {other:?}"),
    }
    assert!(matches!(
        ArchiveIndex::open(&path, Some("wrong password")),
        Err(ArchiveOpenError::InvalidPassword(_))
    ));

    let index = ArchiveIndex::open(&path, Some("correct horse")).unwrap();
    let mut output = Vec::new();
    extract_member(&index, "secret.txt", Some("correct horse"), &mut output).unwrap();
    assert_eq!(output, b"classified");
}

#[test]
#[ignore = "Encrypted RAR archives depend on external CLI unpacker password support"]
fn encrypted_rar_requires_and_validates_a_password() {
    const ARCHIVE: &[u8] = &[
        0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00, 0xcf, 0x90, 0x73, 0x00, 0x00, 0x0d, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xd3, 0xd9, 0x74, 0x24, 0x84, 0x32, 0x00, 0x20, 0x00, 0x00,
        0x00, 0x12, 0x00, 0x00, 0x00, 0x03, 0xf3, 0x8a, 0x03, 0x6e, 0x2d, 0x81, 0x03, 0x47, 0x1d,
        0x33, 0x0a, 0x00, 0xa4, 0x81, 0x00, 0x00, 0x2e, 0x67, 0x69, 0x74, 0x69, 0x67, 0x6e, 0x6f,
        0x72, 0x65, 0x89, 0x04, 0xba, 0x8c, 0x93, 0x06, 0x43, 0x22, 0x1f, 0x39, 0x85, 0xf9, 0x6f,
        0x25, 0x5f, 0x39, 0xcf, 0xe9, 0x21, 0x24, 0x06, 0x56, 0x3c, 0x12, 0x4f, 0x90, 0x06, 0xca,
        0xfc, 0xd9, 0x62, 0xd8, 0x5f, 0xf0, 0xc7, 0x23, 0x32, 0xa5, 0x2e, 0x6d, 0xc4, 0x3d, 0x7b,
        0x00, 0x40, 0x07, 0x00,
    ];
    let temp = TempDir::new();
    let path = temp.path().join("secret.rar");
    std::fs::write(&path, ARCHIVE).unwrap();

    assert!(matches!(
        ArchiveIndex::open(&path, None),
        Err(ArchiveOpenError::PasswordRequired(_))
    ));
    assert!(matches!(
        ArchiveIndex::open(&path, Some("wrong")),
        Err(ArchiveOpenError::InvalidPassword(_))
    ));

    let index = ArchiveIndex::open(&path, Some("unrar")).unwrap();
    let mut output = Vec::new();
    extract_member(&index, ".gitignore", Some("unrar"), &mut output).unwrap();
    assert_eq!(output, b"target\nCargo.lock\n");
}

#[test]
fn treats_standalone_gzip_as_one_named_member() {
    let temp = TempDir::new();
    let path = temp.path().join("notes.txt.gz");
    let mut encoder = GzEncoder::new(File::create(&path).unwrap(), Compression::fast());
    encoder.write_all(b"compressed notes").unwrap();
    encoder.finish().unwrap();

    let index = ArchiveIndex::open(&path, None).unwrap();
    assert_eq!(index.members.len(), 1);
    assert_eq!(index.members[0].path, "notes.txt");
    let mut output = Vec::new();
    extract_member(&index, "notes.txt", None, &mut output).unwrap();
    assert_eq!(output, b"compressed notes");
}

#[test]
fn treats_standalone_zstd_as_one_named_member() {
    let temp = TempDir::new();
    let path = temp.path().join("video.raw.zst");
    let compressed = structured_zstd::encoding::compress_to_vec(
        &b"zstandard payload"[..],
        structured_zstd::encoding::CompressionLevel::from_level(1),
    );
    std::fs::write(&path, compressed).unwrap();

    let index = ArchiveIndex::open(&path, None).unwrap();
    assert_eq!(index.members[0].path, "video.raw");
    let mut output = Vec::new();
    extract_member(&index, "video.raw", None, &mut output).unwrap();
    assert_eq!(output, b"zstandard payload");
}
