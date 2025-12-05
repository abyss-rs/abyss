use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneType {
    Local,
    Remote { namespace: String, pvc: String },
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<DateTime<Utc>>,
    pub permissions: Option<String>,
}

impl FileEntry {
    pub fn format_size(&self) -> String {
        if self.is_dir {
            return "<DIR>".to_string();
        }

        let size = self.size;
        if size < 1024 {
            format!("{} B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

#[derive(Debug, Clone)]
pub enum Operation {
    Copy {
        from_pane: PaneType,
        to_pane: PaneType,
        source_path: String,
        dest_path: String,
    },
    Delete {
        pane: PaneType,
        path: String,
    },
    CreateDir {
        pane: PaneType,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvcInfo {
    pub name: String,
    pub namespace: String,
    pub capacity: String,
    pub access_modes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub name: String,
    pub capacity: String,
    pub access_modes: Vec<String>,
    pub claim_ref: Option<String>,
    pub status: String,
    pub is_pv: bool,
}
