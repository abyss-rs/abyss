use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::LZ4_BLOCK;
use crate::compress::{write_brotli_block, write_deflate_block, write_lz4_block};
use crate::decompress::{BrotliBlockReader, DeflateBlockReader, Lz4BlockReader};
use crate::paths::{safe_mutation_path, safe_path_beneath};
use crate::tree::install_clone;

use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn temporary_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "abyss-helper-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&root).expect("create test root");
    root
}

#[test]
fn rejects_unsafe_components_and_mount_root_mutation() {
    let root = temporary_root();
    for component in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
        assert!(safe_path_beneath(&root, &[component.to_vec()]).is_err());
    }
    assert!(safe_mutation_path(&[]).is_err());
    fs::remove_dir(root).expect("remove test root");
}

#[test]
fn preserves_raw_names_and_refuses_symlink_traversal() {
    let root = temporary_root();
    let raw = vec![b'n', b'a', b'm', b'e', b'-', 0xff];
    let raw_path =
        safe_path_beneath(&root, std::slice::from_ref(&raw)).expect("accept raw path component");
    assert_eq!(raw_path.file_name().expect("raw filename").as_bytes(), raw);

    symlink("/etc", root.join("escape")).expect("create symlink");
    assert!(safe_path_beneath(&root, &[b"escape".to_vec(), b"passwd".to_vec()]).is_err());
    fs::remove_file(root.join("escape")).expect("remove symlink");
    fs::remove_dir(root).expect("remove test root");
}

#[test]
fn lz4_transport_round_trips_compressible_and_stored_blocks() {
    let mut encoded = Vec::new();
    let compressible = vec![b'a'; LZ4_BLOCK];
    let incompressible = (0..LZ4_BLOCK)
        .map(|index| ((index * 131 + index / 7) % 251) as u8)
        .collect::<Vec<_>>();
    write_lz4_block(&mut encoded, &compressible).expect("encode compressible block");
    write_lz4_block(&mut encoded, &incompressible).expect("encode incompressible block");

    let expected = [compressible, incompressible].concat();
    let mut input = encoded.as_slice();
    let mut decoded = Lz4BlockReader::new(&mut input, expected.len() as u64);
    let mut actual = Vec::new();
    decoded.read_to_end(&mut actual).expect("decode blocks");
    assert_eq!(actual, expected);
    assert!(input.is_empty());
}

#[test]
fn lz4_transport_rejects_truncation() {
    let mut encoded = Vec::new();
    write_lz4_block(&mut encoded, b"some useful file contents").expect("encode block");
    encoded.pop();
    let mut input = encoded.as_slice();
    let mut decoded = Lz4BlockReader::new(&mut input, 25);
    assert!(io::copy(&mut decoded, &mut io::sink()).is_err());
}

#[test]
fn pure_rust_bulk_codecs_round_trip() {
    let expected = b"provider-binary-section\0".repeat(32_768);

    let mut brotli = Vec::new();
    write_brotli_block(&mut brotli, &expected).expect("encode Brotli");
    let mut input = brotli.as_slice();
    let mut decoded = BrotliBlockReader::new(&mut input, expected.len() as u64);
    let mut actual = Vec::new();
    decoded.read_to_end(&mut actual).expect("decode Brotli");
    assert_eq!(actual, expected);

    let mut deflate = Vec::new();
    write_deflate_block(&mut deflate, &expected).expect("encode Deflate");
    let mut input = deflate.as_slice();
    let mut decoded = DeflateBlockReader::new(&mut input, expected.len() as u64);
    let mut actual = Vec::new();
    decoded.read_to_end(&mut actual).expect("decode Deflate");
    assert_eq!(actual, expected);
}

#[test]
fn clone_materialization_keeps_files_independent() {
    let root = temporary_root();
    let source = root.join("source");
    let destination = root.join("destination");
    let expected = b"independent clone contents".repeat(1024);
    fs::write(&source, &expected).expect("write clone source");
    install_clone(destination.clone(), expected.len() as u64, false, &source)
        .expect("materialize clone");
    fs::write(&destination, b"changed").expect("change destination");
    assert_eq!(fs::read(&source).expect("read source"), expected);
    fs::remove_dir_all(root).expect("remove clone root");
}
