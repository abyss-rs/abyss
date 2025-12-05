use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
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
