use std::sync::Arc;

use abyss_core::storage::{
    ByteStream, EntryKind, ReadOptions, StorageBackend, StorageError, StoragePath, WriteOptions,
};
use bytes::Bytes;
use futures_util::StreamExt;

#[cfg(feature = "kubernetes")]
use super::bulk::run_kubernetes_bulk_contract;

pub(crate) async fn run_contract(
    backend: Arc<dyn StorageBackend>,
    root: &StoragePath,
    supports_raw_names: bool,
) -> Result<(), StorageError> {
    backend.create_dir(root).await?;
    let source = root.child(b"source.bin")?;
    let copied = root.child(b"copied.bin")?;
    let renamed = root.child(b"renamed.bin")?;
    let content = Bytes::from_static(b"0123456789abcdef");
    backend
        .write(
            &source,
            one_chunk(content.clone()),
            WriteOptions {
                size: Some(content.len() as u64),
                overwrite: false,
                expected_version: None,
            },
        )
        .await?;

    let stat = backend.stat(&source).await?;
    assert_eq!(stat.kind, EntryKind::File);
    assert_eq!(stat.size, Some(content.len() as u64));

    let mut names = Vec::new();
    let mut continuation = None;
    loop {
        let page = backend.list(root, continuation.as_deref()).await?;
        names.extend(page.entries.into_iter().map(|entry| entry.name));
        continuation = page.continuation;
        if continuation.is_none() {
            break;
        }
    }
    assert!(names.iter().any(|name| name == b"source.bin"));

    let read = collect(
        backend
            .read(
                &source,
                ReadOptions {
                    offset: Some(4),
                    length: Some(6),
                    expected_version: stat.version.clone(),
                },
            )
            .await?,
    )
    .await?;
    assert_eq!(read.as_ref(), b"456789");

    if backend.capabilities().server_side_copy {
        backend.copy(&source, &copied, false).await?;
        assert_eq!(
            collect(backend.read(&copied, ReadOptions::default()).await?).await?,
            content
        );

        backend.rename(&copied, &renamed, false).await?;
        assert_eq!(
            collect(backend.read(&renamed, ReadOptions::default()).await?).await?,
            content
        );
    } else {
        backend.rename(&source, &renamed, false).await?;
        assert_eq!(
            collect(backend.read(&renamed, ReadOptions::default()).await?).await?,
            content
        );
    }

    let existing_file = if backend.capabilities().server_side_copy {
        &source
    } else {
        &renamed
    };

    let duplicate = backend
        .write(
            existing_file,
            one_chunk(Bytes::from_static(b"must-not-overwrite")),
            WriteOptions {
                size: Some(18),
                overwrite: false,
                expected_version: None,
            },
        )
        .await;
    assert!(
        duplicate.is_err(),
        "conditional create silently overwrote a file"
    );

    let broken = root.child(b"incomplete.bin")?;
    let incomplete = backend
        .write(
            &broken,
            one_chunk(Bytes::from_static(b"short")),
            WriteOptions {
                size: Some(32),
                overwrite: false,
                expected_version: None,
            },
        )
        .await;
    assert!(
        incomplete.is_err(),
        "truncated upload unexpectedly succeeded"
    );
    assert!(
        backend.stat(&broken).await.is_err(),
        "truncated upload left a visible destination"
    );

    let large = root.child(b"large.bin")?;
    let large_copy = root.child(b"large-copy.bin")?;
    let large_size: usize = std::env::var("ABYSS_CONTRACT_LARGE_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4 * 1024 * 1024);
    let large_content = Bytes::from(
        (0..large_size)
            .map(|index| ((index * 31 + 7) % 251) as u8)
            .collect::<Vec<_>>(),
    );
    if large_size > 64 * 1024 * 1024 {
        let interrupted = root.child(b"interrupted-multipart.bin")?;
        let interrupted_source = Box::pin(futures_util::stream::iter(vec![
            Ok(Bytes::from(vec![0x5a; 16 * 1024 * 1024])),
            Err(StorageError::new(
                abyss_core::storage::ErrorKind::Cancelled,
                "intentional multipart interruption",
            )),
        ])) as ByteStream;
        let result = backend
            .write(
                &interrupted,
                interrupted_source,
                WriteOptions {
                    size: Some(large_size as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await;
        assert!(result.is_err(), "interrupted multipart upload succeeded");
        assert!(
            backend.stat(&interrupted).await.is_err(),
            "interrupted multipart upload left a visible destination"
        );
    }
    backend
        .write(
            &large,
            chunks(large_content.clone(), 63 * 1024),
            WriteOptions {
                size: Some(large_content.len() as u64),
                overwrite: false,
                expected_version: None,
            },
        )
        .await?;
    let ranged = collect(
        backend
            .read(
                &large,
                ReadOptions {
                    offset: Some(1_000_003),
                    length: Some(700_019),
                    expected_version: None,
                },
            )
            .await?,
    )
    .await?;
    assert_eq!(
        ranged.as_ref(),
        &large_content[1_000_003..1_700_022],
        "large ranged read returned different bytes"
    );

    let mut abandoned = backend.read(&large, ReadOptions::default()).await?;
    assert!(abandoned.next().await.transpose()?.is_some());
    drop(abandoned);
    assert_eq!(
        backend.stat(&large).await?.size,
        Some(large_content.len() as u64),
        "aborting a read damaged its source"
    );

    if backend.capabilities().server_side_copy {
        backend.copy(&large, &large_copy, false).await?;
        let copied_tail = collect(
            backend
                .read(
                    &large_copy,
                    ReadOptions {
                        offset: Some((large_content.len() - 131_071) as u64),
                        length: None,
                        expected_version: None,
                    },
                )
                .await?,
        )
        .await?;
        assert_eq!(
            copied_tail.as_ref(),
            &large_content[large_content.len() - 131_071..]
        );
    }

    let nested = root.child(b"nested")?;
    let nested_child = nested.child(b"child")?;
    let nested_file = nested_child.child(b"value.bin")?;
    backend.create_dir(&nested_child).await?;
    backend
        .write(
            &nested_file,
            one_chunk(Bytes::from_static(b"nested")),
            WriteOptions {
                size: Some(6),
                overwrite: false,
                expected_version: None,
            },
        )
        .await?;
    backend.delete(&nested, true).await?;
    assert!(
        backend.stat(&nested).await.is_err(),
        "recursive directory delete left the directory visible"
    );

    if supports_raw_names {
        let raw_name = b"raw-\xFF.bin";
        let raw = root.child(raw_name)?;
        backend
            .write(
                &raw,
                one_chunk(Bytes::from_static(b"raw-name")),
                WriteOptions {
                    size: Some(8),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await?;
        let entries = backend.list(root, None).await?.entries;
        assert!(
            entries.iter().any(|entry| entry.name == raw_name),
            "non-UTF-8 filename did not round-trip through listing"
        );
        assert_eq!(
            collect(backend.read(&raw, ReadOptions::default()).await?).await?,
            Bytes::from_static(b"raw-name")
        );
        backend.delete(&raw, false).await?;

        #[cfg(feature = "kubernetes")]
        run_kubernetes_bulk_contract(Arc::clone(&backend), root).await?;
    }

    if backend.capabilities().server_side_copy {
        backend.delete(&large_copy, false).await?;
    }
    backend.delete(&large, false).await?;
    backend.delete(&renamed, false).await?;
    Ok(())
}

fn one_chunk(value: Bytes) -> ByteStream {
    Box::pin(futures_util::stream::once(async move { Ok(value) }).fuse())
}

fn chunks(value: Bytes, chunk_size: usize) -> ByteStream {
    Box::pin(
        futures_util::stream::unfold((value, 0), move |(value, offset)| async move {
            if offset >= value.len() {
                return None;
            }
            let end = (offset + chunk_size).min(value.len());
            let chunk = value.slice(offset..end);
            Some((Ok(chunk), (value, end)))
        })
        .fuse(),
    )
}

async fn collect(mut stream: ByteStream) -> Result<Bytes, StorageError> {
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        output.extend_from_slice(&chunk?);
    }
    Ok(Bytes::from(output))
}
