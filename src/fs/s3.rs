use anyhow::{Context, Result};
use async_trait::async_trait;
use opendal::{services::S3, Operator};
use std::path::Path;

use crate::fs::backend::{BackendType, S3Provider, StorageBackend};
use crate::fs::types::FileEntry;

/// S3 and S3-compatible storage backend using OpenDAL
pub struct S3Fs {
    operator: Operator,
    bucket: String,
    region: String,
    provider: S3Provider,
}

impl S3Fs {
    /// Create a new S3 backend for AWS with explicit credentials
    pub fn new_aws(bucket: &str, region: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, region, access_key, secret_key, S3Provider::Aws)
    }

    /// Create a new S3 backend using IAM role (EC2/ECS/EKS instance profile)
    /// 
    /// Uses the standard AWS credential chain:
    /// 1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
    /// 2. Shared credentials file (~/.aws/credentials)
    /// 3. EC2 Instance Profile / ECS Task Role / EKS Pod Identity
    /// 
    /// Perfect for running in AWS without hardcoded credentials.
    pub fn new_with_iam(bucket: &str, region: &str) -> Result<Self> {
        let builder = S3::default()
            .bucket(bucket)
            .region(region);
        // Don't set access_key_id/secret_access_key - let OpenDAL auto-detect

        let operator = Operator::new(builder)?
            .finish();

        Ok(Self {
            operator,
            bucket: bucket.to_string(),
            region: region.to_string(),
            provider: S3Provider::Aws,
        })
    }

    /// Create a new S3-compatible backend with explicit credentials
    pub fn new(
        bucket: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
        provider: S3Provider,
    ) -> Result<Self> {
        let mut builder = S3::default()
            .bucket(bucket)
            .region(region)
            .access_key_id(access_key)
            .secret_access_key(secret_key);

        // Set custom endpoint for S3-compatible providers
        if let Some(endpoint) = provider.endpoint(region) {
            builder = builder.endpoint(&endpoint);
        }

        let operator = Operator::new(builder)?
            .finish();

        Ok(Self {
            operator,
            bucket: bucket.to_string(),
            region: region.to_string(),
            provider,
        })
    }

    /// Create backend for DigitalOcean Spaces
    pub fn new_digitalocean(bucket: &str, region: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, region, access_key, secret_key, S3Provider::DigitalOcean)
    }

    /// Create backend for Hetzner Object Storage
    pub fn new_hetzner(bucket: &str, location: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, location, access_key, secret_key, S3Provider::Hetzner)
    }

    /// Create backend for MinIO (local development)
    pub fn new_minio(bucket: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, "us-east-1", access_key, secret_key, S3Provider::MinIO)
    }

    /// Create backend for Cloudflare R2
    pub fn new_cloudflare_r2(bucket: &str, account_id: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, account_id, access_key, secret_key, S3Provider::CloudflareR2)
    }

    /// Create backend for Wasabi
    pub fn new_wasabi(bucket: &str, region: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        Self::new(bucket, region, access_key, secret_key, S3Provider::Wasabi)
    }

    /// Create backend for custom S3-compatible endpoint
    pub fn new_custom(
        bucket: &str,
        region: &str,
        endpoint: &str,
        name: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self> {
        Self::new(
            bucket,
            region,
            access_key,
            secret_key,
            S3Provider::Custom {
                name: name.to_string(),
                endpoint: endpoint.to_string(),
            },
        )
    }
}

#[async_trait]
impl StorageBackend for S3Fs {
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let path = if path.is_empty() || path == "/" { "" } else { path.trim_start_matches('/') };
        let path = if !path.is_empty() && !path.ends_with('/') {
            format!("{}/", path)
        } else {
            path.to_string()
        };

        let entries = self.operator.list(&path).await
            .context("Failed to list S3 directory")?;

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
                // Recursively delete directory contents
                self.operator.remove_all(path).await
                    .context("Failed to delete S3 directory")?;
            } else {
                self.operator.delete(path).await
                    .context("Failed to delete S3 object")?;
            }
        } else {
            // Try deleting as-is
            self.operator.delete(path).await
                .context("Failed to delete S3 object")?;
        }

        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let path = path.trim_start_matches('/');
        let path = if path.ends_with('/') { path.to_string() } else { format!("{}/", path) };
        
        // S3 doesn't have real directories, create a zero-byte object with trailing slash
        self.operator.write(&path, Vec::<u8>::new()).await
            .context("Failed to create S3 directory marker")?;

        Ok(())
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let remote_path = remote_path.trim_start_matches('/');
        
        if local_path.is_dir() {
            // Recursively upload directory - use Box::pin for async recursion
            Box::pin(self.upload_dir(local_path, remote_path)).await?;
        } else {
            let content = tokio::fs::read(local_path).await
                .context("Failed to read local file")?;
            
            self.operator.write(remote_path, content).await
                .context("Failed to upload to S3")?;
        }

        Ok(())
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<()> {
        let remote_path = remote_path.trim_start_matches('/');
        
        // Create parent directory if needed
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = self.operator.read(remote_path).await
            .context("Failed to download from S3")?;
        
        tokio::fs::write(local_path, content.to_vec()).await
            .context("Failed to write local file")?;

        Ok(())
    }

    async fn read_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let path = path.trim_start_matches('/');
        let content = self.operator.read(path).await
            .context("Failed to read from S3")?;
        Ok(content.to_vec())
    }

    async fn write_bytes(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let path = path.trim_start_matches('/');
        self.operator.write(path, data).await
            .context("Failed to write to S3")?;
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
        BackendType::S3 {
            bucket: self.bucket.clone(),
            region: self.region.clone(),
            provider: self.provider.clone(),
        }
    }

    fn display_path(&self, path: &str) -> String {
        format!("s3://{}/{}", self.bucket, path.trim_start_matches('/'))
    }
}

impl S3Fs {
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
