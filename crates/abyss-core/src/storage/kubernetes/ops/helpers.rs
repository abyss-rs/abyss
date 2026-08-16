use std::collections::BTreeSet;

use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::core::{ApiResource, DynamicObject};

use crate::storage::kubernetes::descriptor::NO_PVC_MESSAGE;
use crate::storage::{EntryKind, ErrorKind, ListPage, StorageEntry, StorageError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VolumeUsage {
    pub(crate) capacity_bytes: u64,
    pub(crate) free_bytes: u64,
    pub(crate) total_inodes: u64,
    pub(crate) free_inodes: u64,
}

pub(crate) fn format_usage(usage: &VolumeUsage) -> String {
    let used = usage.capacity_bytes.saturating_sub(usage.free_bytes);
    let percent = used
        .saturating_mul(100)
        .checked_div(usage.capacity_bytes)
        .unwrap_or(0);
    let used_inodes = usage.total_inodes.saturating_sub(usage.free_inodes);
    let inode_percent = used_inodes
        .saturating_mul(100)
        .checked_div(usage.total_inodes)
        .unwrap_or(0);
    format!(
        "abyss-usage|{percent}|{used}|{}|{inode_percent}|{used_inodes}|{}",
        usage.capacity_bytes, usage.total_inodes
    )
}

pub(crate) fn claim_namespaces(
    claims: impl IntoIterator<Item = PersistentVolumeClaim>,
) -> BTreeSet<String> {
    claims
        .into_iter()
        .filter_map(|claim| claim.metadata.namespace)
        .collect()
}

pub(crate) fn namespace_page(namespaces: BTreeSet<String>) -> Result<ListPage, StorageError> {
    if namespaces.is_empty() {
        return Err(StorageError::new(ErrorKind::NotFound, NO_PVC_MESSAGE));
    }
    Ok(ListPage {
        entries: namespaces.into_iter().map(directory_entry).collect(),
        continuation: None,
    })
}

pub(crate) fn volume_snapshot_object(
    name: &str,
    pvc: &str,
    resource: &ApiResource,
) -> DynamicObject {
    DynamicObject::new(name, resource).data(serde_json::json!({
        "spec": {
            "source": {
                "persistentVolumeClaimName": pvc
            }
        }
    }))
}

pub(crate) fn directory_entry(name: String) -> StorageEntry {
    StorageEntry {
        name: name.into_bytes(),
        kind: EntryKind::Directory,
        size: None,
        modified: None,
        version: None,
    }
}
