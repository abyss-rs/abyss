use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Pod};
use kube::Api;
use kube::api::{DeleteParams, PostParams};
use uuid::Uuid;

use super::super::protocol::map_kube_error;
use super::super::session::{
    HelperSession, create_helper_pod_spec, migration_worker_limit, pod_image_startup_failure,
    pod_startup_failure, select_helper_session,
};
use super::KubernetesBackend;
use crate::storage::{ErrorKind, StorageError};

impl KubernetesBackend {
    pub(crate) async fn session(
        &self,
        namespace: &str,
        pvc: &str,
    ) -> Result<HelperSession, StorageError> {
        self.session_for(namespace, pvc, false).await
    }

    pub(crate) async fn scaled_session(
        &self,
        namespace: &str,
        pvc: &str,
    ) -> Result<HelperSession, StorageError> {
        self.session_for(namespace, pvc, true).await
    }

    pub(crate) async fn session_for(
        &self,
        namespace: &str,
        pvc: &str,
        scaled: bool,
    ) -> Result<HelperSession, StorageError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(StorageError::new(
                ErrorKind::Cancelled,
                "Kubernetes storage session is shutting down",
            ));
        }
        let key = (namespace.to_owned(), pvc.to_owned());
        let _gate = self.session_gate.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(StorageError::new(
                ErrorKind::Cancelled,
                "Kubernetes storage session is shutting down",
            ));
        }
        let expired = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|value| value.into_inner());
            let pool = sessions.entry(key.clone()).or_default();
            let mut expired = Vec::new();
            pool.retain(|session| {
                if session.created.elapsed() < Duration::from_secs(23 * 60 * 60) {
                    true
                } else {
                    expired.push(session.clone());
                    false
                }
            });
            if !scaled && let Some(session) = pool.first() {
                return Ok(session.clone());
            }
            if scaled && pool.len() >= self.connection.resolved_migration_workers() {
                return Ok(select_helper_session(pool, &self.next_session));
            }
            expired
        };
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        for session in expired {
            let _ = pods.delete(&session.pod, &DeleteParams::default()).await;
        }

        let claims: Api<PersistentVolumeClaim> = Api::namespaced(self.client.clone(), namespace);
        let claim = claims.get(pvc).await.map_err(map_kube_error)?;
        if claim
            .spec
            .as_ref()
            .and_then(|spec| spec.volume_mode.as_deref())
            == Some("Block")
        {
            return Err(StorageError::new(
                ErrorKind::Unsupported,
                "raw block PVCs cannot be browsed as files",
            ));
        }
        let access_modes = claim
            .spec
            .as_ref()
            .and_then(|spec| spec.access_modes.as_deref())
            .unwrap_or_default();
        let worker_limit =
            migration_worker_limit(self.connection.resolved_migration_workers(), access_modes);
        {
            let sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|value| value.into_inner());
            if let Some(pool) = sessions.get(&key)
                && !pool.is_empty()
                && (!scaled || pool.len() >= worker_limit)
            {
                return Ok(if scaled {
                    select_helper_session(pool, &self.next_session)
                } else {
                    pool[0].clone()
                });
            }
        }

        let node_name = if access_modes.iter().any(|mode| mode == "ReadWriteOnce") {
            let existing = self
                .sessions
                .lock()
                .unwrap_or_else(|value| value.into_inner())
                .get(&key)
                .and_then(|pool| pool.first())
                .cloned();
            if let Some(existing) = existing {
                pods.get(&existing.pod)
                    .await
                    .ok()
                    .and_then(|pod| pod.spec.and_then(|spec| spec.node_name))
            } else {
                None
            }
        } else {
            None
        };

        let helpers = self.connection.resolved_helper_images();
        let mut image_failures = Vec::new();
        'images: for helper in helpers {
            if helper.image.trim().is_empty() {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "Kubernetes helper image candidate cannot be empty",
                ));
            }
            let pod_name = format!("abyss-{}-{}", &self.owner[..8], Uuid::new_v4().simple());
            let pod = create_helper_pod_spec(
                &pod_name,
                pvc,
                &self.connection,
                &self.owner,
                &helper,
                node_name.as_deref(),
            )?;
            pods.create(&PostParams::default(), &pod)
                .await
                .map_err(map_kube_error)?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            loop {
                let pod = pods.get(&pod_name).await.map_err(map_kube_error)?;
                if let Some(message) = pod_image_startup_failure(&pod) {
                    let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
                    image_failures.push(format!("{}: {message}", helper.image));
                    continue 'images;
                }
                if let Some(message) = pod_startup_failure(&pod) {
                    let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
                    return Err(StorageError::new(ErrorKind::Other, message));
                }
                let phase = pod
                    .status
                    .as_ref()
                    .and_then(|status| status.phase.as_deref());
                match phase {
                    Some("Running") => {
                        let session = HelperSession {
                            namespace: namespace.to_owned(),
                            pod: pod_name,
                            created: Instant::now(),
                        };
                        self.sessions
                            .lock()
                            .unwrap_or_else(|value| value.into_inner())
                            .entry(key.clone())
                            .or_default()
                            .push(session.clone());
                        return Ok(session);
                    }
                    Some("Failed") | Some("Succeeded") => {
                        let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
                        return Err(StorageError::new(
                            ErrorKind::Other,
                            format!("Kubernetes helper pod entered phase {}", phase.unwrap()),
                        ));
                    }
                    _ if tokio::time::Instant::now() >= deadline => {
                        let _ = pods.delete(&pod_name, &DeleteParams::default()).await;
                        return Err(StorageError::new(
                            ErrorKind::Timeout,
                            "timed out waiting for Kubernetes helper pod; check scheduling, PVC access mode, image pull, and admission events",
                        ));
                    }
                    _ => tokio::time::sleep(Duration::from_millis(500)).await,
                }
            }
        }
        Err(StorageError::new(
            ErrorKind::NotFound,
            format!(
                "no Kubernetes helper image could start: {}",
                image_failures.join("; ")
            ),
        ))
    }

    pub(crate) fn invalidate_session(&self, namespace: &str, pvc: &str, pod: &str) {
        let key = (namespace.to_owned(), pvc.to_owned());
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|value| value.into_inner());
        if let Some(pool) = sessions.get_mut(&key) {
            pool.retain(|session| session.pod != pod);
            if pool.is_empty() {
                sessions.remove(&key);
            }
        }
    }

    pub(crate) fn take_sessions(&self) -> Vec<HelperSession> {
        self.sessions
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .drain()
            .flat_map(|(_, sessions)| sessions)
            .collect()
    }

    pub(crate) async fn delete_sessions(&self) -> Result<(), StorageError> {
        let mut first_error = None;
        for session in self.take_sessions() {
            let pods: Api<Pod> = Api::namespaced(self.client.clone(), &session.namespace);
            if let Err(error) = pods.delete(&session.pod, &DeleteParams::default()).await {
                let error = map_kube_error(error);
                if error.kind != ErrorKind::NotFound && first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
