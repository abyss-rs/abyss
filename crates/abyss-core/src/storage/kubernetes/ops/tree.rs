use super::super::backend::KubernetesBackend;
use super::super::compression::{
    count_wire_stream, decode_brotli_stream, decode_deflate_stream, decode_lz4_stream,
    encode_brotli_stream, encode_deflate_stream, encode_lz4_stream, tree_compression,
};
use super::super::protocol::{
    entry_kind_from_helper, helper_tree_entry, helper_tree_write_entry, tree_entry_from_helper,
};
use crate::storage::helper_protocol::{HelperCompression, HelperOperation, HelperResult};
use crate::storage::{
    ByteStream, ErrorKind, StorageError, StoragePath, TreeEntry, TreeState, TreeWriteEntry,
    WireProgress,
};

impl KubernetesBackend {
    pub(crate) async fn list_tree_impl(
        &self,
        root: &StoragePath,
    ) -> Result<Vec<TreeEntry>, StorageError> {
        let parts = Self::parts(root)?;
        let namespace = Self::text_component(parts, 0, "namespace")?;
        let pvc = Self::text_component(parts, 1, "PVC")?;
        self.ensure_bulk(namespace, pvc).await?;
        let (result, _) = self
            .exchange(
                namespace,
                pvc,
                HelperOperation::ListTree {
                    root: parts[2..].to_vec(),
                },
                None,
            )
            .await?;
        let HelperResult::TreeEntries(entries) = result else {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "helper returned the wrong response for bulk listing",
            ));
        };
        Ok(entries.into_iter().map(tree_entry_from_helper).collect())
    }

    pub(crate) async fn inspect_tree_impl(
        &self,
        root: &StoragePath,
        entries: &[TreeEntry],
    ) -> Result<Vec<Option<TreeState>>, StorageError> {
        let parts = Self::parts(root)?;
        let namespace = Self::text_component(parts, 0, "namespace")?;
        let pvc = Self::text_component(parts, 1, "PVC")?;
        self.ensure_bulk(namespace, pvc).await?;
        let (result, _) = self
            .exchange(
                namespace,
                pvc,
                HelperOperation::InspectTree {
                    root: parts[2..].to_vec(),
                    entries: entries.iter().map(helper_tree_entry).collect(),
                },
                None,
            )
            .await?;
        let HelperResult::TreeStates(states) = result else {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "helper returned the wrong response for bulk inspection",
            ));
        };
        Ok(states
            .into_iter()
            .map(|kind| {
                kind.map(|kind| TreeState {
                    kind: entry_kind_from_helper(kind),
                })
            })
            .collect())
    }

    pub(crate) async fn read_tree_impl(
        &self,
        root: &StoragePath,
        entries: Vec<TreeEntry>,
        wire_progress: Option<WireProgress>,
    ) -> Result<ByteStream, StorageError> {
        let parts = Self::parts(root)?;
        let namespace = Self::text_component(parts, 0, "namespace")?;
        let pvc = Self::text_component(parts, 1, "PVC")?;
        self.ensure_bulk(namespace, pvc).await?;
        let compression = tree_compression(&entries);
        let stream = self
            .read_stream(
                namespace,
                pvc,
                HelperOperation::ReadTree {
                    root: parts[2..].to_vec(),
                    entries: entries.iter().map(helper_tree_entry).collect(),
                    compression,
                },
                matches!(compression, HelperCompression::Lz4),
                true,
            )
            .await?;
        Ok(match compression {
            HelperCompression::None => count_wire_stream(stream, wire_progress),
            HelperCompression::Lz4 => decode_lz4_stream(stream, wire_progress),
            HelperCompression::Brotli => decode_brotli_stream(stream, wire_progress),
            HelperCompression::Deflate => decode_deflate_stream(stream, wire_progress),
        })
    }

    pub(crate) async fn write_tree_impl(
        &self,
        root: &StoragePath,
        entries: Vec<TreeWriteEntry>,
        source: ByteStream,
        wire_progress: Option<WireProgress>,
    ) -> Result<(), StorageError> {
        let parts = Self::parts(root)?;
        let namespace = Self::text_component(parts, 0, "namespace")?;
        let pvc = Self::text_component(parts, 1, "PVC")?;
        self.ensure_bulk(namespace, pvc).await?;
        let compression = tree_compression(
            &entries
                .iter()
                .map(|entry| entry.entry.clone())
                .collect::<Vec<_>>(),
        );
        let source = match compression {
            HelperCompression::None => count_wire_stream(source, wire_progress),
            HelperCompression::Lz4 => encode_lz4_stream(source, wire_progress),
            HelperCompression::Brotli => encode_brotli_stream(source, wire_progress),
            HelperCompression::Deflate => encode_deflate_stream(source, wire_progress),
        };
        let operation = HelperOperation::WriteTree {
            root: parts[2..].to_vec(),
            entries: entries.iter().map(helper_tree_write_entry).collect(),
            compression,
        };
        let forwarded = self
            .forward_support
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .get(&(namespace.to_owned(), pvc.to_owned()))
            .copied()
            .unwrap_or(false);
        if forwarded
            && matches!(
                compression,
                HelperCompression::None | HelperCompression::Lz4
            )
        {
            self.exchange_forwarded_scaled(namespace, pvc, operation, Some(source))
                .await?;
        } else {
            self.exchange_scaled(namespace, pvc, operation, Some(source))
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn copy_tree_impl(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        entries: Vec<TreeWriteEntry>,
    ) -> Result<(), StorageError> {
        let source = Self::parts(source)?;
        let destination = Self::parts(destination)?;
        if source.get(..2) != destination.get(..2) {
            return Err(StorageError::new(
                ErrorKind::Unsupported,
                "server-side tree copy requires the same PVC",
            ));
        }
        let namespace = Self::text_component(source, 0, "namespace")?;
        let pvc = Self::text_component(source, 1, "PVC")?;
        self.ensure_bulk(namespace, pvc).await?;
        self.exchange(
            namespace,
            pvc,
            HelperOperation::CopyTree {
                source: source[2..].to_vec(),
                destination: destination[2..].to_vec(),
                entries: entries.iter().map(helper_tree_write_entry).collect(),
            },
            None,
        )
        .await?;
        Ok(())
    }
}
