use std::collections::BTreeSet;

use futures_util::{StreamExt, stream};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::Api;
use kube::api::ListParams;

use super::super::ops::helpers::{
    VolumeUsage, claim_namespaces, directory_entry, format_usage, namespace_page,
};
use super::super::protocol::map_kube_error;
use super::KubernetesBackend;
use crate::storage::helper_protocol::{HelperOperation, HelperResult};
use crate::storage::{ErrorKind, ListPage, StorageError};

impl KubernetesBackend {
    pub(crate) async fn list_namespaces_with_claims(&self) -> Result<ListPage, StorageError> {
        let allowed = &self.connection.namespaces;
        if !allowed.is_empty() {
            let results = stream::iter(allowed.iter().cloned())
                .map(|namespace| async move {
                    let has_claims = !self
                        .list_claims(&namespace, false)
                        .await?
                        .entries
                        .is_empty();
                    Ok::<_, StorageError>((namespace, has_claims))
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
            let mut namespaces = BTreeSet::new();
            for result in results {
                let (namespace, has_claims) = result?;
                if has_claims {
                    namespaces.insert(namespace);
                }
            }
            return namespace_page(namespaces);
        }

        let claims: Api<PersistentVolumeClaim> = Api::all(self.client.clone());
        let namespaces = claim_namespaces(
            claims
                .list(&ListParams::default())
                .await
                .map_err(map_kube_error)?
                .items,
        );
        namespace_page(namespaces)
    }

    pub(crate) async fn list_claims(
        &self,
        namespace: &str,
        include_usage: bool,
    ) -> Result<ListPage, StorageError> {
        let claims: Api<PersistentVolumeClaim> = Api::namespaced(self.client.clone(), namespace);
        let names = claims
            .list(&ListParams::default())
            .await
            .map_err(map_kube_error)?
            .items
            .into_iter()
            .filter_map(|claim| claim.metadata.name)
            .collect::<Vec<_>>();
        let entries = if include_usage {
            stream::iter(names)
                .map(|name| async move {
                    let mut entry = directory_entry(name.clone());
                    entry.version = Some(match self.pvc_usage(namespace, &name).await {
                        Ok(usage) => format_usage(&usage),
                        Err(error) => format!("abyss-usage|unavailable|{error}"),
                    });
                    entry
                })
                .buffered(4)
                .collect()
                .await
        } else {
            names.into_iter().map(directory_entry).collect()
        };
        Ok(ListPage {
            entries,
            continuation: None,
        })
    }

    pub(crate) async fn pvc_usage(
        &self,
        namespace: &str,
        pvc: &str,
    ) -> Result<VolumeUsage, StorageError> {
        let (result, _) = self
            .exchange(namespace, pvc, HelperOperation::Usage, None)
            .await?;
        let HelperResult::Usage {
            capacity_bytes,
            free_bytes,
            total_inodes,
            free_inodes,
        } = result
        else {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "Kubernetes helper returned an invalid usage response",
            ));
        };
        Ok(VolumeUsage {
            capacity_bytes,
            free_bytes,
            total_inodes,
            free_inodes,
        })
    }
}
