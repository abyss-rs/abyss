use anyhow::{Context, Result};
use chrono::DateTime;
use std::fs;
use std::path::{Path, PathBuf};

use crate::fs::types::FileEntry;

pub struct LocalFs;

impl LocalFs {
    pub fn list_dir(path: &Path) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();

        let read_dir = fs::read_dir(path)
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;

        for entry in read_dir {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files starting with .
            if name.starts_with('.') {
                continue;
            }

            let modified = metadata.modified().ok().and_then(|t| {
                DateTime::from_timestamp(
                    t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64,
                    0,
                )
            });

            entries.push(FileEntry {
                name,
                size: metadata.len(),
                is_dir: metadata.is_dir(),
                modified,
                permissions: None,
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

    pub fn delete(path: &Path) -> Result<()> {
        if path.is_dir() {
            fs::remove_dir_all(path)
                .with_context(|| format!("Failed to delete directory: {}", path.display()))?;
        } else {
            fs::remove_file(path)
                .with_context(|| format!("Failed to delete file: {}", path.display()))?;
        }
        Ok(())
    }

    pub fn create_dir(path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        Ok(())
    }

    pub fn copy_file(from: &Path, to: &Path) -> Result<()> {
        if from.is_dir() {
            Self::copy_dir_recursive(from, to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(from, to).with_context(|| {
                format!("Failed to copy {} to {}", from.display(), to.display())
            })?;
        }
        Ok(())
    }

    fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
        fs::create_dir_all(to)?;

        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let from_path = entry.path();
            let to_path = to.join(entry.file_name());

            if file_type.is_dir() {
                Self::copy_dir_recursive(&from_path, &to_path)?;
            } else {
                fs::copy(&from_path, &to_path)?;
            }
        }

        Ok(())
    }

    pub fn normalize_path(path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                _ => normalized.push(component),
            }
        }

        if normalized.as_os_str().is_empty() {
            normalized.push("/");
        }

        normalized
    }
}

/// Local filesystem backend adapter
pub struct LocalBackend {
    pub root: PathBuf,
}

impl LocalBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    
    fn full_path(&self, path: &str) -> PathBuf {
        if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        }
    }
}

#[async_trait::async_trait]
impl crate::fs::backend::StorageBackend for LocalBackend {
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        LocalFs::list_dir(&self.full_path(path))
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        LocalFs::delete(&self.full_path(path))
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        tokio::fs::create_dir_all(self.full_path(path)).await?;
        Ok(())
    }
    
    async fn is_dir(&self, path: &str) -> Result<bool> {
        let full = self.full_path(path);
        Ok(full.is_dir())
    }
    
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let dest = self.full_path(remote_path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Use copy_file_recursive logic or standard copy
        // For local-to-local uploads, a simple recursive copy is best
        LocalFs::copy_file(local_path, &dest)?;
        Ok(())
    }
    
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<()> {
        let src = self.full_path(remote_path);
        // For local-to-local downloads, same recursive copy
        LocalFs::copy_file(&src, local_path)?;
        Ok(())
    }
    
    async fn read_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let data = tokio::fs::read(self.full_path(path)).await
            .context("Failed to read local file")?;
        Ok(data)
    }
    
    async fn read_range(&self, path: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let mut file = tokio::fs::File::open(self.full_path(path)).await
            .context("Failed to open local file")?;
        
        file.seek(std::io::SeekFrom::Start(offset)).await
            .context("Failed to seek file")?;
            
        let mut buffer = vec![0u8; length as usize];
        // Read exact or until EOF
        let mut read = 0;
        while read < buffer.len() {
             let n = file.read(&mut buffer[read..]).await?;
             if n == 0 { break; }
             read += n;
        }
        buffer.truncate(read);
        Ok(buffer)
    }
    
    async fn write_bytes(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let dest = self.full_path(path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(dest, data).await
            .context("Failed to write local file")?;
        Ok(())
    }
    
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let from_path = self.full_path(from);
        let to_path = self.full_path(to);
        tokio::fs::rename(from_path, to_path).await
            .context("Failed to rename")?;
        Ok(())
    }
    
    fn backend_type(&self) -> crate::fs::backend::BackendType {
        crate::fs::backend::BackendType::Local
    }
    
    fn capabilities(&self) -> crate::fs::backend::BackendCapabilities {
        crate::fs::backend::BackendCapabilities::local()
    }
    
    fn root_path(&self) -> &str {
        self.root.to_str().unwrap_or("/")
    }

    fn display_path(&self, path: &str) -> String {
        self.full_path(path).to_string_lossy().to_string()
    }
}
