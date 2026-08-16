use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use k8s_openapi::api::core::v1::Pod;

use crate::storage::{ErrorKind, KubernetesConnection, KubernetesHelperImage, StorageError};

#[derive(Clone)]
pub(crate) struct HelperSession {
    pub(crate) namespace: String,
    pub(crate) pod: String,
    pub(crate) created: Instant,
}

pub(crate) fn select_helper_session(pool: &[HelperSession], next: &AtomicU64) -> HelperSession {
    let index = next.fetch_add(1, Ordering::Relaxed) as usize % pool.len();
    pool[index].clone()
}

pub(crate) fn migration_worker_limit(configured: usize, access_modes: &[String]) -> usize {
    if access_modes.iter().any(|mode| mode == "ReadWriteOncePod") {
        1
    } else {
        configured.clamp(1, 8)
    }
}

pub(crate) fn create_helper_pod_spec(
    name: &str,
    pvc: &str,
    connection: &KubernetesConnection,
    owner: &str,
    helper: &KubernetesHelperImage,
    node_name: Option<&str>,
) -> Result<Pod, StorageError> {
    if connection
        .image_pull_secrets
        .iter()
        .any(|name| name.trim().is_empty())
    {
        return Err(StorageError::new(
            ErrorKind::InvalidInput,
            "Kubernetes image pull-secret name cannot be empty",
        ));
    }
    let mut container_security = serde_json::json!({
        "allowPrivilegeEscalation": false,
        "capabilities": {"drop": ["ALL"]}
    });
    if let Some(user) = connection.run_as_user {
        container_security["runAsUser"] = serde_json::json!(user);
        container_security["runAsNonRoot"] = serde_json::json!(user != 0);
    }
    if let Some(group) = connection.run_as_group {
        container_security["runAsGroup"] = serde_json::json!(group);
    }
    let mut pod_security = serde_json::json!({
        "seccompProfile": {"type": "RuntimeDefault"}
    });
    if let Some(group) = connection.fs_group {
        pod_security["fsGroup"] = serde_json::json!(group);
    }
    let mut spec = serde_json::json!({
        "restartPolicy": "Never",
        "activeDeadlineSeconds": 86400,
        "automountServiceAccountToken": false,
        "serviceAccountName": connection.service_account,
        "securityContext": pod_security,
        "containers": [{
            "name": "helper",
            "image": helper.image,
            "imagePullPolicy": helper.pull_policy.kubernetes_value(),
            "command": ["/usr/local/bin/abyss-kube-helper", "idle"],
            "securityContext": container_security,
            "volumeMounts": [{"name": "data", "mountPath": "/data"}]
        }],
        "volumes": [{
            "name": "data",
            "persistentVolumeClaim": {"claimName": pvc, "readOnly": false}
        }]
    });
    if !connection.image_pull_secrets.is_empty() {
        spec["imagePullSecrets"] = serde_json::Value::Array(
            connection
                .image_pull_secrets
                .iter()
                .map(|name| serde_json::json!({"name": name}))
                .collect(),
        );
    }
    if let Some(node_name) = node_name {
        spec["nodeName"] = serde_json::json!(node_name);
    }
    serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "labels": {
                "app.kubernetes.io/name": "abyss-kube-helper",
                "app.kubernetes.io/created-by": "abyss",
                "abyss.vyrti.dev/owner": owner
            }
        },
        "spec": spec
    }))
    .map_err(|error| StorageError::new(ErrorKind::Other, error.to_string()))
}

pub(crate) fn pod_image_startup_failure(pod: &Pod) -> Option<String> {
    const IMAGE_WAITING: &[&str] = &[
        "ErrImageNeverPull",
        "ErrImagePull",
        "ImageInspectError",
        "ImagePullBackOff",
        "InvalidImageName",
    ];
    for container in pod
        .status
        .as_ref()?
        .container_statuses
        .as_deref()
        .unwrap_or_default()
    {
        let Some(waiting) = container
            .state
            .as_ref()
            .and_then(|state| state.waiting.as_ref())
        else {
            continue;
        };
        let Some(reason) = waiting.reason.as_deref() else {
            continue;
        };
        if IMAGE_WAITING.contains(&reason) {
            let detail = waiting.message.as_deref().unwrap_or(reason);
            return Some(format!(
                "Kubernetes helper image could not start ({reason}): {detail}"
            ));
        }
    }
    None
}

pub(crate) fn pod_startup_failure(pod: &Pod) -> Option<String> {
    let status = pod.status.as_ref()?;
    if status.phase.as_deref() == Some("Failed") {
        let detail = status
            .message
            .as_deref()
            .or(status.reason.as_deref())
            .unwrap_or("pod entered Failed phase");
        return Some(format!("Kubernetes helper pod failed: {detail}"));
    }
    for condition in status.conditions.as_deref().unwrap_or_default() {
        if condition.type_ == "PodScheduled" && condition.status == "False" {
            let detail = condition
                .message
                .as_deref()
                .or(condition.reason.as_deref())
                .unwrap_or("pod could not be scheduled");
            return Some(format!(
                "Kubernetes helper pod cannot be scheduled: {detail}"
            ));
        }
    }
    const FATAL_WAITING: &[&str] = &[
        "CreateContainerConfigError",
        "CreateContainerError",
        "RunContainerError",
    ];
    for container in status.container_statuses.as_deref().unwrap_or_default() {
        let Some(waiting) = container
            .state
            .as_ref()
            .and_then(|state| state.waiting.as_ref())
        else {
            continue;
        };
        let Some(reason) = waiting.reason.as_deref() else {
            continue;
        };
        if FATAL_WAITING.contains(&reason) {
            let detail = waiting.message.as_deref().unwrap_or(reason);
            return Some(format!(
                "Kubernetes helper container could not start ({reason}): {detail}"
            ));
        }
    }
    None
}
