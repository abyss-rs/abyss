use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;

use crate::fs::types::FileEntry;
use crate::fs::backend::{StorageBackend, BackendType, BackendCapabilities};

/// Backend for storage selection menu
/// Presents storage options as "files" in a directory
pub struct SelectingBackend;

#[async_trait]
impl StorageBackend for SelectingBackend {
    async fn list_dir(&self, _path: &str) -> Result<Vec<FileEntry>> {
        Ok(vec![
            FileEntry {
                name: "📁 Local Filesystem".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "💾 Kubernetes PersistentVolumes".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "📀 Kubernetes PersistentVolumeClaims".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "☁ Cloud Storage (S3, GCS)".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
        ])
    }
    
    async fn delete(&self, _path: &str) -> Result<()> {
        Err(anyhow!("Cannot modify selection menu"))
    }
    
    async fn create_dir(&self, _path: &str) -> Result<()> {
        Err(anyhow!("Cannot modify selection menu"))
    }
    
    async fn is_dir(&self, _path: &str) -> Result<bool> {
        Ok(true) // Treat root as directory
    }
    
    async fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<()> {
        Err(anyhow!("Cannot upload to selection menu"))
    }
    
    async fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<()> {
        Err(anyhow!("Cannot download from selection menu"))
    }
    
    async fn read_bytes(&self, _path: &str) -> Result<Vec<u8>> {
        Err(anyhow!("Cannot read from selection menu"))
    }
    
    async fn write_bytes(&self, _path: &str, _data: Vec<u8>) -> Result<()> {
        Err(anyhow!("Cannot write to selection menu"))
    }
    
    fn backend_type(&self) -> BackendType {
        BackendType::Selecting
    }
    
    fn display_path(&self, _path: &str) -> String {
        "Select Storage Type".to_string()
    }
    
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::selecting()
    }
}
