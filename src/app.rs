use anyhow::Result;
use std::path::PathBuf;

use crate::fs::{LocalFs, RemoteFs};
use crate::k8s::{K8sClient, StorageManager};
use crate::ui::Pane;

pub enum AppMode {
    Normal,
    SelectStorage, // Choose between PV and PVC
    SelectNamespace,
    SelectPvc,
    SelectPv,
    DiskAnalyzer,  // ncdu-like disk usage view
    ConfirmDelete, // Confirmation dialog for delete
}

pub struct App {
    pub left_pane: Pane,
    pub right_pane: Pane,
    pub active_pane: ActivePane,
    pub message: String,
    pub k8s_client: K8sClient,
    pub storage_manager: StorageManager,
    pub remote_fs: RemoteFs,
    pub mode: AppMode,
    pub namespaces: Vec<String>,
    pub current_namespace: String,
    pub should_quit: bool,
    // Progress tracking
    pub progress: Option<Progress>,
    // Background task for live progress
    pub background_task: Option<tokio::task::JoinHandle<anyhow::Result<String>>>,
    // Delete confirmation target (full_path, is_local, is_dir)
    pub delete_target: Option<DeleteTarget>,
}

#[derive(Debug, Clone)]
pub struct DeleteTarget {
    pub full_path: String,
    pub display_path: String,
    pub is_local: bool,
    pub is_dir: bool,
    // For remote: namespace, pvc, path_in_pvc
    pub namespace: Option<String>,
    pub pvc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub stage: ProgressStage,
    pub current: u64,
    pub total: u64,
    pub current_file: String,
    pub files_done: usize,
    pub total_files: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProgressStage {
    Counting,
    Archiving,
    Transferring,
    Extracting,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Left,
    Right,
}

impl App {
    pub async fn new() -> Result<Self> {
        let k8s_client = K8sClient::new().await?;
        let current_namespace = k8s_client.current_namespace().to_string();
        let storage_manager = StorageManager::new(k8s_client.client());
        let remote_fs = RemoteFs::new(k8s_client.client());

        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let mut left_pane = Pane::new(home_dir.clone());
        left_pane.is_active = true;

        let right_pane = Pane::new(String::new());

        let mut app = Self {
            left_pane,
            right_pane,
            active_pane: ActivePane::Left,
            message: "Welcome to Abyss - Select storage type in right pane".to_string(),
            k8s_client,
            storage_manager,
            remote_fs,
            mode: AppMode::SelectStorage,
            namespaces: Vec::new(),
            current_namespace,
            should_quit: false,
            progress: None,
            background_task: None,
            delete_target: None,
        };

        // Load initial directory
        app.refresh_left_pane().await?;

        // Load storage options in right pane
        app.right_pane.entries = vec![
            crate::fs::types::FileEntry {
                name: "PersistentVolumes (PV) - Direct access".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
            crate::fs::types::FileEntry {
                name: "PersistentVolumeClaims (PVC) - Namespace scoped".to_string(),
                size: 0,
                is_dir: true,
                modified: None,
                permissions: None,
            },
        ];
        app.right_pane.state.select(Some(0));

        Ok(app)
    }

    pub async fn refresh_left_pane(&mut self) -> Result<()> {
        let path = PathBuf::from(&self.left_pane.path);
        match LocalFs::list_dir(&path) {
            Ok(entries) => {
                self.left_pane.entries = entries;
                if self.left_pane.state.selected().is_none() && !self.left_pane.entries.is_empty() {
                    self.left_pane.state.select(Some(0));
                }
            }
            Err(e) => {
                self.message = format!("Error: {}", e);
            }
        }
        Ok(())
    }

    pub async fn refresh_right_pane(
        &mut self,
        namespace: &str,
        pvc: &str,
        path: &str,
    ) -> Result<()> {
        match self.remote_fs.list_dir(namespace, pvc, path).await {
            Ok(entries) => {
                self.right_pane.entries = entries;
                if self.right_pane.state.selected().is_none() && !self.right_pane.entries.is_empty()
                {
                    self.right_pane.state.select(Some(0));
                }
            }
            Err(e) => {
                self.message = format!("Error: {}", e);
            }
        }
        Ok(())
    }

    pub fn switch_pane(&mut self) {
        self.left_pane.is_active = !self.left_pane.is_active;
        self.right_pane.is_active = !self.right_pane.is_active;

        self.active_pane = if self.left_pane.is_active {
            ActivePane::Left
        } else {
            ActivePane::Right
        };
    }

    pub fn active_pane_mut(&mut self) -> &mut Pane {
        match self.active_pane {
            ActivePane::Left => &mut self.left_pane,
            ActivePane::Right => &mut self.right_pane,
        }
    }

    pub fn active_pane(&self) -> &Pane {
        match self.active_pane {
            ActivePane::Left => &self.left_pane,
            ActivePane::Right => &self.right_pane,
        }
    }

    /// Poll background task for completion (non-blocking)
    pub async fn poll_background_task(&mut self) {
        if let Some(ref mut handle) = self.background_task {
            // Check if task is finished without blocking
            if handle.is_finished() {
                // Take ownership of the handle
                if let Some(handle) = self.background_task.take() {
                    match handle.await {
                        Ok(Ok(msg)) => {
                            self.message = msg;
                            self.progress = None;
                            // Refresh both panes to show new files
                            let _ = self.refresh_left_pane().await;
                            // Also try to refresh right pane if we have a valid path
                            if !self.right_pane.path.is_empty() {
                                let parts: Vec<&str> = self.right_pane.path.split('/').collect();
                                if parts.len() >= 2 {
                                    let ns = parts[0].to_string();
                                    let pvc = parts[1].to_string();
                                    let dir = if parts.len() > 2 {
                                        format!("/{}", parts[2..].join("/"))
                                    } else {
                                        "/data".to_string()
                                    };
                                    let _ = self.refresh_right_pane(&ns, &pvc, &dir).await;
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            self.message = format!("✗ Error: {}", e);
                            self.progress = None;
                        }
                        Err(e) => {
                            self.message = format!("✗ Task failed: {}", e);
                            self.progress = None;
                        }
                    }
                }
            }
        }
    }

    pub async fn navigate_into(&mut self) -> Result<()> {
        let pane = self.active_pane_mut();

        if let Some(entry) = pane.selected_entry() {
            if !entry.is_dir {
                self.message = format!("'{}' is not a directory", entry.name);
                return Ok(());
            }

            let entry_name = entry.name.clone();

            match self.active_pane {
                ActivePane::Left => {
                    let new_path = PathBuf::from(&self.left_pane.path).join(&entry_name);
                    self.left_pane.path = new_path.to_string_lossy().to_string();
                    self.refresh_left_pane().await?;
                }
                ActivePane::Right => {
                    // Navigate into directory in remote pane
                    let current_path = self.right_pane.path.clone();

                    if current_path.is_empty() {
                        self.message = "No PVC selected".to_string();
                        return Ok(());
                    }

                    // Parse namespace/pvc from path (format: namespace/pvc/data/...)
                    let parts: Vec<&str> = current_path.split('/').collect();
                    if parts.len() >= 3 {
                        let namespace = parts[0].to_string();
                        let pvc = parts[1].to_string();
                        // Current path inside PVC (e.g., "data" or "data/subdir")
                        let path_in_pvc = format!("/{}", parts[2..].join("/"));

                        let new_path = format!("{}/{}", path_in_pvc, entry_name);

                        // Update path
                        self.right_pane.path = format!("{}/{}{}", namespace, pvc, new_path);
                        self.message = format!("Navigating to: {}", new_path);

                        // Refresh
                        self.refresh_right_pane(&namespace, &pvc, &new_path).await?;
                    } else {
                        self.message = format!(
                            "Invalid path format: {} (parts: {})",
                            current_path,
                            parts.len()
                        );
                    }
                }
            }
        } else {
            self.message = "No entry selected".to_string();
        }

        Ok(())
    }

    pub async fn navigate_up(&mut self) -> Result<()> {
        match self.active_pane {
            ActivePane::Left => {
                let path = PathBuf::from(&self.left_pane.path);
                if let Some(parent) = path.parent() {
                    self.left_pane.path = parent.to_string_lossy().to_string();
                    self.refresh_left_pane().await?;
                }
            }
            ActivePane::Right => {
                // Navigate up in remote pane
                let current_path = self.right_pane.path.clone();

                if current_path.is_empty() {
                    // Already at storage selection, do nothing
                    return Ok(());
                }

                let parts: Vec<&str> = current_path.split('/').collect();
                if parts.len() >= 3 {
                    let namespace = parts[0].to_string();
                    let pvc = parts[1].to_string();
                    let path_in_pvc = format!("/{}", parts[2..].join("/"));

                    // If at /data, go back to storage selection
                    if path_in_pvc == "/data" {
                        // Return to storage type selection
                        self.mode = AppMode::SelectStorage;
                        self.right_pane.path = String::new();
                        self.right_pane.entries = vec![
                            crate::fs::types::FileEntry {
                                name: "PersistentVolumes (PV) - Direct access".to_string(),
                                size: 0,
                                is_dir: true,
                                modified: None,
                                permissions: None,
                            },
                            crate::fs::types::FileEntry {
                                name: "PersistentVolumeClaims (PVC) - Namespace scoped".to_string(),
                                size: 0,
                                is_dir: true,
                                modified: None,
                                permissions: None,
                            },
                        ];
                        self.right_pane.state.select(Some(0));
                        self.message = "Select storage type".to_string();
                    } else {
                        // Go to parent directory
                        let path = std::path::Path::new(&path_in_pvc);
                        if let Some(parent) = path.parent() {
                            let parent_str = parent.to_string_lossy();
                            let new_path = if parent_str.is_empty() || parent_str == "/" {
                                "/data".to_string()
                            } else {
                                parent_str.to_string()
                            };

                            // Update path
                            self.right_pane.path = format!("{}/{}{}", namespace, pvc, new_path);
                            self.message = format!("Navigating to: {}", new_path);

                            // Refresh
                            self.refresh_right_pane(&namespace, &pvc, &new_path).await?;
                        }
                    }
                } else {
                    // Path doesn't look valid, return to storage selection
                    self.mode = AppMode::SelectStorage;
                    self.right_pane.path = String::new();
                    self.message = "Returned to storage selection".to_string();
                }
            }
        }

        Ok(())
    }
}
