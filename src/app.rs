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
    Rename,              // Rename file/directory
    ViewFile,            // View file contents
    Search,              // Search for files
    EditFile,            // Edit file contents (nano-like)
    ConfirmLargeLoad,    // Confirm loading large remote file
    EditorSearch,        // Search text inside editor
}

#[derive(Debug, Clone, PartialEq)]
pub enum LargeFileAction {
    View,
    Edit,
}

/// Text input state for rename/search operations.
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    /// Current input text.
    pub value: String,
    /// Cursor position in the text.
    pub cursor: usize,
    /// Original value (for rename - to know what to rename from).
    pub original: String,
}

impl TextInput {
    pub fn new(initial: &str) -> Self {
        Self {
            value: initial.to_string(),
            cursor: initial.len(),
            original: initial.to_string(),
        }
    }
    
    pub fn insert(&mut self, c: char) {
        self.value.insert(self.cursor, c);
        self.cursor += 1;
    }
    
    pub fn delete_back(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.value.remove(self.cursor);
        }
    }
    
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }
    
    pub fn move_right(&mut self) {
        if self.cursor < self.value.len() {
            self.cursor += 1;
        }
    }
    
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }
}

pub struct App {
    pub left_pane: Pane,
    pub right_pane: Pane,
    pub active_pane: ActivePane,
    pub message: String,
    pub k8s_client: Option<K8sClient>,
    pub storage_manager: Option<StorageManager>,
    pub remote_fs: Option<RemoteFs>,
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
    // Sync state
    pub sync_enabled: bool,
    pub sync_status: SyncStatus,
    // Sync background task and progress receiver
    pub sync_task: Option<tokio::task::JoinHandle<anyhow::Result<crate::sync::SyncResult>>>,
    pub sync_progress_rx: Option<tokio::sync::mpsc::Receiver<crate::sync::SyncProgress>>,
    // Text input for rename/search
    pub text_input: TextInput,
    // File viewer content
    pub view_content: Vec<String>,
    pub view_scroll: usize,
    // File editor
    pub editor: TextEditor,
    
    // Large file handling
    pub pending_large_action: Option<LargeFileAction>,
    pub view_file_offset: u64,
    pub view_file_path: String,
    pub view_file_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TextEditor {
    pub content: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub filename: String,
    pub modified: bool,
    pub cut_buffer: Option<String>,
}

impl TextEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_char(&mut self, c: char) {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        let line = &mut self.content[self.cursor_row];
        if self.cursor_col >= line.len() {
            line.push(c);
            self.cursor_col = line.len();
        } else {
            line.insert(self.cursor_col, c);
            self.cursor_col += 1;
        }
        self.modified = true;
    }

    pub fn insert_newline(&mut self) {
        if self.content.is_empty() {
            self.content.push(String::new());
            self.content.push(String::new());
            self.cursor_row = 1;
            self.cursor_col = 0;
            self.modified = true;
            return;
        }
        
        let line = &mut self.content[self.cursor_row];
        let new_line = if self.cursor_col < line.len() {
            let tail = line.split_off(self.cursor_col);
            tail
        } else {
            String::new()
        };
        
        self.content.insert(self.cursor_row + 1, new_line);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.modified = true;
    }

    pub fn delete_back(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.content[self.cursor_row];
            line.remove(self.cursor_col - 1);
            self.cursor_col -= 1;
            self.modified = true;
        } else if self.cursor_row > 0 {
            let line = self.content.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_line = &mut self.content[self.cursor_row];
            self.cursor_col = prev_line.len();
            prev_line.push_str(&line);
            self.modified = true;
        }
    }

    pub fn cut_line(&mut self) {
        if self.content.is_empty() {
            return;
        }
        
        // If buffer is empty, cut the whole line.
        // If we want Nano behavior:
        // - Ctrl+K cuts entire line. 
        // - Multiple Ctrl+K appends to buffer? Nano does append if you don't move cursor.
        // For simplicity, let's just implement single line cut for now, replacing buffer.
        
        if self.cursor_row < self.content.len() {
            let line = self.content.remove(self.cursor_row);
            self.cut_buffer = Some(line);
            self.modified = true;
            
            // If we removed the last line and it's now empty, ensure there's at least one line
            if self.content.is_empty() {
                self.content.push(String::new());
            } else if self.cursor_row >= self.content.len() {
                self.cursor_row = self.content.len() - 1;
            }
            self.cursor_col = 0;
        }
    }

    pub fn uncut_line(&mut self) {
        if let Some(ref line) = self.cut_buffer {
            if self.content.is_empty() {
                 self.content.push(line.clone());
            } else {
                 self.content.insert(self.cursor_row, line.clone());
                 self.cursor_row += 1;
            }
            self.modified = true;
        }
    }
}

/// Current sync status display.
#[derive(Debug, Clone, Default)]
pub enum SyncStatus {
    #[default]
    Disabled,
    Idle,
    Scanning,
    Syncing { current_file: String, progress: f32 },
    Complete { files_synced: usize },
    Error { message: String },
}

impl SyncStatus {
    pub fn display(&self) -> String {
        match self {
            Self::Disabled => "Sync: Off".to_string(),
            Self::Idle => "Sync: Idle".to_string(),
            Self::Scanning => "Sync: Scanning...".to_string(),
            Self::Syncing { current_file, progress } => {
                format!("Sync: {} ({:.0}%)", current_file, progress * 100.0)
            }
            Self::Complete { files_synced } => format!("Sync: Done ({} files)", files_synced),
            Self::Error { message } => format!("Sync Error: {}", message),
        }
    }
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
        // Try to initialize K8s, but don't fail if unavailable
        let (k8s_client, storage_manager, remote_fs, current_namespace, k8s_message) = 
            match K8sClient::new().await {
                Ok(client) => {
                    let namespace = client.current_namespace().to_string();
                    let storage_mgr = StorageManager::new(client.client());
                    let remote = RemoteFs::new(client.client());
                    (Some(client), Some(storage_mgr), Some(remote), namespace, None)
                }
                Err(e) => {
                    // K8s not available - app will work without it
                    (None, None, None, "default".to_string(), Some(format!("K8s unavailable: {}", e)))
                }
            };

        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        
        // Both panes start as local filesystem
        let mut left_pane = Pane::new(home_dir.clone());
        left_pane.is_active = true;
        
        let mut right_pane = Pane::new(home_dir.clone());
        right_pane.is_active = false;

        let welcome_msg = if let Some(k8s_err) = k8s_message {
            format!("Welcome to Abyss ({})", k8s_err)
        } else {
            "Welcome to Abyss - Press Ctrl+N to change pane storage type".to_string()
        };

        let mut app = Self {
            left_pane,
            right_pane,
            active_pane: ActivePane::Left,
            message: welcome_msg,
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
            sync_enabled: false,
            sync_status: SyncStatus::Disabled,
            sync_task: None,
            sync_progress_rx: None,
            text_input: TextInput::default(),
            view_content: Vec::new(),
            view_scroll: 0,
            editor: TextEditor::default(),
            pending_large_action: None,
            view_file_offset: 0,
            view_file_path: String::new(),
            view_file_size: 0,
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
            Ok(mut entries) => {
                // Add ".." entry at top if not at root
                let is_root = pane.path.is_empty() || pane.path == "/" || pane.path == ".";
                if !is_root {
                    entries.insert(0, crate::fs::types::FileEntry {
                        name: "..".to_string(),
                        size: 0,
                        is_dir: true,
                        modified: None,
                        permissions: None,
                    });
                }
                
                pane.entries = entries;
                // Always reset cursor to first entry when directory changes
                if !pane.entries.is_empty() {
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
            
            // Handle ".." to navigate up
            if entry_name == ".." {
                return self.navigate_up().await;
            }
            
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

