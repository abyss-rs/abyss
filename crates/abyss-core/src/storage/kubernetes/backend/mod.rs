pub(crate) mod discovery;
pub(crate) mod exec;
pub(crate) mod forward;
pub(crate) mod session;

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};

use kube::config::{ExecInteractiveMode, KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};
use uuid::Uuid;

use super::protocol::map_kube_error;
use super::session::HelperSession;
use crate::storage::{ErrorKind, KubernetesConnection, StorageError, StoragePath};

pub struct KubernetesBackend {
    pub(crate) id: String,
    pub(crate) client: Client,
    pub(crate) connection: KubernetesConnection,
    pub(crate) owner: String,
    pub(crate) sessions: Mutex<HashMap<(String, String), Vec<HelperSession>>>,
    pub(crate) bulk_support: Mutex<HashMap<(String, String), bool>>,
    pub(crate) forward_support: Mutex<HashMap<(String, String), bool>>,
    pub(crate) session_gate: tokio::sync::Mutex<()>,
    pub(crate) next_session: AtomicU64,
    pub(crate) closed: AtomicBool,
}

impl KubernetesBackend {
    pub(crate) async fn connect(
        id: String,
        connection: KubernetesConnection,
    ) -> Result<Self, StorageError> {
        let options = KubeConfigOptions {
            context: Some(connection.context.clone()),
            ..Default::default()
        };
        let mut kubeconfig = if connection.kubeconfig.is_empty() {
            Kubeconfig::read().map_err(map_kube_error)?
        } else {
            let mut files = connection.kubeconfig.iter();
            let first = files.next().expect("checked nonempty");
            let mut merged = Kubeconfig::read_from(first).map_err(map_kube_error)?;
            for path in files {
                merged = merged
                    .merge(Kubeconfig::read_from(path).map_err(map_kube_error)?)
                    .map_err(map_kube_error)?;
            }
            merged
        };
        for named in &mut kubeconfig.auth_infos {
            if let Some(exec) = named.auth_info.as_mut().and_then(|auth| auth.exec.as_mut()) {
                exec.interactive_mode = Some(ExecInteractiveMode::Never);
            }
        }
        let config = Config::from_custom_kubeconfig(kubeconfig, &options)
            .await
            .map_err(map_kube_error)?;
        let client = Client::try_from(config).map_err(map_kube_error)?;
        Ok(Self {
            id,
            client,
            connection,
            owner: Uuid::new_v4().simple().to_string(),
            sessions: Mutex::new(HashMap::new()),
            bulk_support: Mutex::new(HashMap::new()),
            forward_support: Mutex::new(HashMap::new()),
            session_gate: tokio::sync::Mutex::new(()),
            next_session: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        })
    }

    pub(crate) fn parts(path: &StoragePath) -> Result<&[Vec<u8>], StorageError> {
        let StoragePath::Kubernetes(parts) = path else {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "Kubernetes paths must use byte components",
            ));
        };
        Ok(parts)
    }

    pub(crate) fn text_component<'a>(
        parts: &'a [Vec<u8>],
        index: usize,
        label: &str,
    ) -> Result<&'a str, StorageError> {
        std::str::from_utf8(parts.get(index).ok_or_else(|| {
            StorageError::new(
                ErrorKind::InvalidInput,
                format!("missing Kubernetes {label}"),
            )
        })?)
        .map_err(|_| {
            StorageError::new(
                ErrorKind::InvalidInput,
                format!("Kubernetes {label} must be UTF-8"),
            )
        })
    }
}

impl Drop for KubernetesBackend {
    fn drop(&mut self) {
        let sessions = self
            .sessions
            .get_mut()
            .unwrap_or_else(|value| value.into_inner())
            .drain()
            .flat_map(|(_, sessions)| sessions)
            .collect::<Vec<_>>();
        if sessions.is_empty() {
            return;
        }
        let client = self.client.clone();
        std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                for session in sessions {
                    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> =
                        kube::Api::namespaced(client.clone(), &session.namespace);
                    let _ = pods
                        .delete(&session.pod, &kube::api::DeleteParams::default())
                        .await;
                }
            });
        });
    }
}
