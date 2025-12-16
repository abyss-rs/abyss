//! Universal copy engine for cross-backend file transfers.
//! 
//! Supports copying between any combination of:
//! - Local filesystem
//! - Kubernetes PVC/PV
//! - S3 and S3-compatible storage
//! - Google Cloud Storage

use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::mpsc;

use crate::fs::backend::{StorageBackend, BackendType};
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

/// Copy a single file using direct filesystem operations (for local-to-local)
fn copy_file_local(src: &Path, dst: &Path) -> Result<()> {
    // Create parent directories if needed
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    
    std::fs::copy(src, dst)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), dst.display()))?;
    
    Ok(())
}

/// Copy a directory recursively using direct filesystem operations (for local-to-local)
fn copy_dir_local(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;
    
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))? {
        
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if entry.file_type()?.is_dir() {
            copy_dir_local(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("Failed to copy {} to {}", src_path.display(), dst_path.display()))?;
        }
    }
    
    Ok(())
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
        .with_context(|| format!("Failed to read from source: {}", source_path))?;
    
    // Write to destination
    dest.write_bytes(dest_path, data).await
        .with_context(|| format!("Failed to write to destination: {}", dest_path))?;
    
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
        .with_context(|| format!("Failed to create destination directory: {}", dest_path))?;
    
    // List source directory
    let entries = source.list_dir(source_path).await
        .with_context(|| format!("Failed to list source directory: {}", source_path))?;
    
    let total_files = count_files(&entries);
    let mut files_done = 0;
    
    for entry in entries {
        // Skip .. entries
        if entry.name == ".." {
            continue;
        }
        
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

/// Copy between backends, auto-detecting if source is file or directory.
/// Uses direct filesystem operations for local-to-local copies (much faster).
pub async fn copy_between_backends(
    source: &dyn StorageBackend,
    source_path: &str,
    dest: &dyn StorageBackend,
    dest_path: &str,
    progress_tx: Option<mpsc::Sender<CopyProgress>>,
) -> Result<()> {
    // Optimize: For local-to-local, use direct filesystem operations
    if matches!(source.backend_type(), BackendType::Local) 
        && matches!(dest.backend_type(), BackendType::Local) {
        
        let src_path = Path::new(source_path);
        let dst_path = Path::new(dest_path);
        
        return if src_path.is_dir() {
            copy_dir_local(src_path, dst_path)
        } else {
            copy_file_local(src_path, dst_path)
        };
    }
    
    // For cross-backend copies, use the generic read/write approach
    if source.is_dir(source_path).await? {
        copy_dir_between_backends(source, source_path, dest, dest_path, progress_tx.as_ref()).await
    } else {
        copy_file_between_backends(source, source_path, dest, dest_path).await
    }
}

/// Move between backends, auto-detecting if source is file or directory.
/// Uses std::fs::rename for local-to-local moves when possible (instant for same filesystem).
/// Falls back to copy + delete for cross-filesystem or cross-backend moves.
pub async fn move_between_backends(
    source: &dyn StorageBackend,
    source_path: &str,
    dest: &dyn StorageBackend,
    dest_path: &str,
) -> Result<()> {
    // Optimize: For local-to-local, try rename first (instant for same filesystem)
    if matches!(source.backend_type(), BackendType::Local) 
        && matches!(dest.backend_type(), BackendType::Local) {
        
        let src_path = Path::new(source_path);
        let dst_path = Path::new(dest_path);
        
        // Create parent directory if needed
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        // Try rename first (works if same filesystem)
        match std::fs::rename(src_path, dst_path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                // If rename fails (likely cross-filesystem), fall back to copy+delete
                if e.kind() == std::io::ErrorKind::CrossesDevices 
                   || e.kind() == std::io::ErrorKind::Other {
                    // Copy then delete
                    if src_path.is_dir() {
                        copy_dir_local(src_path, dst_path)?;
                    } else {
                        copy_file_local(src_path, dst_path)?;
                    }
                    
                    // Delete source
                    if src_path.is_dir() {
                        std::fs::remove_dir_all(src_path)
                            .with_context(|| format!("Failed to remove source directory: {}", src_path.display()))?;
                    } else {
                        std::fs::remove_file(src_path)
                            .with_context(|| format!("Failed to remove source file: {}", src_path.display()))?;
                    }
                    return Ok(());
                } else {
                    return Err(e).with_context(|| format!("Failed to move {} to {}", src_path.display(), dst_path.display()));
                }
            }
        }
    }
    
    // Cross-backend move: copy + delete
    copy_between_backends(source, source_path, dest, dest_path, None).await?;
    source.delete(source_path).await?;
    
    Ok(())
}
