use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::fs::types::FileEntry;

/// Backend type information for display and identification
#[derive(Debug, Clone, PartialEq)]
pub enum BackendType {
    Local,
    Kubernetes { namespace: String, pvc: String },
    S3 { bucket: String, region: String, provider: S3Provider },
    Gcs { bucket: String },
    Selecting,
}

impl BackendType {
    /// Get a short display name for the backend
    pub fn short_name(&self) -> &'static str {
        match self {
            BackendType::Local => "Local",
            BackendType::Kubernetes { .. } => "K8s",
            BackendType::S3 { .. } => "S3",
            BackendType::Gcs { .. } => "GCS",
            BackendType::Selecting => "Select",
        }
    }
}

/// S3-compatible storage providers
#[derive(Debug, Clone, PartialEq)]
pub enum S3Provider {
    Aws,
    DigitalOcean,
    Hetzner,
    MinIO,
    CloudflareR2,
    Wasabi,
    Custom { name: String, endpoint: String },
}

impl S3Provider {
    /// Get the endpoint URL for this provider
    pub fn endpoint(&self, region: &str) -> Option<String> {
        match self {
            S3Provider::Aws => None, // Use default AWS endpoint
            S3Provider::DigitalOcean => Some(format!("https://{}.digitaloceanspaces.com", region)),
            S3Provider::Hetzner => Some(format!("https://{}.your-objectstorage.com", region)),
            S3Provider::MinIO => Some("http://localhost:9000".to_string()),
            S3Provider::CloudflareR2 => Some(format!("https://{}.r2.cloudflarestorage.com", region)),
            S3Provider::Wasabi => Some(format!("https://s3.{}.wasabisys.com", region)),
            S3Provider::Custom { endpoint, .. } => Some(endpoint.clone()),
        }
    }

    /// Get display name for the provider
    pub fn display_name(&self) -> &str {
        match self {
            S3Provider::Aws => "AWS S3",
            S3Provider::DigitalOcean => "DigitalOcean Spaces",
            S3Provider::Hetzner => "Hetzner Object Storage",
            S3Provider::MinIO => "MinIO",
            S3Provider::CloudflareR2 => "Cloudflare R2",
            S3Provider::Wasabi => "Wasabi",
            S3Provider::Custom { name, .. } => name,
        }
    }
}

/// Backend capability flags
#[derive(Debug, Clone, Default)]
pub struct BackendCapabilities {
    /// Supports streaming read/write (vs full download)
    pub streaming: bool,
    /// Supports file append operations
    pub append: bool,
    /// Supports Unix-style permissions
    pub permissions: bool,
    /// Supports symbolic links
    pub symlinks: bool,
    /// Supports rename/move operations
    pub rename: bool,
    /// Supports file system stat (size, mtime, etc.)
    pub stat: bool,
    /// Supports hard links
    pub hardlinks: bool,
    /// Supports extended attributes
    pub xattr: bool,
}

impl BackendCapabilities {
    /// Local filesystem capabilities
    pub fn local() -> Self {
        Self {
            streaming: true,
            append: true,
            permissions: true,
            symlinks: true,
            rename: true,
            stat: true,
            hardlinks: true,
            xattr: true,
        }
    }
    
    /// Cloud storage (S3/GCS) capabilities
    pub fn cloud() -> Self {
        Self {
            streaming: true,
            append: false,
            permissions: false,
            symlinks: false,
            rename: true, // Via copy + delete
            stat: true,
            hardlinks: false,
            xattr: false,
        }
    }
    
    /// Kubernetes PVC capabilities
    pub fn kubernetes() -> Self {
        Self {
            streaming: false, // Via tar
            append: true,
            permissions: true,
            symlinks: true,
            rename: true,
            stat: true,
            hardlinks: true,
            xattr: false,
        }
    }

    /// Selecting backend capabilities (readonly)
    pub fn selecting() -> Self {
        Self {
            streaming: false,
            append: false,
            permissions: false,
            symlinks: false,
            rename: false,
            stat: false,
            hardlinks: false,
            xattr: false,
        }
    }
}

/// File metadata from stat operation
#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
    pub permissions: Option<String>,
}

/// Unified storage backend trait for all storage providers
#[async_trait]
pub trait StorageBackend: Send + Sync {
    // ========== Core Operations ==========
    
    /// List directory contents
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>>;
    
    /// Delete a file or directory
    async fn delete(&self, path: &str) -> Result<()>;
    
    /// Create a directory
    async fn create_dir(&self, path: &str) -> Result<()>;
    
    /// Check if a path is a directory
    async fn is_dir(&self, path: &str) -> Result<bool>;
    
    // ========== File Transfer ==========
    
    /// Upload a file from local filesystem
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<()>;
    
    /// Download a file to local filesystem
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<()>;
    
    /// Read file contents as bytes (for cross-backend copy)
    async fn read_bytes(&self, path: &str) -> Result<Vec<u8>>;
    
    /// Write bytes to a file (for cross-backend copy)
    async fn write_bytes(&self, path: &str, data: Vec<u8>) -> Result<()>;

    /// Read a range of bytes from a file (for streaming/large files)
    async fn read_range(&self, path: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
        // Default impl reads everything and slices (inefficient, override for performance)
        let all = self.read_bytes(path).await?;
        let start = offset as usize;
        let end = (offset + length) as usize;
        if start >= all.len() {
            return Ok(Vec::new());
        }
        Ok(all[start..end.min(all.len())].to_vec())
    }
    
    // ========== Metadata ==========
    
    /// Get file/directory metadata
    async fn stat(&self, path: &str) -> Result<FileStat> {
        // Default implementation using list_dir
        let parent = if path.contains('/') {
            path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
        } else {
            ""
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        
        let entries = self.list_dir(parent).await?;
        entries.into_iter()
            .find(|e| e.name == name)
            .map(|e| FileStat {
                size: e.size,
                is_dir: e.is_dir,
                modified: e.modified,
                created: None,
                permissions: e.permissions,
            })
            .ok_or_else(|| anyhow::anyhow!("Path not found: {}", path))
    }
    
    /// Rename/move a file or directory
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        // Default implementation: copy + delete
        let data = self.read_bytes(from).await?;
        self.write_bytes(to, data).await?;
        self.delete(from).await?;
        Ok(())
    }
    
    /// Get disk usage info (if supported)
    async fn get_disk_usage(&self) -> Result<Option<String>> {
        Ok(None)
    }
    
    // ========== Backend Info ==========
    
    /// Get the backend type
    fn backend_type(&self) -> BackendType;
    
    /// Get backend capabilities
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }
    
    /// Get display path for the current location
    fn display_path(&self, path: &str) -> String {
        path.to_string()
    }
    
    /// Get the root path for this backend
    fn root_path(&self) -> &str {
        "/"
    }

    /// Check if backend is local filesystem
    fn is_local(&self) -> bool {
        matches!(self.backend_type(), BackendType::Local)
    }
}
