use std::sync::Arc;

use super::backend::KubernetesBackend;
use crate::storage::{
    BackendFuture, Connection, ErrorKind, ProviderDescriptor, ProviderField, StorageBackend,
    StorageError, StorageProviderFactory,
};

pub(crate) const KUBE_FIELDS: &[ProviderField] = &[
    ProviderField {
        key: "kubeconfig",
        label: "Kubeconfig",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "context",
        label: "Context",
        required: true,
        secret: false,
    },
    ProviderField {
        key: "helper_image",
        label: "Legacy helper image",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "helper_images",
        label: "Ordered helper image candidates",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "image_pull_secrets",
        label: "Registry pull-secret names",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "migration_workers",
        label: "Parallel migration helper pods (1-8)",
        required: false,
        secret: false,
    },
];

pub(crate) const HELPER_PORT: u16 = 31_777;
pub(crate) const MAX_HELPER_FRAME: usize = 16 * 1024 * 1024;
pub(crate) const NO_PVC_MESSAGE: &str = "no pvc found in this cluster";

pub(crate) static KUBE_DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    id: "kubernetes",
    name: "Kubernetes PVC",
    schemes: &["kube"],
    fields: KUBE_FIELDS,
    help: "Filesystem PVC access through a managed Abyss helper pod",
};

pub struct KubernetesFactory;

impl StorageProviderFactory for KubernetesFactory {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &KUBE_DESCRIPTOR
    }

    fn create(&self, id: String, connection: Connection) -> BackendFuture {
        Box::pin(async move {
            let Connection::Kubernetes(connection) = connection else {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "the Kubernetes factory requires a Kubernetes connection",
                ));
            };
            Ok(Arc::new(KubernetesBackend::connect(id, connection).await?)
                as Arc<dyn StorageBackend>)
        })
    }
}
