#[cfg(feature = "kubernetes")]
use std::sync::Arc;

#[cfg(feature = "kubernetes")]
use abyss_core::storage::{
    ByteStream, EntryKind, StorageBackend, StorageError, StoragePath, TreeEntry, TreeWriteEntry,
};
#[cfg(feature = "kubernetes")]
use bytes::Bytes;
#[cfg(feature = "kubernetes")]
use futures_util::StreamExt;

#[cfg(feature = "kubernetes")]
pub(crate) async fn run_kubernetes_bulk_contract(
    backend: Arc<dyn StorageBackend>,
    root: &StoragePath,
) -> Result<(), StorageError> {
    let source = root.child(b"bulk-source")?;
    let copied = root.child(b"bulk-copy")?;
    let file_count = std::env::var("ABYSS_KUBE_BULK_FILES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000);
    let mut entries = vec![TreeEntry {
        path: vec![b"nested".to_vec()],
        kind: EntryKind::Directory,
        size: 0,
    }];
    let mut expected = Vec::new();
    for index in 0..file_count {
        let contents = format!("bulk-file-{index:06}-").repeat(32).into_bytes();
        expected.extend_from_slice(&contents);
        entries.push(TreeEntry {
            path: vec![
                b"nested".to_vec(),
                format!("file-{index:06}.txt").into_bytes(),
            ],
            kind: EntryKind::File,
            size: contents.len() as u64,
        });
    }
    let writes = entries
        .iter()
        .cloned()
        .map(|entry| TreeWriteEntry {
            entry,
            overwrite: false,
            clone_from: None,
        })
        .collect::<Vec<_>>();
    backend
        .write_tree(
            &source,
            writes.clone(),
            one_chunk(Bytes::from(expected.clone())),
            None,
        )
        .await?;

    let listed = backend.list_tree(&source).await?;
    assert_eq!(
        listed
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .count(),
        file_count
    );
    let files = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .cloned()
        .collect::<Vec<_>>();
    let downloaded = collect(backend.read_tree(&source, files, None).await?).await?;
    assert_eq!(downloaded.as_ref(), expected.as_slice());

    let states = backend.inspect_tree(&source, &entries).await?;
    assert!(states.iter().all(Option::is_some));
    backend.copy_tree(&source, &copied, writes).await?;
    let copied_files = backend.list_tree(&copied).await?;
    assert_eq!(
        copied_files
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .count(),
        file_count
    );
    backend.delete(&copied, true).await?;
    backend.delete(&source, true).await?;
    Ok(())
}

#[cfg(feature = "kubernetes")]
fn one_chunk(value: Bytes) -> ByteStream {
    Box::pin(futures_util::stream::once(async move { Ok(value) }).fuse())
}

#[cfg(feature = "kubernetes")]
async fn collect(mut stream: ByteStream) -> Result<Bytes, StorageError> {
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        output.extend_from_slice(&chunk?);
    }
    Ok(Bytes::from(output))
}
