//! Universal copy engine for cross-backend file transfers.
//! 
//! Supports copying between any combination of:
//! - Local filesystem
//! - Kubernetes PVC/PV
//! - S3 and S3-compatible storage
//! - Google Cloud Storage

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::fs::backend::StorageBackend;
use crate::fs::types::FileEntry;

/// Progress update for copy operations
#[derive(Debug, Clone)]
pub struct CopyProgress {
    pub bytes_copied: u64,
    pub total_bytes: u64,
    pub current_file: String,
    pub files_done: usize,
    pub total_files: usize,
}

/// Copy a file between any two storage backends using streaming
pub async fn copy_file_between_backends(
    source: &dyn StorageBackend,
    source_path: &str,
    dest: &dyn StorageBackend,
    dest_path: &str,
) -> Result<()> {
    // Read from source
    let data = source.read_bytes(source_path).await
        .context("Failed to read from source")?;
    
    // Write to destination
    dest.write_bytes(dest_path, data).await
        .context("Failed to write to destination")?;
    
    Ok(())
}

/// Copy a directory recursively between backends
pub async fn copy_dir_between_backends(
    source: &dyn StorageBackend,
    source_path: &str,
    dest: &dyn StorageBackend,
    dest_path: &str,
    progress_tx: Option<&mpsc::Sender<CopyProgress>>,
) -> Result<()> {
    // Create destination directory
    dest.create_dir(dest_path).await
        .context("Failed to create destination directory")?;
    
    // List source directory
    let entries = source.list_dir(source_path).await
        .context("Failed to list source directory")?;
    
    let total_files = count_files(&entries);
    let mut files_done = 0;
    
    for entry in entries {
        let src = format!("{}/{}", source_path.trim_end_matches('/'), entry.name);
        let dst = format!("{}/{}", dest_path.trim_end_matches('/'), entry.name);
        
        if entry.is_dir {
            // Recursive copy for directories
            Box::pin(copy_dir_between_backends(source, &src, dest, &dst, progress_tx)).await?;
        } else {
            // Copy file
            if let Some(tx) = progress_tx {
                let _ = tx.send(CopyProgress {
                    bytes_copied: 0,
                    total_bytes: entry.size,
                    current_file: entry.name.clone(),
                    files_done,
                    total_files,
                }).await;
            }
            
            copy_file_between_backends(source, &src, dest, &dst).await?;
            files_done += 1;
        }
    }
    
    Ok(())
}

/// Count total files in entry list (non-recursive estimate)
fn count_files(entries: &[FileEntry]) -> usize {
    entries.iter().filter(|e| !e.is_dir).count()
}

/// Copy between backends, auto-detecting if source is file or directory
pub async fn copy_between_backends(
    source: &dyn StorageBackend,
    source_path: &str,
    dest: &dyn StorageBackend,
    dest_path: &str,
    progress_tx: Option<mpsc::Sender<CopyProgress>>,
) -> Result<()> {
    // Check if source is a directory
    if source.is_dir(source_path).await? {
        copy_dir_between_backends(source, source_path, dest, dest_path, progress_tx.as_ref()).await
    } else {
        copy_file_between_backends(source, source_path, dest, dest_path).await
    }
}

