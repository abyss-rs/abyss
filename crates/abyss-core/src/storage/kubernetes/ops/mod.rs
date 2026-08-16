pub(crate) mod helpers;
pub(crate) mod tree;

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use kube::Api;
use kube::api::PostParams;
use kube::core::{ApiResource, DynamicObject, GroupVersionKind};
use uuid::Uuid;

pub(crate) use self::helpers::*;
use super::backend::KubernetesBackend;
use super::protocol::{map_kube_error, storage_entry};
use crate::storage::helper_protocol::{HelperOperation, HelperResult};
use crate::storage::{
    BackendCapabilities, ByteStream, EntryKind, ErrorKind, ListPage, ReadOptions, StorageBackend,
    StorageEntry, StorageError, StoragePath, TreeEntry, TreeState, TreeWriteEntry, WireProgress,
    WriteOptions,
};

#[async_trait]
impl StorageBackend for KubernetesBackend {
    fn connection_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            read_only: false,
            paginated_list: false,
            range_read: true,
            multipart_write: false,
            create_dir: true,
            recursive_delete: true,
            server_side_copy: false,
            atomic_rename: true,
            conditional_write: false,
            checksum: false,
            real_directories: true,
            bulk_tree_read: true,
            bulk_tree_write: true,
            server_side_tree_copy: true,
            volume_snapshot: true,
        }
    }

    async fn probe(&self) -> Result<(), StorageError> {
        self.list_namespaces_with_claims().await.map(|_| ())
    }

    async fn list(
        &self,
        path: &StoragePath,
        _continuation: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        let parts = Self::parts(path)?;
        match parts.len() {
            0 => self.list_namespaces_with_claims().await,
            1 => {
                self.list_claims(Self::text_component(parts, 0, "namespace")?, true)
                    .await
            }
            _ => {
                let namespace = Self::text_component(parts, 0, "namespace")?;
                let pvc = Self::text_component(parts, 1, "PVC")?;
                let (result, _) = self
                    .exchange(
                        namespace,
                        pvc,
                        HelperOperation::List {
                            path: parts[2..].to_vec(),
                        },
                        None,
                    )
                    .await?;
                let HelperResult::Entries(entries) = result else {
                    return Err(StorageError::new(
                        ErrorKind::Transport,
                        "helper returned the wrong response for list",
                    ));
                };
                Ok(ListPage {
                    entries: entries.into_iter().map(storage_entry).collect(),
                    continuation: None,
                })
            }
        }
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError> {
        let parts = Self::parts(path)?;
        if parts.len() <= 2 {
            let name = parts.last().cloned().unwrap_or_default();
            return Ok(StorageEntry {
                name,
                kind: EntryKind::Directory,
                size: None,
                modified: None,
                version: None,
            });
        }
        let (result, _) = self
            .exchange(
                Self::text_component(parts, 0, "namespace")?,
                Self::text_component(parts, 1, "PVC")?,
                HelperOperation::Stat {
                    path: parts[2..].to_vec(),
                },
                None,
            )
            .await?;
        let HelperResult::Entry(entry) = result else {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "helper returned the wrong response for stat",
            ));
        };
        Ok(storage_entry(entry))
    }

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError> {
        let parts = Self::parts(path)?;
        self.read_stream(
            Self::text_component(parts, 0, "namespace")?,
            Self::text_component(parts, 1, "PVC")?,
            HelperOperation::Read {
                path: parts[2..].to_vec(),
                offset: options.offset.unwrap_or(0),
                length: options.length,
            },
            false,
            false,
        )
        .await
    }

    async fn write(
        &self,
        path: &StoragePath,
        source: ByteStream,
        options: WriteOptions,
    ) -> Result<(), StorageError> {
        let parts = Self::parts(path)?;
        let mut staged = None;
        let (size, source) = if let Some(size) = options.size {
            (size, source)
        } else {
            use tokio::io::AsyncWriteExt as _;
            let temporary = tempfile::NamedTempFile::new().map_err(|error| {
                StorageError::new(
                    ErrorKind::Other,
                    format!("create Kubernetes upload staging: {error}"),
                )
            })?;
            let mut file = tokio::fs::File::from_std(temporary.reopen().map_err(map_kube_error)?);
            let mut source = source;
            let mut size = 0_u64;
            while let Some(chunk) = source.next().await {
                let chunk = chunk?;
                file.write_all(&chunk).await.map_err(map_kube_error)?;
                size = size.saturating_add(chunk.len() as u64);
            }
            file.flush().await.map_err(map_kube_error)?;
            drop(file);
            let reader = tokio::fs::File::from_std(temporary.reopen().map_err(map_kube_error)?);
            let stream = futures_util::TryStreamExt::map_err(
                tokio_util::io::ReaderStream::new(reader),
                map_kube_error,
            );
            staged = Some(temporary);
            (size, Box::pin(stream) as ByteStream)
        };
        self.exchange(
            Self::text_component(parts, 0, "namespace")?,
            Self::text_component(parts, 1, "PVC")?,
            HelperOperation::Write {
                path: parts[2..].to_vec(),
                size,
                overwrite: options.overwrite,
            },
            Some(source),
        )
        .await?;
        drop(staged);
        Ok(())
    }

    async fn create_dir(&self, path: &StoragePath) -> Result<(), StorageError> {
        let parts = Self::parts(path)?;
        self.exchange(
            Self::text_component(parts, 0, "namespace")?,
            Self::text_component(parts, 1, "PVC")?,
            HelperOperation::CreateDir {
                path: parts[2..].to_vec(),
            },
            None,
        )
        .await?;
        Ok(())
    }

    async fn delete(&self, path: &StoragePath, recursive: bool) -> Result<(), StorageError> {
        let parts = Self::parts(path)?;
        self.exchange(
            Self::text_component(parts, 0, "namespace")?,
            Self::text_component(parts, 1, "PVC")?,
            HelperOperation::Delete {
                path: parts[2..].to_vec(),
                recursive,
            },
            None,
        )
        .await?;
        Ok(())
    }

    async fn copy(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        overwrite: bool,
    ) -> Result<(), StorageError> {
        let size = self.stat(source).await?.size;
        let stream = self.read(source, ReadOptions::default()).await?;
        self.write(
            destination,
            stream,
            WriteOptions {
                size,
                overwrite,
                ..Default::default()
            },
        )
        .await
    }

    async fn rename(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        overwrite: bool,
    ) -> Result<(), StorageError> {
        let source = Self::parts(source)?;
        let destination = Self::parts(destination)?;
        if source.get(..2) != destination.get(..2) {
            self.copy(
                &StoragePath::Kubernetes(source.to_vec()),
                &StoragePath::Kubernetes(destination.to_vec()),
                overwrite,
            )
            .await?;
            return self
                .delete(&StoragePath::Kubernetes(source.to_vec()), false)
                .await;
        }
        self.exchange(
            Self::text_component(source, 0, "namespace")?,
            Self::text_component(source, 1, "PVC")?,
            HelperOperation::Rename {
                source: source[2..].to_vec(),
                destination: destination[2..].to_vec(),
                overwrite,
            },
            None,
        )
        .await?;
        Ok(())
    }

    async fn list_tree(&self, root: &StoragePath) -> Result<Vec<TreeEntry>, StorageError> {
        self.list_tree_impl(root).await
    }

    async fn inspect_tree(
        &self,
        root: &StoragePath,
        entries: &[TreeEntry],
    ) -> Result<Vec<Option<TreeState>>, StorageError> {
        self.inspect_tree_impl(root, entries).await
    }

    async fn read_tree(
        &self,
        root: &StoragePath,
        entries: Vec<TreeEntry>,
        wire_progress: Option<WireProgress>,
    ) -> Result<ByteStream, StorageError> {
        self.read_tree_impl(root, entries, wire_progress).await
    }

    async fn write_tree(
        &self,
        root: &StoragePath,
        entries: Vec<TreeWriteEntry>,
        source: ByteStream,
        wire_progress: Option<WireProgress>,
    ) -> Result<(), StorageError> {
        self.write_tree_impl(root, entries, source, wire_progress)
            .await
    }

    async fn copy_tree(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        entries: Vec<TreeWriteEntry>,
    ) -> Result<(), StorageError> {
        self.copy_tree_impl(source, destination, entries).await
    }

    async fn create_snapshot(&self, path: &StoragePath) -> Result<String, StorageError> {
        let parts = Self::parts(path)?;
        if parts.len() < 2 {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "select a Kubernetes PVC or an item inside one to create a snapshot",
            ));
        }
        let namespace = Self::text_component(parts, 0, "namespace")?;
        let pvc = Self::text_component(parts, 1, "PVC")?;
        let gvk = GroupVersionKind::gvk("snapshot.storage.k8s.io", "v1", "VolumeSnapshot");
        let resource = ApiResource::from_gvk(&gvk);
        let snapshots: Api<DynamicObject> =
            Api::namespaced_with(self.client.clone(), namespace, &resource);
        let suffix = Uuid::new_v4().simple().to_string();
        let max_pvc = 63_usize.saturating_sub("-abyss-".len() + 8);
        let pvc_prefix = pvc.chars().take(max_pvc).collect::<String>();
        let name = format!("{pvc_prefix}-abyss-{}", &suffix[..8]);
        let snapshot = volume_snapshot_object(&name, pvc, &resource);
        snapshots
            .create(&PostParams::default(), &snapshot)
            .await
            .map_err(map_kube_error)?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            let snapshot = snapshots.get(&name).await.map_err(map_kube_error)?;
            if snapshot
                .data
                .pointer("/status/readyToUse")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                return Ok(name);
            }
            if let Some(message) = snapshot
                .data
                .pointer("/status/error/message")
                .and_then(serde_json::Value::as_str)
            {
                return Err(StorageError::new(
                    ErrorKind::Other,
                    format!("VolumeSnapshot {name} failed: {message}"),
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(StorageError::new(
                    ErrorKind::Timeout,
                    format!(
                        "VolumeSnapshot {name} was created but did not become ready within 120 seconds"
                    ),
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn shutdown(&self) -> Result<(), StorageError> {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.delete_sessions().await
    }
}
