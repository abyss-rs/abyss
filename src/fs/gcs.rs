use anyhow::{Context, Result};
use async_trait::async_trait;
use opendal::{services::Gcs, Operator};
use std::path::Path;

use crate::fs::backend::{BackendType, StorageBackend};
use crate::fs::types::FileEntry;

/// Google Cloud Storage backend using OpenDAL
pub struct GcsFs {
    operator: Operator,
    bucket: String,
}

impl GcsFs {
    /// Create a new GCS backend
    /// 
    /// Uses Application Default Credentials if no service account is provided.
    /// Set GOOGLE_APPLICATION_CREDENTIALS env var or provide service account JSON.
    pub fn new(bucket: &str, credential: Option<&str>) -> Result<Self> {
        let mut builder = Gcs::default()
            .bucket(bucket);

        if let Some(cred) = credential {
            builder = builder.credential(cred);
        }

        let operator = Operator::new(builder)?
            .finish();

        Ok(Self {
            operator,
            bucket: bucket.to_string(),
        })
    }

    /// Create GCS backend using GCE/GKE Workload Identity (automatic credentials)
    /// 
    /// Uses the standard Google credential chain:
    /// 1. GOOGLE_APPLICATION_CREDENTIALS env var
    /// 2. Well-known credentials file (~/.config/gcloud)
    /// 3. GCE metadata server (for VMs with service account)
    /// 4. GKE Workload Identity
    /// 
    /// Perfect for running in GCP without hardcoded credentials.
    pub fn new_with_workload_identity(bucket: &str) -> Result<Self> {
        Self::new(bucket, None)
    }

    /// Create GCS backend using service account JSON file
    pub fn from_service_account(bucket: &str, service_account_path: &str) -> Result<Self> {
        let credential = std::fs::read_to_string(service_account_path)
            .context("Failed to read service account file")?;
        
        Self::new(bucket, Some(&credential))
    }
}

#[async_trait]
impl StorageBackend for GcsFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let path = if path.is_empty() || path == "/" { "" } else { path.trim_start_matches('/') };
        let path = if !path.is_empty() && !path.ends_with('/') {
            format!("{}/", path)
        } else {
            path.to_string()
        };

        let entries = self.operator.list(&path).await
            .context("Failed to list GCS directory")?;

        let mut result = Vec::new();

        for entry in entries {
            let name = entry.name().to_string();
            
            // Skip the current directory marker
            if name.is_empty() || name == "/" {
                continue;
            }

            let is_dir = entry.metadata().mode().is_dir();
            let size = entry.metadata().content_length();

            result.push(FileEntry {
                name: name.trim_end_matches('/').to_string(),
                size,
                is_dir,
                modified: None,
                permissions: None,
            });
        }

        // Sort: directories first, then by name
        result.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(result)
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let path = path.trim_start_matches('/');
        
        // Check if it's a directory
        let meta = self.operator.stat(path).await;
        
        if let Ok(meta) = meta {
            if meta.mode().is_dir() {
                self.operator.remove_all(path).await
                    .context("Failed to delete GCS directory")?;
            } else {
                self.operator.delete(path).await
                    .context("Failed to delete GCS object")?;
            }
        } else {
            self.operator.delete(path).await
                .context("Failed to delete GCS object")?;
        }

        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let path = path.trim_start_matches('/');
        let path = if path.ends_with('/') { path.to_string() } else { format!("{}/", path) };
        
        self.operator.write(&path, Vec::<u8>::new()).await
            .context("Failed to create GCS directory marker")?;

        Ok(())
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let remote_path = remote_path.trim_start_matches('/');
        
        if local_path.is_dir() {
            Box::pin(self.upload_dir(local_path, remote_path)).await?;
        } else {
            let content = tokio::fs::read(local_path).await
                .context("Failed to read local file")?;
            
            self.operator.write(remote_path, content).await
                .context("Failed to upload to GCS")?;
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<()> {
        let remote_path = remote_path.trim_start_matches('/');
        
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = self.operator.read(remote_path).await
            .context("Failed to download from GCS")?;
        
        tokio::fs::write(local_path, content.to_vec()).await
            .context("Failed to write local file")?;

        Ok(())
    }

    async fn read_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let path = path.trim_start_matches('/');
        let content = self.operator.read(path).await
            .context("Failed to read from GCS")?;
        Ok(content.to_vec())
    }

    async fn write_bytes(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let path = path.trim_start_matches('/');
        self.operator.write(path, data).await
            .context("Failed to write to GCS")?;
        Ok(())
    }

    async fn is_dir(&self, path: &str) -> Result<bool> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Ok(true);
        }
        
        match self.operator.stat(path).await {
            Ok(meta) => Ok(meta.mode().is_dir()),
            Err(_) => {
                // Try with trailing slash
                let dir_path = if path.ends_with('/') { path.to_string() } else { format!("{}/", path) };
                match self.operator.stat(&dir_path).await {
                    Ok(meta) => Ok(meta.mode().is_dir()),
                    Err(_) => Ok(false),
                }
            }
        }
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Gcs {
            bucket: self.bucket.clone(),
        }
    }

    fn display_path(&self, path: &str) -> String {
        format!("gs://{}/{}", self.bucket, path.trim_start_matches('/'))
    }
}

impl GcsFs {
    fn upload_dir<'a>(&'a self, local_path: &'a Path, remote_path: &'a str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = tokio::fs::read_dir(local_path).await?;
            
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let remote = format!("{}/{}", remote_path.trim_end_matches('/'), name);
                
                if path.is_dir() {
                    self.upload_dir(&path, &remote).await?;
                } else {
                    let content = tokio::fs::read(&path).await?;
                    self.operator.write(&remote, content).await?;
                }
            }
            
            Ok(())
        })
    }
}
