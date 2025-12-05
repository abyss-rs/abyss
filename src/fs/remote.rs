use anyhow::{Context, Result};
use kube::Client;
use std::sync::Arc;

use crate::fs::types::FileEntry;
use crate::k8s::pod::PodManager;

#[derive(Clone)]
pub struct RemoteFs {
    client: Client,
    pod_manager: Arc<PodManager>,
}

impl RemoteFs {
    pub fn new(client: Client) -> Self {
        Self {
            pod_manager: Arc::new(PodManager::new(client.clone())),
            client,
        }
    }

    pub async fn list_dir(&self, namespace: &str, pvc: &str, path: &str) -> Result<Vec<FileEntry>> {
        let pod_name = self
            .pod_manager
            .ensure_pod(namespace, pvc)
            .await
            .context("Failed to create pod for PVC access")?;

        // Busybox ls doesn't support --time-style, use simpler format
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "ls -la {} 2>&1 || echo 'ERROR: Failed to list directory'",
                path
            ),
        ];

        let output = self
            .pod_manager
            .exec_command(namespace, &pod_name, command)
            .await
            .context("Failed to execute ls command")?;

        if output.contains("ERROR:") || output.contains("No such file") {
            return Ok(Vec::new()); // Empty directory or doesn't exist
        }

        let mut entries = Vec::new();

        for line in output.lines().skip(1) {
            // Skip "total" line
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Busybox ls -la output: perms links owner group size month day time name
            if parts.len() < 8 {
                continue;
            }

            // Name is everything after the 8th field (index 7+)
            let name = if parts.len() > 8 {
                parts[8..].join(" ")
            } else if parts.len() == 8 {
                parts[7].to_string()
            } else {
                continue;
            };

            // Skip . and ..
            if name == "." || name == ".." {
                continue;
            }

            let is_dir = parts[0].starts_with('d');
            let size = parts[4].parse::<u64>().unwrap_or(0);

            entries.push(FileEntry {
                name,
                size,
                is_dir,
                modified: None,
                permissions: Some(parts[0].to_string()),
            });
        }

        // Sort: directories first, then by name
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(entries)
    }

    /// Get disk usage for the PVC mount point
    pub async fn get_disk_usage(&self, namespace: &str, pvc: &str) -> Result<String> {
        let pod_name = self
            .pod_manager
            .ensure_pod(namespace, pvc)
            .await
            .context("Failed to create pod for PVC access")?;

        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            "df -h /data 2>/dev/null | tail -1".to_string(),
        ];

        let output = self
            .pod_manager
            .exec_command(namespace, &pod_name, command)
            .await
            .context("Failed to get disk usage")?;

        // Parse df output: Filesystem Size Used Avail Use% Mounted
        let parts: Vec<&str> = output.split_whitespace().collect();
        if parts.len() >= 5 {
            Ok(format!(
                "Size: {} | Used: {} | Avail: {} | {}%",
                parts[1],
                parts[2],
                parts[3],
                parts[4].trim_end_matches('%')
            ))
        } else {
            Ok(output.trim().to_string())
        }
    }

    /// Get directory sizes for ncdu-like analysis
    pub async fn get_directory_sizes(
        &self,
        namespace: &str,
        pvc: &str,
        path: &str,
    ) -> Result<Vec<(String, u64, bool)>> {
        let pod_name = self
            .pod_manager
            .ensure_pod(namespace, pvc)
            .await
            .context("Failed to create pod for PVC access")?;

        // Use du to get sizes, with -s for summary and -b for bytes
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("du -s {}/* 2>/dev/null | sort -rn", path),
        ];

        let output = self
            .pod_manager
            .exec_command(namespace, &pod_name, command)
            .await
            .context("Failed to get directory sizes")?;

        let mut entries = Vec::new();

        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let size_kb = parts[0].parse::<u64>().unwrap_or(0) * 1024; // du returns KB by default
                let full_path = parts[1..].join(" ");
                let name = full_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&full_path)
                    .to_string();

                // Check if it's a directory
                let is_dir = true; // du -s only summarizes, so we assume directories

                entries.push((name, size_kb, is_dir));
            }
        }

        Ok(entries)
    }

    pub async fn delete(&self, namespace: &str, pvc: &str, path: &str) -> Result<()> {
        let pod_name = self.pod_manager.ensure_pod(namespace, pvc).await?;

        let command = vec!["rm".to_string(), "-rf".to_string(), path.to_string()];

        self.pod_manager
            .exec_command(namespace, &pod_name, command)
            .await?;
        Ok(())
    }

    pub async fn create_dir(&self, namespace: &str, pvc: &str, path: &str) -> Result<()> {
        let pod_name = self.pod_manager.ensure_pod(namespace, pvc).await?;

        let command = vec!["mkdir".to_string(), "-p".to_string(), path.to_string()];

        self.pod_manager
            .exec_command(namespace, &pod_name, command)
            .await?;
        Ok(())
    }

    pub async fn copy_to_remote(
        &self,
        namespace: &str,
        pvc: &str,
        local_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<()> {
        let pod_name = self.pod_manager.ensure_pod(namespace, pvc).await?;

        // Create parent directory if needed
        if let Some(parent) = std::path::Path::new(remote_path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                self.create_dir(namespace, pvc, &parent_str).await?;
            }
        }

        self.pod_manager
            .copy_to_pod(namespace, &pod_name, local_path, remote_path)
            .await?;
        Ok(())
    }

    pub async fn copy_from_remote(
        &self,
        namespace: &str,
        pvc: &str,
        remote_path: &str,
        local_path: &std::path::Path,
    ) -> Result<()> {
        let pod_name = self.pod_manager.ensure_pod(namespace, pvc).await?;

        // Create parent directory if needed
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.pod_manager
            .copy_from_pod(namespace, &pod_name, remote_path, local_path)
            .await?;
        Ok(())
    }
}
