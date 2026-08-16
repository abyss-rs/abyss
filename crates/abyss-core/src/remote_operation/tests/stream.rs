use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;

use crate::progress::CopyStats;
use crate::remote_operation::download::download_from_backend;
use crate::remote_operation::file::{FileWrite, stream_between_backends};
use crate::storage::{
    ByteStream, EntryKind, ErrorKind, ListPage, ReadOptions, RemoteLocation, StorageBackend,
    StorageEntry, StorageError, StoragePath, WriteOptions,
};

struct StreamingMock {
    source: bool,
    chunks: usize,
    chunk_size: usize,
    produced: Arc<AtomicUsize>,
    consumed: Arc<AtomicUsize>,
    max_lead: Arc<AtomicUsize>,
}

#[async_trait]
impl StorageBackend for StreamingMock {
    fn connection_id(&self) -> &str {
        if self.source { "source" } else { "destination" }
    }

    fn capabilities(&self) -> crate::storage::BackendCapabilities {
        Default::default()
    }

    async fn list(
        &self,
        _path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        Err(mock_unsupported())
    }

    async fn stat(&self, _path: &StoragePath) -> Result<StorageEntry, StorageError> {
        Err(mock_unsupported())
    }

    async fn read(
        &self,
        _path: &StoragePath,
        _options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        if !self.source {
            return Err(mock_unsupported());
        }
        let chunks = self.chunks;
        let chunk_size = self.chunk_size;
        let produced = Arc::clone(&self.produced);
        Ok(Box::pin(futures_util::stream::unfold(
            0usize,
            move |index| {
                let produced = Arc::clone(&produced);
                async move {
                    if index >= chunks {
                        return None;
                    }
                    produced.fetch_add(1, Ordering::Relaxed);
                    Some((Ok(Bytes::from(vec![0x5a; chunk_size])), index + 1))
                }
            },
        )))
    }

    async fn write(
        &self,
        _path: &StoragePath,
        mut source: ByteStream,
        options: WriteOptions,
    ) -> Result<(), StorageError> {
        if self.source {
            return Err(mock_unsupported());
        }
        assert_eq!(options.size, Some((self.chunks * self.chunk_size) as u64));
        while let Some(chunk) = source.next().await {
            let chunk = chunk?;
            assert_eq!(chunk.len(), self.chunk_size);
            let produced = self.produced.load(Ordering::Relaxed);
            let consumed = self.consumed.load(Ordering::Relaxed);
            let lead = produced.saturating_sub(consumed);
            self.max_lead.fetch_max(lead, Ordering::Relaxed);
            self.consumed.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    async fn create_dir(&self, _path: &StoragePath) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn delete(&self, _path: &StoragePath, _recursive: bool) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn copy(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn rename(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }
}

fn mock_unsupported() -> StorageError {
    StorageError::new(ErrorKind::Unsupported, "not used by streaming mock")
}

struct ResumableDownloadMock {
    reads: AtomicUsize,
    offsets: std::sync::Mutex<Vec<u64>>,
}

#[async_trait]
impl StorageBackend for ResumableDownloadMock {
    fn connection_id(&self) -> &str {
        "resume-source"
    }

    fn capabilities(&self) -> crate::storage::BackendCapabilities {
        crate::storage::BackendCapabilities {
            range_read: true,
            ..Default::default()
        }
    }

    async fn list(
        &self,
        _path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        Err(mock_unsupported())
    }

    async fn stat(&self, _path: &StoragePath) -> Result<StorageEntry, StorageError> {
        Ok(StorageEntry {
            name: b"object".to_vec(),
            kind: EntryKind::File,
            size: Some(10),
            modified: None,
            version: Some("stable-version".to_owned()),
        })
    }

    async fn read(
        &self,
        _path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        let offset = options.offset.unwrap_or(0);
        assert_eq!(options.expected_version.as_deref(), Some("stable-version"));
        self.offsets.lock().unwrap().push(offset);
        let call = self.reads.fetch_add(1, Ordering::Relaxed);
        if call == 0 {
            assert_eq!(offset, 0);
            return Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(Bytes::from_static(b"0123")),
                Err(
                    StorageError::new(ErrorKind::Transport, "simulated disconnect").retryable(true),
                ),
            ])));
        }
        assert_eq!(offset, 4);
        Ok(Box::pin(futures_util::stream::once(async {
            Ok(Bytes::from_static(b"456789"))
        })))
    }

    async fn write(
        &self,
        _path: &StoragePath,
        _source: ByteStream,
        _options: WriteOptions,
    ) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn create_dir(&self, _path: &StoragePath) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn delete(&self, _path: &StoragePath, _recursive: bool) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn copy(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }

    async fn rename(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        Err(mock_unsupported())
    }
}

#[tokio::test]
async fn remote_download_resumes_from_durable_partial_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("download.bin");
    let resume_root = directory.path().join("resume");
    let backend = Arc::new(ResumableDownloadMock {
        reads: AtomicUsize::new(0),
        offsets: std::sync::Mutex::new(Vec::new()),
    });
    let source = RemoteLocation {
        scheme: "mock".to_owned(),
        connection: "resume-source".to_owned(),
        path: StoragePath::Remote("bucket/object".to_owned()),
    };
    let first = download_from_backend(
        backend.clone(),
        &source,
        &destination,
        10,
        false,
        &Arc::new(AtomicBool::new(false)),
        &Arc::new(CopyStats::default()),
        Some(&resume_root),
    )
    .await;
    assert!(first.is_err());
    assert!(!destination.exists(), "partial destination became visible");

    download_from_backend(
        backend.clone(),
        &source,
        &destination,
        10,
        false,
        &Arc::new(AtomicBool::new(false)),
        &Arc::new(CopyStats::default()),
        Some(&resume_root),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"0123456789");
    assert_eq!(*backend.offsets.lock().unwrap(), vec![0, 4]);
    assert!(
        std::fs::read_dir(&resume_root)
            .unwrap()
            .flat_map(Result::into_iter)
            .all(|entry| entry.file_type().unwrap().is_dir()),
        "resume journal was not removed after publication"
    );
}

#[tokio::test]
async fn cross_provider_copy_has_one_chunk_backpressure_and_bounded_memory() {
    const CHUNKS: usize = 4_096;
    const CHUNK_SIZE: usize = 256 * 1024;
    let produced = Arc::new(AtomicUsize::new(0));
    let consumed = Arc::new(AtomicUsize::new(0));
    let max_lead = Arc::new(AtomicUsize::new(0));
    let source: Arc<dyn StorageBackend> = Arc::new(StreamingMock {
        source: true,
        chunks: CHUNKS,
        chunk_size: CHUNK_SIZE,
        produced: Arc::clone(&produced),
        consumed: Arc::clone(&consumed),
        max_lead: Arc::clone(&max_lead),
    });
    let destination: Arc<dyn StorageBackend> = Arc::new(StreamingMock {
        source: false,
        chunks: CHUNKS,
        chunk_size: CHUNK_SIZE,
        produced: Arc::clone(&produced),
        consumed: Arc::clone(&consumed),
        max_lead: Arc::clone(&max_lead),
    });
    stream_between_backends(
        source,
        destination,
        &StoragePath::Remote("source".to_owned()),
        &StoragePath::Remote("destination".to_owned()),
        (CHUNKS * CHUNK_SIZE) as u64,
        Arc::new(AtomicBool::new(false)),
        Arc::new(CopyStats::default()),
        FileWrite {
            overwrite: false,
            expected_version: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(produced.load(Ordering::Relaxed), CHUNKS);
    assert_eq!(consumed.load(Ordering::Relaxed), CHUNKS);
    assert_eq!(
        max_lead.load(Ordering::Relaxed),
        1,
        "the destination must backpressure the source without read-ahead"
    );
}
