use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as StorePath};
use tokio::sync::Mutex;

use super::{
    BackendCapabilities, ByteStream, Connection, EntryKind, ErrorKind, GcsConnection, ListPage,
    ProviderDescriptor, ProviderField, ReadOptions, StorageBackend, StorageEntry, StorageError,
    StoragePath, StorageProviderFactory, WriteOptions,
};
use object_store::Error as ObjectStoreError;

pub struct GcsFactory;

impl StorageProviderFactory for GcsFactory {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &ProviderDescriptor {
            id: "gcs",
            name: "Google Cloud Storage",
            schemes: &["gcs"],
            fields: &[
                ProviderField {
                    key: "project",
                    label: "Project ID",
                    required: true,
                    secret: false,
                },
                ProviderField {
                    key: "endpoint",
                    label: "Endpoint URL",
                    required: false,
                    secret: false,
                },
                ProviderField {
                    key: "credential_path",
                    label: "Credential Path",
                    required: false,
                    secret: false,
                },
            ],
            help: "Connects to Google Cloud Storage.",
        }
    }

    fn create(&self, id: String, connection: Connection) -> super::BackendFuture {
        Box::pin(async move {
            let Connection::Gcs(connection) = connection else {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "expected GCS connection",
                ));
            };
            Ok(Arc::new(GcsBackend::connect(id, connection).await?) as Arc<dyn StorageBackend>)
        })
    }
}

pub struct GcsBackend {
    id: String,
    builder: GoogleCloudStorageBuilder,
    stores: Mutex<HashMap<String, Arc<dyn ObjectStore>>>,
}

impl GcsBackend {
    async fn connect(id: String, connection: GcsConnection) -> Result<Self, StorageError> {
        let mut builder = GoogleCloudStorageBuilder::new();
        if let Some(path) = connection.credential_path {
            builder = builder.with_service_account_path(path.to_string_lossy().to_string());
        }
        Ok(Self {
            id,
            builder,
            stores: Mutex::new(HashMap::new()),
        })
    }

    async fn get_store(&self, bucket: &str) -> Result<Arc<dyn ObjectStore>, StorageError> {
        let mut stores = self.stores.lock().await;
        if let Some(store) = stores.get(bucket) {
            return Ok(store.clone());
        }
        let builder = self.builder.clone().with_bucket_name(bucket);
        let store = builder.build().map_err(map_object_store_error)?;
        let arc = Arc::new(store) as Arc<dyn ObjectStore>;
        stores.insert(bucket.to_owned(), arc.clone());
        Ok(arc)
    }

    fn object_path<'a>(&self, path: &'a StoragePath) -> Result<(&'a str, StorePath), StorageError> {
        let StoragePath::Remote(path) = path else {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "GCS paths must be remote object paths",
            ));
        };
        let (bucket, key) = path.split_once('/').unwrap_or((path.as_str(), ""));
        if bucket.is_empty() {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "GCS operation requires a bucket",
            ));
        }
        Ok((bucket, StorePath::from(key)))
    }
}

#[async_trait]
impl StorageBackend for GcsBackend {
    fn connection_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            read_only: false,
            paginated_list: true,
            range_read: true,
            multipart_write: true,
            create_dir: true,
            recursive_delete: true,
            server_side_copy: true,
            atomic_rename: false,
            conditional_write: true,
            checksum: false,
            real_directories: false,
            bulk_tree_read: false,
            bulk_tree_write: false,
            server_side_tree_copy: false,
            volume_snapshot: false,
        }
    }

    async fn probe(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn list(
        &self,
        path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        let (bucket, key) = self.object_path(path)?;
        let store = self.get_store(bucket).await?;
        let prefix = if key.as_ref().is_empty() {
            None
        } else {
            Some(key.clone())
        };

        let result = store
            .list_with_delimiter(prefix.as_ref())
            .await
            .map_err(map_object_store_error)?;

        let mut entries = Vec::new();
        for dir in result.common_prefixes {
            let name = dir.filename().unwrap_or_default().as_bytes().to_vec();
            if !name.is_empty() {
                entries.push(StorageEntry {
                    name,
                    kind: EntryKind::Directory,
                    size: None,
                    modified: None,
                    version: None,
                });
            }
        }
        for object in result.objects {
            if object.location == key {
                continue;
            }
            let name = object
                .location
                .filename()
                .unwrap_or_default()
                .as_bytes()
                .to_vec();
            if name.is_empty() {
                continue;
            }
            let modified = std::time::SystemTime::from(object.last_modified);
            entries.push(StorageEntry {
                name,
                kind: EntryKind::File,
                size: Some(object.size),
                modified: Some(modified),
                version: None,
            });
        }

        Ok(ListPage {
            entries,
            continuation: None,
        })
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError> {
        let (bucket, key) = self.object_path(path)?;
        if key.as_ref().is_empty() {
            return Ok(StorageEntry {
                name: bucket.as_bytes().to_vec(),
                kind: EntryKind::Directory,
                size: None,
                modified: None,
                version: None,
            });
        }
        let store = self.get_store(bucket).await?;
        match store.head(&key).await {
            Ok(meta) => Ok(StorageEntry {
                name: meta
                    .location
                    .filename()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec(),
                kind: EntryKind::File,
                size: Some(meta.size),
                modified: Some(std::time::SystemTime::from(meta.last_modified)),
                version: None,
            }),
            Err(object_store::Error::NotFound { .. }) => {
                let prefix = key.clone();
                let result = store
                    .list_with_delimiter(Some(&prefix))
                    .await
                    .map_err(map_object_store_error)?;
                if !result.objects.is_empty() || !result.common_prefixes.is_empty() {
                    Ok(StorageEntry {
                        name: key.filename().unwrap_or_default().as_bytes().to_vec(),
                        kind: EntryKind::Directory,
                        size: None,
                        modified: None,
                        version: None,
                    })
                } else {
                    Err(StorageError::new(ErrorKind::NotFound, "not found"))
                }
            }
            Err(err) => Err(map_object_store_error(err)),
        }
    }

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        if options.length == Some(0) {
            return Ok(Box::pin(futures_util::stream::empty()));
        }
        let (bucket, key) = self.object_path(path)?;
        let store = self.get_store(bucket).await?;

        let stream = if options.offset.is_some() || options.length.is_some() {
            let offset = options.offset.unwrap_or(0);
            let range = if let Some(length) = options.length {
                offset..(offset + length)
            } else {
                offset..u64::MAX
            };
            let get_range = store
                .get_range(&key, range)
                .await
                .map_err(map_object_store_error)?;
            let bytes = futures_util::stream::once(async move { Ok(get_range) });
            Box::pin(bytes) as ByteStream
        } else {
            let get = store.get(&key).await.map_err(map_object_store_error)?;
            Box::pin(get.into_stream().map(|r| r.map_err(map_object_store_error))) as ByteStream
        };
        Ok(stream)
    }

    async fn write(
        &self,
        path: &StoragePath,
        mut source: ByteStream,
        _options: WriteOptions,
    ) -> Result<(), StorageError> {
        let (bucket, key) = self.object_path(path)?;
        let store = self.get_store(bucket).await?;
        let mut upload = store
            .put_multipart(&key)
            .await
            .map_err(map_object_store_error)?;
        while let Some(chunk) = source.next().await {
            let chunk = chunk?;
            upload
                .put_part(chunk.into())
                .await
                .map_err(map_object_store_error)?;
        }
        upload.complete().await.map_err(map_object_store_error)?;
        Ok(())
    }

    async fn create_dir(&self, _path: &StoragePath) -> Result<(), StorageError> {
        Ok(())
    }

    async fn delete(&self, path: &StoragePath, _recursive: bool) -> Result<(), StorageError> {
        let (bucket, key) = self.object_path(path)?;
        let store = self.get_store(bucket).await?;
        store.delete(&key).await.map_err(map_object_store_error)?;
        Ok(())
    }

    async fn copy(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        let (source_bucket, source_key) = self.object_path(source)?;
        let (dest_bucket, dest_key) = self.object_path(destination)?;
        if source_bucket != dest_bucket {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "cannot copy across buckets",
            ));
        }
        let store = self.get_store(source_bucket).await?;
        store
            .copy(&source_key, &dest_key)
            .await
            .map_err(map_object_store_error)?;
        Ok(())
    }

    async fn rename(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        _overwrite: bool,
    ) -> Result<(), StorageError> {
        let (source_bucket, source_key) = self.object_path(source)?;
        let (dest_bucket, dest_key) = self.object_path(destination)?;
        if source_bucket != dest_bucket {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "cannot rename across buckets",
            ));
        }
        let store = self.get_store(source_bucket).await?;
        store
            .rename(&source_key, &dest_key)
            .await
            .map_err(map_object_store_error)?;
        Ok(())
    }
}

fn map_object_store_error(error: ObjectStoreError) -> StorageError {
    let kind = match &error {
        ObjectStoreError::NotFound { .. } => ErrorKind::NotFound,
        ObjectStoreError::AlreadyExists { .. } => ErrorKind::Conflict,
        ObjectStoreError::NotSupported { .. } => ErrorKind::InvalidInput,
        _ => ErrorKind::Transport,
    };
    StorageError::new(kind, format!("object_store: {error}"))
}
