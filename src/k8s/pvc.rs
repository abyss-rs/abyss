use anyhow::Result;
use k8s_openapi::api::core::v1::{PersistentVolume, PersistentVolumeClaim};
use kube::{Api, Client};

use crate::fs::types::{PvcInfo, StorageInfo};

pub struct StorageManager {
    client: Client,
}

impl StorageManager {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn list_pvcs(&self, namespace: &str) -> Result<Vec<PvcInfo>> {
        let api: Api<PersistentVolumeClaim> = Api::namespaced(self.client.clone(), namespace);
        let pvcs = api.list(&Default::default()).await?;

        let mut result = Vec::new();

        for pvc in pvcs.items {
            let name = pvc.metadata.name.unwrap_or_default();
            let namespace = pvc.metadata.namespace.unwrap_or_default();

            let capacity = pvc
                .status
                .as_ref()
                .and_then(|s| s.capacity.as_ref())
                .and_then(|c| c.get("storage"))
                .map(|q| q.0.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let access_modes = pvc
                .spec
                .as_ref()
                .and_then(|s| s.access_modes.as_ref())
                .map(|modes| modes.clone())
                .unwrap_or_default();

            result.push(PvcInfo {
                name,
                namespace,
                capacity,
                access_modes,
            });
        }

        Ok(result)
    }

    pub async fn list_pvs(&self) -> Result<Vec<StorageInfo>> {
        let api: Api<PersistentVolume> = Api::all(self.client.clone());
        let pvs = api.list(&Default::default()).await?;

        let mut result = Vec::new();

        for pv in pvs.items {
            let name = pv.metadata.name.unwrap_or_default();

            let capacity = pv
                .spec
                .as_ref()
                .and_then(|s| s.capacity.as_ref())
                .and_then(|c| c.get("storage"))
                .map(|q| q.0.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let access_modes = pv
                .spec
                .as_ref()
                .and_then(|s| s.access_modes.as_ref())
                .map(|modes| modes.clone())
                .unwrap_or_default();

            let claim_ref = pv
                .spec
                .as_ref()
                .and_then(|s| s.claim_ref.as_ref())
                .map(|cr| {
                    format!(
                        "{}/{}",
                        cr.namespace.as_deref().unwrap_or(""),
                        cr.name.as_deref().unwrap_or("")
                    )
                });

            let status = pv
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            result.push(StorageInfo {
                name,
                capacity,
                access_modes,
                claim_ref,
                status,
                is_pv: true,
            });
        }

        Ok(result)
    }

    pub async fn list_all_storage(&self) -> Result<Vec<StorageInfo>> {
        let mut result = Vec::new();

        // Add all PVs
        let pvs = self.list_pvs().await?;
        result.extend(pvs);

        Ok(result)
    }

    pub async fn get_namespaces(&self) -> Result<Vec<String>> {
        let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(self.client.clone());
        let namespaces = api.list(&Default::default()).await?;

        let mut result = Vec::new();
        for ns in namespaces.items {
            if let Some(name) = ns.metadata.name {
                result.push(name);
            }
        }

        result.sort();
        Ok(result)
    }
}
