use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::{ObjectStore, path::Path as StorePath};
use tokio::sync::Mutex;

use super::{
    BackendCapabilities, ByteStream, Connection, EntryKind, ErrorKind, ListPage,
    ProviderDescriptor, ProviderField, ProviderRegistry, ReadOptions, S3Connection, StorageBackend,
    StorageEntry, StorageError, StoragePath, StorageProviderFactory, WriteOptions,
};
use object_store::Error as ObjectStoreError;

pub struct S3Factory;

impl StorageProviderFactory for S3Factory {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &ProviderDescriptor {
            id: "s3",
            name: "S3",
            schemes: &["s3"],
            fields: &[
                ProviderField {
                    key: "bucket",
                    label: "Bucket",
                    required: true,
                    secret: false,
                },
                ProviderField {
                    key: "region",
                    label: "Region",
                    required: false,
                    secret: false,
                },
                ProviderField {
                    key: "endpoint",
                    label: "Endpoint URL",
                    required: false,
                    secret: false,
                },
                ProviderField {
                    key: "access_key_id",
                    label: "Access Key ID",
                    required: false,
                    secret: false,
                },
                ProviderField {
                    key: "secret_access_key",
                    label: "Secret Access Key",
                    required: false,
                    secret: true,
                },
            ],
            help: "Connects to AWS S3 and S3-compatible endpoints like Cloudflare R2 or MinIO.",
        }
    }

    fn create(&self, id: String, connection: Connection) -> super::BackendFuture {
        Box::pin(async move {
            let Connection::S3(connection) = connection else {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "expected S3 connection",
                ));
            };
            Ok(Arc::new(S3Backend::connect(id, connection).await?) as Arc<dyn StorageBackend>)
        })
    }
}

pub struct S3Backend {
    id: String,
    builder: AmazonS3Builder,
    stores: Mutex<HashMap<String, Arc<dyn ObjectStore>>>,
    configured_buckets: Vec<String>,
    checksums: bool,
    multipart: bool,
}

impl S3Backend {
    async fn connect(id: String, connection: S3Connection) -> Result<Self, StorageError> {
        let preset = ProviderRegistry::s3_preset(connection.preset);
        let mut builder = AmazonS3Builder::new();

        if let Some(profile) = connection.profile.as_deref() {
            unsafe {
                std::env::set_var("AWS_PROFILE", profile);
            }
        }

        if let Some(endpoint) = expand_endpoint(&connection, preset.endpoint_template)? {
            builder = builder.with_endpoint(endpoint);
        }

        if let Some(region) = connection.region.as_deref() {
            builder = builder.with_region(region);
        } else if let Some(default_region) = preset.default_region {
            builder = builder.with_region(default_region);
        }

        let force_path_style = connection
            .force_path_style
            .unwrap_or(preset.force_path_style);
        builder = builder.with_virtual_hosted_style_request(!force_path_style);

        Ok(Self {
            id,
            builder,
            stores: Mutex::new(HashMap::new()),
            configured_buckets: connection.buckets.clone(),
            checksums: !connection.disable_checksums,
            multipart: !connection.disable_multipart,
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
                "S3 paths must be remote object paths",
            ));
        };
        let (bucket, key) = path.split_once('/').unwrap_or((path.as_str(), ""));
        if bucket.is_empty() {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "S3 operation requires a bucket",
            ));
        }
        Ok((bucket, StorePath::from(key)))
    }
}

#[async_trait]
impl StorageBackend for S3Backend {
    fn connection_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            read_only: false,
            paginated_list: true,
            range_read: true,
            multipart_write: self.multipart,
            create_dir: true,
            recursive_delete: true,
            server_side_copy: true,
            atomic_rename: false,
            conditional_write: true,
            checksum: self.checksums,
            real_directories: false,
            bulk_tree_read: false,
            bulk_tree_write: false,
            server_side_tree_copy: false,
            volume_snapshot: false,
        }
    }

    async fn probe(&self) -> Result<(), StorageError> {
        let Some(bucket) = self.configured_buckets.first() else {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "configure at least one bucket",
            ));
        };
        let store = self.get_store(bucket).await?;
        store.head(&StorePath::from("")).await.ok();
        Ok(())
    }

    async fn list(
        &self,
        path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        if matches!(path, StoragePath::Remote(p) if p.is_empty()) {
            let names = self.configured_buckets.clone();
            return Ok(ListPage {
                entries: names
                    .into_iter()
                    .map(|name| StorageEntry {
                        name: name.into_bytes(),
                        kind: EntryKind::Directory,
                        size: None,
                        modified: None,
                        version: None,
                    })
                    .collect(),
                continuation: None,
            });
        }

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
                version: object.e_tag,
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
                version: meta.e_tag,
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

        if self.multipart {
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
        } else {
            let mut buffer = Vec::new();
            while let Some(chunk) = source.next().await {
                buffer.extend_from_slice(&chunk?);
            }
            store
                .put(&key, bytes::Bytes::from(buffer).into())
                .await
                .map_err(map_object_store_error)?;
        }
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

fn expand_endpoint(
    connection: &S3Connection,
    template: Option<&str>,
) -> Result<Option<String>, StorageError> {
    let endpoint = connection
        .endpoint
        .clone()
        .or_else(|| template.map(str::to_owned));
    let Some(mut endpoint) = endpoint else {
        return Ok(None);
    };
    if endpoint.contains("{account_id}") {
        let account = connection
            .account_id
            .as_deref()
            .ok_or_else(|| StorageError::new(ErrorKind::InvalidInput, "account ID required"))?;
        endpoint = endpoint.replace("{account_id}", account);
    }
    if endpoint.contains("{region}") {
        let region = connection
            .region
            .as_deref()
            .ok_or_else(|| StorageError::new(ErrorKind::InvalidInput, "region required"))?;
        endpoint = endpoint.replace("{region}", region);
    }
    Ok(Some(endpoint))
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
use object_store::ObjectStoreExt;
