use anyhow::{anyhow, Result};
use k8s_openapi::api::core::v1::{
    Container, PersistentVolumeClaimVolumeSource, Pod, PodSpec, Volume, VolumeMount,
};
use kube::api::{AttachParams, DeleteParams, PostParams};
use kube::{Api, Client};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PodManager {
    client: Client,
    pods: Arc<Mutex<HashMap<String, String>>>, // key: "namespace/pvc", value: pod_name
}

impl PodManager {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            pods: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn ensure_pod(&self, namespace: &str, pvc: &str) -> Result<String> {
        let key = format!("{}/{}", namespace, pvc);

        let mut pods = self.pods.lock().await;

        if let Some(pod_name) = pods.get(&key) {
            // Check if pod still exists
            let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
            if api.get(pod_name).await.is_ok() {
                return Ok(pod_name.clone());
            }
        }

        // Create new pod
        let pod_name = format!("abyss-{}-{}", pvc, chrono::Utc::now().timestamp());
        self.create_pod(namespace, pvc, &pod_name).await?;

        pods.insert(key, pod_name.clone());

        Ok(pod_name)
    }

    async fn create_pod(&self, namespace: &str, pvc: &str, pod_name: &str) -> Result<()> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some(pod_name.to_string()),
                labels: Some({
                    let mut labels = BTreeMap::new();
                    labels.insert("app".to_string(), "abyss".to_string());
                    labels
                }),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "helper".to_string(),
                    image: Some("busybox:latest".to_string()),
                    command: Some(vec![
                        "sh".to_string(),
                        "-c".to_string(),
                        "sleep infinity".to_string(),
                    ]),
                    volume_mounts: Some(vec![VolumeMount {
                        name: "data".to_string(),
                        mount_path: "/data".to_string(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    name: "data".to_string(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: pvc.to_string(),
                        read_only: Some(false),
                    }),
                    ..Default::default()
                }]),
                restart_policy: Some("Never".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        api.create(&PostParams::default(), &pod).await?;

        // Wait for pod to be running
        self.wait_for_pod_running(namespace, pod_name).await?;

        Ok(())
    }

    async fn wait_for_pod_running(&self, namespace: &str, pod_name: &str) -> Result<()> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        for _ in 0..60 {
            let pod = api.get(pod_name).await?;

            if let Some(status) = pod.status {
                if let Some(phase) = status.phase {
                    if phase == "Running" {
                        return Ok(());
                    }
                    if phase == "Failed" || phase == "Unknown" {
                        return Err(anyhow!("Pod failed to start: {}", phase));
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Err(anyhow!("Timeout waiting for pod to start"))
    }

    pub async fn exec_command(
        &self,
        namespace: &str,
        pod_name: &str,
        command: Vec<String>,
    ) -> Result<String> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        let attach_params = AttachParams {
            container: Some("helper".to_string()),
            stdout: true,
            stderr: true,
            stdin: false,
            tty: false,
            ..Default::default()
        };

        let mut attached = api.exec(pod_name, command, &attach_params).await?;

        let mut output = Vec::new();

        // Read stdout
        if let Some(mut stdout) = attached.stdout() {
            use tokio::io::AsyncReadExt;
            stdout.read_to_end(&mut output).await?;
        }

        Ok(String::from_utf8_lossy(&output).to_string())
    }

    pub async fn copy_to_pod(
        &self,
        namespace: &str,
        pod_name: &str,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<()> {
        // For simplicity, we'll use tar to copy files
        // Create tar archive
        let tar_data = self.create_tar_archive(local_path)?;

        // Extract the parent directory from remote_path
        let remote_dir = if remote_path.starts_with("/data/") {
            Path::new(remote_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/data".to_string())
        } else {
            "/data".to_string()
        };

        // Write tar data to pod and extract
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        // Use stdin to send tar data
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "cat > /tmp/upload.tar && tar -xf /tmp/upload.tar -C {} && rm /tmp/upload.tar",
                remote_dir
            ),
        ];

        let attach_params = AttachParams {
            container: Some("helper".to_string()),
            stdout: true,
            stderr: true,
            stdin: true,
            tty: false,
            ..Default::default()
        };

        let mut attached = api.exec(pod_name, command, &attach_params).await?;

        // Send tar data
        if let Some(mut stdin) = attached.stdin() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&tar_data).await?;
            stdin.shutdown().await?;
        }

        // Wait for completion
        attached.join().await?;

        Ok(())
    }

    pub async fn copy_from_pod(
        &self,
        namespace: &str,
        pod_name: &str,
        remote_path: &str,
        local_path: &Path,
    ) -> Result<()> {
        // Create tar on remote and stream it back
        let command = vec![
            "tar".to_string(),
            "-cf".to_string(),
            "-".to_string(),
            "-C".to_string(),
            Path::new(remote_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/data".to_string()),
            Path::new(remote_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string()),
        ];

        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        let attach_params = AttachParams {
            container: Some("helper".to_string()),
            stdout: true,
            stderr: true,
            stdin: false,
            tty: false,
            ..Default::default()
        };

        let mut attached = api.exec(pod_name, command, &attach_params).await?;

        let mut tar_data = Vec::new();

        if let Some(mut stdout) = attached.stdout() {
            use tokio::io::AsyncReadExt;
            stdout.read_to_end(&mut tar_data).await?;
        }

        // Extract tar to local path
        self.extract_tar_archive(&tar_data, local_path.parent().unwrap_or(Path::new(".")))?;

        Ok(())
    }

    fn create_tar_archive(&self, path: &Path) -> Result<Vec<u8>> {
        let mut tar_data = Vec::new();
        let mut archive = tar::Builder::new(&mut tar_data);

        if path.is_dir() {
            archive.append_dir_all(path.file_name().unwrap_or(path.as_os_str()), path)?;
        } else {
            let mut file = std::fs::File::open(path)?;
            archive.append_file(path.file_name().unwrap_or(path.as_os_str()), &mut file)?;
        }

        archive.finish()?;
        drop(archive);

        Ok(tar_data)
    }

    fn extract_tar_archive(&self, data: &[u8], dest: &Path) -> Result<()> {
        let mut archive = tar::Archive::new(data);
        archive.unpack(dest)?;
        Ok(())
    }

    pub async fn cleanup_pod(&self, namespace: &str, pod_name: &str) -> Result<()> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        api.delete(pod_name, &DeleteParams::default()).await?;
        Ok(())
    }
}
