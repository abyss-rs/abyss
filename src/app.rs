use anyhow::Result;

use crate::fs::RemoteFs;
use crate::k8s::{K8sClient, StorageManager};
use crate::ui::Pane;

pub enum AppMode {
    Normal,
    SelectStorage,       // Choose between PV, PVC, or Cloud
    SelectNamespace,
    SelectPvc,
    SelectPv,
    SelectCloudProvider, // Choose S3/GCS/etc.
    ConfigureCloud,      // Enter bucket/credentials for cloud storage
    DiskAnalyzer,        // ncdu-like disk usage view
    ConfirmDelete,       // Confirmation dialog for delete
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

#[derive(Clone)]
pub struct DeleteTarget {
    pub backend: std::sync::Arc<dyn crate::fs::StorageBackend>,
    pub path: String,
    pub display_path: String,
    pub is_dir: bool,
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
        
        // Both panes start as local filesystem
        let mut left_pane = Pane::new(home_dir.clone());
        left_pane.is_active = true;
        
        let mut right_pane = Pane::new(home_dir.clone());
        right_pane.is_active = false;

        let mut app = Self {
            left_pane,
            right_pane,
            active_pane: ActivePane::Left,
            message: "Welcome to Abyss - Press Ctrl+N to change pane storage type".to_string(),
            k8s_client,
            storage_manager,
            remote_fs,
            mode: AppMode::Normal,
            namespaces: Vec::new(),
            current_namespace,
            should_quit: false,
            progress: None,
            background_task: None,
            delete_target: None,
        };

        // Load initial directories for both panes
        app.refresh_pane(ActivePane::Left).await?;
        app.refresh_pane(ActivePane::Right).await?;

        Ok(app)
    }

    pub async fn refresh_pane(&mut self, pane_type: ActivePane) -> Result<()> {
        let pane = match pane_type {
            ActivePane::Left => &mut self.left_pane,
            ActivePane::Right => &mut self.right_pane,
        };
        
        match pane.storage.list_dir(&pane.path).await {
            Ok(entries) => {
                pane.entries = entries;
                if pane.state.selected().is_none() && !pane.entries.is_empty() {
                    pane.state.select(Some(0));
                }
            }
            Err(e) => {
                self.message = format!("Error: {}", e);
                // Clear entries on error to indicate issue
                pane.entries.clear();
            }
        }
        Ok(())
    }

    // Helper to refresh the currently active pane
    pub async fn refresh_active_pane(&mut self) -> Result<()> {
        self.refresh_pane(self.active_pane).await
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
                            let _ = self.refresh_pane(ActivePane::Left).await;
                            let _ = self.refresh_pane(ActivePane::Right).await;
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
        let active_pane = self.active_pane;
        let pane = self.active_pane_mut();

        if let Some(entry) = pane.selected_entry() {
            if !entry.is_dir {
                self.message = format!("'{}' is not a directory", entry.name);
                return Ok(());
            }

            let entry_name = entry.name.clone();
            
            // Generic path joining logic
            // Handle root path ("/" or "") vs subdirs
            let separator = if pane.path.ends_with('/') || pane.path.is_empty() {
                "" 
            } else {
                "/"
            };
            
            let new_path = format!("{}{}{}", pane.path, separator, entry_name);
            pane.path = new_path;
            
            // Release borrow
        } else {
            self.message = "No entry selected".to_string();
            return Ok(());
        }

        // Refresh using the new path
        self.refresh_pane(active_pane).await?;
        Ok(())
    }

    pub async fn navigate_up(&mut self) -> Result<()> {
        let active_pane = self.active_pane;
        let pane = self.active_pane_mut();
        
        let parent = if pane.path.len() > 1 {
             // Simple string parent logic for VFS
             // Assumes '/' separator for all backends including Local (adapter handles translation)
             let p = std::path::Path::new(&pane.path);
             p.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "/".to_string())
        } else {
            // Already at root or empty
            "/".to_string()
        };
        
        // Special case: don't go above "/" if already there
        if pane.path == "/" || pane.path.is_empty() {
            self.message = "Already at root".to_string();
            return Ok(());
        }
        
        pane.path = parent;
        
        self.refresh_pane(active_pane).await?;
        Ok(())
    }
}

