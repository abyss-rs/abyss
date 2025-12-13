use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, AppMode};

pub async fn handle_events(app: &mut App) -> Result<()> {
    // Poll for sync progress updates (non-blocking)
    let _ = poll_sync_progress(app).await;
    
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            match app.mode {
                AppMode::Normal => handle_normal_mode(app, key).await?,
                AppMode::SelectStorage => handle_storage_select(app, key).await?,
                AppMode::SelectNamespace => handle_namespace_select(app, key).await?,
                AppMode::SelectPvc => handle_pvc_select(app, key).await?,
                AppMode::SelectPv => handle_pv_select(app, key).await?,
                AppMode::SelectCloudProvider => handle_cloud_provider_select(app, key).await?,
                AppMode::ConfigureCloud => handle_configure_cloud(app, key).await?,
                AppMode::DiskAnalyzer => handle_disk_analyzer(app, key).await?,
                AppMode::ConfirmDelete => handle_confirm_delete(app, key).await?,
            }
        }
    }
    Ok(())
}

async fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Tab => {
            app.switch_pane();
        }
        KeyCode::Up => {
            app.active_pane_mut().select_previous();
        }
        KeyCode::Down => {
            app.active_pane_mut().select_next();
        }
        KeyCode::Enter => {
            app.navigate_into().await?;
        }
        KeyCode::Backspace => {
            app.navigate_up().await?;
        }
        KeyCode::F(5) => {
            // Copy operation
            handle_copy(app).await?;
        }
        KeyCode::F(6) => {
            // Move operation
            handle_move(app).await?;
        }
        KeyCode::F(2) => {
            // Show disk usage for current PVC
            handle_disk_usage(app).await?;
        }
        KeyCode::F(3) => {
            // ncdu-like disk analyzer
            handle_disk_analyzer_enter(app).await?;
        }
        KeyCode::F(7) => {
            // Create directory
            handle_mkdir(app).await?;
        }
        KeyCode::F(8) => {
            // Delete
            handle_delete(app).await?;
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Toggle sync mode
            handle_sync_toggle(app)?;
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Manual one-time sync
            handle_sync_now(app).await?;
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Dry-run sync (preview changes)
            handle_sync_dry_run(app).await?;
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Select storage type for the ACTIVE pane
            app.mode = AppMode::SelectStorage;

            // Build storage options list
            let storage_options = vec![
                crate::fs::types::FileEntry {
                    name: "📁 Local Filesystem".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "☸ PersistentVolumes (PV) - Direct access".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "☸ PersistentVolumeClaims (PVC) - Namespace scoped".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "☁ Cloud Storage (S3/GCS/Hetzner/DO)".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
            ];

            // Display storage options in the ACTIVE pane
            let pane = app.active_pane_mut();
            pane.entries = storage_options;
            pane.state.select(Some(0));
            pane.storage = std::sync::Arc::new(crate::fs::SelectingBackend);

            let pane_name = match app.active_pane {
                crate::app::ActivePane::Left => "LEFT",
                crate::app::ActivePane::Right => "RIGHT",
            };
            app.message = format!("{} pane: Select storage type (↑/↓ to navigate, Enter to select, Esc to cancel)", pane_name);
        }
        _ => {}
    }
    Ok(())
}

async fn handle_storage_select(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Cancel - restore to Local filesystem
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let pane = app.active_pane_mut();
            pane.storage = std::sync::Arc::new(crate::fs::LocalBackend::new(std::path::PathBuf::from(&home)));
            pane.path = home.clone();
            
            // Refresh the pane with local contents
            let path = std::path::PathBuf::from(&home);
            if let Ok(entries) = crate::fs::LocalFs::list_dir(&path) {
                pane.entries = entries;
                pane.state.select(Some(0));
            }
            
            app.mode = AppMode::Normal;
            app.message = "Cancelled - restored to local filesystem".to_string();
        }
        KeyCode::Up => {
            app.active_pane_mut().select_previous();
        }
        KeyCode::Down => {
            app.active_pane_mut().select_next();
        }
        KeyCode::Enter => {
            let entry_name = app.active_pane_mut().selected_entry().map(|e| e.name.clone());
            
            if let Some(name) = entry_name {
                if name.contains("Local Filesystem") {
                    // Switch to local filesystem
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                    let pane = app.active_pane_mut();
                    pane.storage = std::sync::Arc::new(crate::fs::LocalBackend::new(std::path::PathBuf::from(&home)));
                    pane.path = home.clone();
                    
                    // Load local directory
                    let path = std::path::PathBuf::from(&home);
                    if let Ok(entries) = crate::fs::LocalFs::list_dir(&path) {
                        pane.entries = entries;
                        if !pane.entries.is_empty() {
                            pane.state.select(Some(0));
                        }
                    }
                    
                    app.mode = AppMode::Normal;
                    app.message = "Switched to local filesystem".to_string();
                    
                } else if name.contains("PersistentVolumes") {
                    // Direct PV access - check if K8s is available
                    let Some(ref storage_manager) = app.storage_manager else {
                        app.message = "Kubernetes not available".to_string();
                        return Ok(());
                    };
                    
                    app.mode = AppMode::SelectPv;
                    app.message = "Loading PVs...".to_string();

                    let pvs = storage_manager.list_all_storage().await?;

                    let entries: Vec<_> = pvs
                        .iter()
                        .map(|pv| crate::fs::types::FileEntry {
                            name: format!(
                                "{} ({}) - {}",
                                pv.name,
                                pv.capacity,
                                pv.claim_ref.as_deref().unwrap_or("Available")
                            ),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        })
                        .collect();

                    let pane = app.active_pane_mut();
                    pane.entries = entries;
                    
                    if !pane.entries.is_empty() {
                        pane.state.select(Some(0));
                        app.message = "Select PV (↑/↓ to navigate, Enter to select, Esc to cancel)"
                            .to_string();
                    } else {
                        app.message = "No PVs found in cluster".to_string();
                        app.mode = AppMode::Normal;
                    }
                    
                } else if name.contains("Cloud Storage") {
                    // Cloud storage selection
                    app.mode = AppMode::SelectCloudProvider;
                    
                    // Show available cloud providers
                    let cloud_providers = vec![
                        crate::fs::types::FileEntry {
                            name: "☁ AWS S3".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ Google Cloud Storage (GCS)".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ DigitalOcean Spaces".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ Hetzner Object Storage".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ Cloudflare R2".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ MinIO (Local/Self-hosted)".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                        crate::fs::types::FileEntry {
                            name: "☁ Wasabi".to_string(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        },
                    ];
                    
                    let pane = app.active_pane_mut();
                    pane.entries = cloud_providers;
                    pane.state.select(Some(0));
                    app.message = "Select cloud provider (↑/↓ to navigate, Enter to select, Esc to cancel)".to_string();
                    
                } else if name.contains("PersistentVolumeClaims") {
                    // PVC access - check if K8s is available
                    let Some(ref storage_manager) = app.storage_manager else {
                        app.message = "Kubernetes not available".to_string();
                        return Ok(());
                    };
                    
                    // PVC access - show namespace selection
                    app.mode = AppMode::SelectNamespace;
                    app.namespaces = storage_manager.get_namespaces().await?;

                    // Display namespaces in active pane
                    let entries: Vec<_> = app
                        .namespaces
                        .iter()
                        .map(|ns| crate::fs::types::FileEntry {
                            name: ns.clone(),
                            size: 0,
                            is_dir: true,
                            modified: None,
                            permissions: None,
                        })
                        .collect();

                    let pane = app.active_pane_mut();
                    pane.entries = entries;
                    
                    if !pane.entries.is_empty() {
                        pane.state.select(Some(0));
                    }

                    app.message =
                        "Select namespace (↑/↓ to navigate, Enter to select, Esc to cancel)"
                            .to_string();
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle cloud provider selection
async fn handle_cloud_provider_select(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::SelectStorage;
            // Return to storage selection
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
                crate::fs::types::FileEntry {
                    name: "☁ Cloud Storage (S3/GCS/Hetzner/DO)".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
            ];
            app.right_pane.state.select(Some(0));
            app.message = "Cancelled - select storage type".to_string();
        }
        KeyCode::Up => {
            app.right_pane.select_previous();
        }
        KeyCode::Down => {
            app.right_pane.select_next();
        }
        KeyCode::Enter => {
            if let Some(entry) = app.right_pane.selected_entry() {
                let provider_name = entry.name.clone();
                
                // For now, show a message that cloud storage requires environment variables
                // Full credential input UI would require a text input mode
                app.message = format!(
                    "☁ {} selected. Set credentials via environment variables:\n\
                     AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, S3_BUCKET\n\
                     Then restart the application.\n\
                     (Full credential input UI coming soon)",
                    provider_name.trim_start_matches("☁ ")
                );
                
                // Return to normal mode for now
                app.mode = AppMode::Normal;
                
                // Note: A complete implementation would:
                // 1. Enter ConfigureCloud mode
                // 2. Display a text input form for bucket/region/credentials
                // 3. Store the configuration
                // 4. Connect to the cloud storage and display files
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle cloud storage configuration (placeholder for text input)
async fn handle_configure_cloud(app: &mut App, key: KeyEvent) -> Result<()> {
    // This is a placeholder - full text input would require additional UI work
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::SelectCloudProvider;
            app.message = "Configuration cancelled".to_string();
        }
        _ => {
            app.message = "Cloud configuration UI not yet implemented. Use environment variables.".to_string();
        }
    }
    Ok(())
}

async fn handle_namespace_select(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Cancelled".to_string();
        }
        KeyCode::Up => {
            app.right_pane.select_previous();
        }
        KeyCode::Down => {
            app.right_pane.select_next();
        }
        KeyCode::Enter => {
            if let Some(entry) = app.right_pane.selected_entry() {
                let selected_namespace = entry.name.clone();

                // Update current namespace
                app.current_namespace = selected_namespace.clone();

                // Check if K8s is available
                let Some(ref storage_manager) = app.storage_manager else {
                    app.message = "Kubernetes not available".to_string();
                    app.mode = AppMode::Normal;
                    return Ok(());
                };

                // Move to PVC selection
                app.mode = AppMode::SelectPvc;
                app.message =
                    "Select PVC (↑/↓ to navigate, Enter to select, Esc to cancel)".to_string();

                // Load PVCs for selected namespace
                let pvcs = storage_manager.list_pvcs(&selected_namespace).await?;

                // Convert PVCs to file entries for display
                app.right_pane.entries = pvcs
                    .iter()
                    .map(|pvc| crate::fs::types::FileEntry {
                        name: format!("{} ({})", pvc.name, pvc.capacity),
                        size: 0,
                        is_dir: true,
                        modified: None,
                        permissions: None,
                    })
                    .collect();

                // Reset selection
                if !app.right_pane.entries.is_empty() {
                    app.right_pane.state.select(Some(0));
                } else {
                    app.message = format!("No PVCs found in namespace '{}'", selected_namespace);
                    app.mode = AppMode::Normal;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_pvc_select(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Cancelled".to_string();
        }
        KeyCode::Up => {
            app.right_pane.select_previous();
        }
        KeyCode::Down => {
            app.right_pane.select_next();
        }
        KeyCode::Enter => {
            if let Some(entry) = app.right_pane.selected_entry() {
                // Check if K8s is available
                let Some(ref remote_fs) = app.remote_fs else {
                    app.message = "Kubernetes not available".to_string();
                    app.mode = AppMode::Normal;
                    return Ok(());
                };
                
                // Extract PVC name (remove capacity info)
                let pvc_name = entry
                    .name
                    .split(" (")
                    .next()
                    .unwrap_or(&entry.name)
                    .to_string();
                let namespace = app.current_namespace.clone();

                // Set right pane to browse this PVC using K8sBackend
                let k8s_backend = crate::fs::K8sBackend::new(
                    namespace.clone(),
                    pvc_name.clone(),
                    remote_fs.clone(),
                );
                app.right_pane.storage = std::sync::Arc::new(k8s_backend);
                app.right_pane.path = "/data".to_string();

                // Load files from PVC
                app.refresh_pane(crate::app::ActivePane::Right).await?;

                app.mode = AppMode::Normal;
                app.message = format!("Connected to PVC: {}", pvc_name);
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_pv_select(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Cancelled".to_string();
        }
        KeyCode::Up => {
            app.right_pane.select_previous();
        }
        KeyCode::Down => {
            app.right_pane.select_next();
        }
        KeyCode::Enter => {
            if let Some(entry) = app.right_pane.selected_entry() {
                // Check if K8s is available
                let Some(ref remote_fs) = app.remote_fs else {
                    app.message = "Kubernetes not available".to_string();
                    app.mode = AppMode::Normal;
                    return Ok(());
                };
                
                // Extract PV name (before first space)
                let pv_name = entry
                    .name
                    .split(' ')
                    .next()
                    .unwrap_or(&entry.name)
                    .to_string();

                // For PV access, we need to create a pod that mounts the PV directly
                // This is more complex as PVs don't have a namespace
                // For now, we'll use a default namespace
                let namespace = "default";

                // Set right pane to browse this PV using K8sBackend
                // Uses default namespace hack as before
                let k8s_backend = crate::fs::K8sBackend::new(
                    namespace.to_string(),
                    pv_name.clone(),
                    remote_fs.clone(),
                );
                app.right_pane.storage = std::sync::Arc::new(k8s_backend);
                app.right_pane.path = "/data".to_string();

                // Load files from PV
                app.refresh_pane(crate::app::ActivePane::Right).await?;

                app.mode = AppMode::Normal;
                app.message = format!("Connected to PV: {}", pv_name);
            }
        }
        _ => {}
    }
    Ok(())
}

fn count_files_in_dir(path: &std::path::Path) -> usize {
    if path.is_file() {
        return 1;
    }

    // Use jwalk for fast parallel directory walking
    jwalk::WalkDir::new(path)
        .skip_hidden(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
        .max(1) // At least 1 for the directory itself
}

async fn handle_copy(app: &mut App) -> Result<()> {
    // Don't start new copy if one is already running
    if app.background_task.is_some() {
        app.message = "Copy already in progress...".to_string();
        return Ok(());
    }

    // Get source and destination panes
    let (src_pane, dest_pane) = match app.active_pane {
        crate::app::ActivePane::Left => (&app.left_pane, &app.right_pane),
        crate::app::ActivePane::Right => (&app.right_pane, &app.left_pane),
    };

    if let Some(entry) = src_pane.selected_entry() {
        let entry_name = entry.name.clone();
        let entry_size = entry.size;
        let entry_is_dir = entry.is_dir;

        // Check destination path validity
        // Note: With VFS, empty path might be valid (root), but depends on backend.
        // SelectingBackend gives empty path usually.
        // We assume valid storage if not Selecting.
        // SelectingBackend rejects writes, so copy will fail gracefully if used.

        // Construct full generic paths
        let src_path_str = if src_pane.path.ends_with('/') || src_pane.path.is_empty() {
            format!("{}{}", src_pane.path, entry_name)
        } else {
            format!("{}/{}", src_pane.path, entry_name)
        };

        let dest_path_str = if dest_pane.path.ends_with('/') || dest_pane.path.is_empty() {
             format!("{}{}", dest_pane.path, entry_name)
        } else {
             format!("{}/{}", dest_pane.path, entry_name)
        };

        // Prepare for background task
        let src_storage = src_pane.storage.clone();
        let dest_storage = dest_pane.storage.clone();
        let src_full = src_path_str.clone();
        let dest_full = dest_path_str.clone();
        let entry_name_clone = entry_name.clone();

        app.message = format!("Copying {}...", entry_name);
        
        // Spawn background task
        let handle = tokio::spawn(async move {
            crate::fs::copy_between_backends(
                &*src_storage,
                &src_full,
                &*dest_storage,
                &dest_full,
                None
            ).await?;
            
            Ok(format!("✓ Copied {} successfully", entry_name_clone))
        });
        
        app.background_task = Some(handle);
        
        // Setup initial progress (generic/indeterminate)
        app.progress = Some(crate::app::Progress {
            stage: crate::app::ProgressStage::Transferring,
            current: 0,
            total: entry_size,
            current_file: entry_name,
            files_done: 0,
            total_files: if entry_is_dir { 0 } else { 1 },
        });

    } else {
        app.message = "No entry selected".to_string();
    }
    Ok(())
}

/// Show delete confirmation popup - sets up the target and switches mode
/// Show delete confirmation popup - sets up the target and switches mode
async fn handle_delete(app: &mut App) -> Result<()> {
    
    // Get info from active pane
    let pane = app.active_pane();
    
    if let Some(entry) = pane.selected_entry() {
        let entry_name = entry.name.clone();
        let is_dir = entry.is_dir;
        
        // Construct full path
        let path = if pane.path.ends_with('/') || pane.path.is_empty() {
            format!("{}{}", pane.path, entry_name)
        } else {
             format!("{}/{}", pane.path, entry_name)
        };
        
        // Populate generic DeleteTarget
        app.delete_target = Some(crate::app::DeleteTarget {
            backend: pane.storage.clone(),
            path: path.clone(),
            display_path: path.clone(),
            is_dir,
        });
        
        app.mode = crate::app::AppMode::ConfirmDelete;
        app.message = "Press Y to confirm delete, N or Esc to cancel".to_string();
    }

    Ok(())
}

/// Handle confirmation dialog for delete
/// Handle confirmation dialog for delete
async fn handle_confirm_delete(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            // User confirmed delete
            if let Some(target) = app.delete_target.clone() {
                // Generic delete operation
                match target.backend.delete(&target.path).await {
                    Ok(_) => {
                        app.message = format!("✓ Deleted {}", target.display_path);
                        // Refresh active pane so changes are reflected
                        app.refresh_active_pane().await?;
                    }
                    Err(e) => {
                         app.message = format!("✗ Error deleting: {}", e);
                    }
                }
            }
            app.delete_target = None;
            app.mode = AppMode::Normal;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            // User cancelled
            app.message = "Delete cancelled".to_string();
            app.delete_target = None;
            app.mode = AppMode::Normal;
        }
        _ => {
            // Ignore other keys, remind user
            app.message = "Press Y to confirm delete, N or Esc to cancel".to_string();
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

async fn handle_disk_usage(app: &mut App) -> Result<()> {
    use crate::app::ActivePane;

    if app.active_pane != ActivePane::Right {
        app.message = "F2: Switch to right pane to show PVC usage".to_string();
        return Ok(());
    }

    if app.right_pane.path.is_empty() {
        app.message = "F2: No PVC selected".to_string();
        return Ok(());
    }

    let path_clone = app.right_pane.path.clone();
    let parts: Vec<&str> = path_clone.split('/').collect();
    if parts.len() >= 2 {
        let namespace = parts[0];
        let pvc = parts[1];

        // Check if K8s is available
        let Some(ref remote_fs) = app.remote_fs else {
            app.message = "Kubernetes not available".to_string();
            return Ok(());
        };

        app.message = "Loading disk usage...".to_string();

        match remote_fs.get_disk_usage(namespace, pvc).await {
            Ok(usage) => {
                app.message = format!("📊 {} - {}", pvc, usage);
            }
            Err(e) => {
                app.message = format!("Error getting disk usage: {}", e);
            }
        }
    }

    Ok(())
}

async fn handle_disk_analyzer_enter(app: &mut App) -> Result<()> {
    use crate::app::ActivePane;

    if app.active_pane != ActivePane::Right {
        app.message = "F3: Switch to right pane for disk analysis".to_string();
        return Ok(());
    }

    if app.right_pane.path.is_empty() {
        app.message = "F3: No PVC selected".to_string();
        return Ok(());
    }

    let path_clone = app.right_pane.path.clone();
    let parts: Vec<&str> = path_clone.split('/').collect();
    if parts.len() >= 3 {
        let namespace = parts[0].to_string();
        let pvc = parts[1].to_string();
        let current_path = format!("/{}", parts[2..].join("/"));

        // Check if K8s is available
        let Some(ref remote_fs) = app.remote_fs else {
            app.message = "Kubernetes not available".to_string();
            return Ok(());
        };

        app.message = "Analyzing disk usage...".to_string();

        match remote_fs
            .get_directory_sizes(&namespace, &pvc, &current_path)
            .await
        {
            Ok(sizes) => {
                // Convert to file entries with size info in name
                app.right_pane.entries = sizes
                    .iter()
                    .map(|(name, size, is_dir)| crate::fs::types::FileEntry {
                        name: format!("{:>8} {}", format_size(*size), name),
                        size: *size,
                        is_dir: *is_dir,
                        modified: None,
                        permissions: None,
                    })
                    .collect();

                if !app.right_pane.entries.is_empty() {
                    app.right_pane.state.select(Some(0));
                }

                app.mode = AppMode::DiskAnalyzer;
                app.message = format!(
                    "📊 Disk Analysis: {} - Esc to exit, Enter to drill down",
                    current_path
                );
            }
            Err(e) => {
                app.message = format!("Error analyzing disk: {}", e);
            }
        }
    }

    Ok(())
}

async fn handle_disk_analyzer(app: &mut App, key: KeyEvent) -> Result<()> {
    // Check if K8s is available for disk analyzer operations
    let remote_fs = match &app.remote_fs {
        Some(fs) => fs.clone(),
        None => {
            app.message = "Kubernetes not available".to_string();
            app.mode = AppMode::Normal;
            return Ok(());
        }
    };
    
    match key.code {
        KeyCode::Esc => {
            // Exit analyzer, return to normal mode and refresh
            app.mode = AppMode::Normal;


                app.refresh_pane(crate::app::ActivePane::Right).await?;

            app.message = "Returned to file browser".to_string();
        }
        KeyCode::Up => {
            app.right_pane.select_previous();
        }
        KeyCode::Down => {
            app.right_pane.select_next();
        }
        KeyCode::Enter => {
            // Drill down into selected directory
            if let Some(entry) = app.right_pane.selected_entry() {
                if entry.is_dir {
                    // Extract the actual name (after the size)
                    let name = entry
                        .name
                        .split_whitespace()
                        .last()
                        .unwrap_or(&entry.name)
                        .to_string();

                    let path_clone = app.right_pane.path.clone();
                    let parts: Vec<&str> = path_clone.split('/').collect();
                    if parts.len() >= 3 {
                        let namespace = parts[0].to_string();
                        let pvc = parts[1].to_string();
                        let current_path = format!("/{}", parts[2..].join("/"));
                        let new_path = format!("{}/{}", current_path, name);

                        app.right_pane.path = format!("{}/{}{}", namespace, pvc, new_path);

                        match remote_fs
                            .get_directory_sizes(&namespace, &pvc, &new_path)
                            .await
                        {
                            Ok(sizes) => {
                                app.right_pane.entries = sizes
                                    .iter()
                                    .map(|(n, size, is_dir)| crate::fs::types::FileEntry {
                                        name: format!("{:>8} {}", format_size(*size), n),
                                        size: *size,
                                        is_dir: *is_dir,
                                        modified: None,
                                        permissions: None,
                                    })
                                    .collect();

                                if !app.right_pane.entries.is_empty() {
                                    app.right_pane.state.select(Some(0));
                                }

                                app.message =
                                    format!("📊 Disk Analysis: {} - Esc to exit", new_path);
                            }
                            Err(e) => {
                                app.message = format!("Error: {}", e);
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Backspace => {
            // Go up one directory in analyzer
            let path_clone = app.right_pane.path.clone();
            let parts: Vec<&str> = path_clone.split('/').collect();
            if parts.len() >= 3 {
                let namespace = parts[0].to_string();
                let pvc = parts[1].to_string();
                let current_path = format!("/{}", parts[2..].join("/"));

                if current_path != "/data" {
                    let path = std::path::Path::new(&current_path);
                    if let Some(parent) = path.parent() {
                        let new_path = if parent.to_string_lossy().is_empty()
                            || parent.to_string_lossy() == "/"
                        {
                            "/data".to_string()
                        } else {
                            parent.to_string_lossy().to_string()
                        };

                        app.right_pane.path = format!("{}/{}{}", namespace, pvc, new_path);

                        match remote_fs
                            .get_directory_sizes(&namespace, &pvc, &new_path)
                            .await
                        {
                            Ok(sizes) => {
                                app.right_pane.entries = sizes
                                    .iter()
                                    .map(|(n, size, is_dir)| crate::fs::types::FileEntry {
                                        name: format!("{:>8} {}", format_size(*size), n),
                                        size: *size,
                                        is_dir: *is_dir,
                                        modified: None,
                                        permissions: None,
                                    })
                                    .collect();

                                if !app.right_pane.entries.is_empty() {
                                    app.right_pane.state.select(Some(0));
                                }

                                app.message = format!("📊 Disk Analysis: {}", new_path);
                            }
                            Err(e) => {
                                app.message = format!("Error: {}", e);
                            }
                        }
                    }
                } else {
                    // At root, exit analyzer
                    app.mode = AppMode::Normal;
                    app.refresh_pane(crate::app::ActivePane::Right).await?;
                    app.message = "Returned to file browser".to_string();
                }
            }
        }
        _ => {}
    }

    Ok(())
}

// ============================================================================
// File Operation Handlers
// ============================================================================

/// Move file/directory from active pane to other pane (copy + delete).
async fn handle_move(app: &mut App) -> Result<()> {
    // Don't start new move if one is already running
    if app.background_task.is_some() {
        app.message = "Operation already in progress...".to_string();
        return Ok(());
    }

    // Get source and destination panes
    let (src_pane, dest_pane) = match app.active_pane {
        crate::app::ActivePane::Left => (&app.left_pane, &app.right_pane),
        crate::app::ActivePane::Right => (&app.right_pane, &app.left_pane),
    };

    if let Some(entry) = src_pane.selected_entry() {
        // Skip ".."
        if entry.name == ".." {
            app.message = "Cannot move '..'".to_string();
            return Ok(());
        }
        
        let entry_name = entry.name.clone();
        let entry_size = entry.size;
        let entry_is_dir = entry.is_dir;
        
        // Compute paths
        let src_path = if src_pane.path.ends_with('/') || src_pane.path.is_empty() {
            format!("{}{}", src_pane.path, entry_name)
        } else {
            format!("{}/{}", src_pane.path, entry_name)
        };
        
        let dest_path = if dest_pane.path.ends_with('/') || dest_pane.path.is_empty() {
            format!("{}{}", dest_pane.path, entry_name)
        } else {
            format!("{}/{}", dest_pane.path, entry_name)
        };
        
        let src_backend = src_pane.storage.clone();
        let dest_backend = dest_pane.storage.clone();
        
        app.message = format!("🔄 Moving {}...", entry_name);
        
        let entry_name_clone = entry_name.clone();
        let src_path_clone = src_path.clone();
        
        // Spawn move task (copy + delete)
        let handle = tokio::spawn(async move {
            // Copy first
            crate::fs::copy::copy_between_backends(
                src_backend.as_ref(),
                &src_path,
                dest_backend.as_ref(),
                &dest_path,
                None,
            ).await?;
            
            // Then delete source
            src_backend.delete(&src_path_clone).await?;
            
            Ok(format!("✓ Moved {} successfully", entry_name_clone))
        });
        
        app.background_task = Some(handle);
        
        app.progress = Some(crate::app::Progress {
            stage: crate::app::ProgressStage::Transferring,
            current: 0,
            total: entry_size,
            current_file: entry_name,
            files_done: 0,
            total_files: if entry_is_dir { 0 } else { 1 },
        });
    } else {
        app.message = "No entry selected".to_string();
    }
    Ok(())
}

/// Create a new directory in the active pane.
async fn handle_mkdir(app: &mut App) -> Result<()> {
    // For now, create a simple "NewFolder" directory
    // A proper implementation would use a text input popup
    let pane = app.active_pane_mut();
    
    let new_dir_name = "NewFolder";
    let new_path = if pane.path.ends_with('/') || pane.path.is_empty() {
        format!("{}{}", pane.path, new_dir_name)
    } else {
        format!("{}/{}", pane.path, new_dir_name)
    };
    
    match pane.storage.create_dir(&new_path).await {
        Ok(_) => {
            app.message = format!("✓ Created directory: {}", new_dir_name);
            app.refresh_active_pane().await?;
        }
        Err(e) => {
            app.message = format!("❌ Failed to create directory: {}", e);
        }
    }
    
    Ok(())
}

// ============================================================================
// Sync Handlers (Phase 3)
// ============================================================================

/// Toggle sync mode between enabled and disabled.
fn handle_sync_toggle(app: &mut App) -> Result<()> {
    use crate::app::SyncStatus;
    
    app.sync_enabled = !app.sync_enabled;
    
    if app.sync_enabled {
        app.sync_status = SyncStatus::Idle;
        app.message = "🔄 Sync enabled - Left pane ↔ Right pane | Ctrl+Y to sync now, Ctrl+D for dry-run".to_string();
    } else {
        app.sync_status = SyncStatus::Disabled;
        app.message = "Sync disabled".to_string();
    }
    
    Ok(())
}

/// Perform a one-time sync between left and right panes.
/// This spawns the sync as a background task and returns immediately.
async fn handle_sync_now(app: &mut App) -> Result<()> {
    use crate::app::SyncStatus;
    use crate::sync::{SyncEngine, SyncConfig, SyncMode};
    
    if !app.sync_enabled {
        app.message = "Sync not enabled - Press Ctrl+S to enable".to_string();
        return Ok(());
    }
    
    // Check if already syncing
    if app.sync_task.is_some() {
        app.message = "⚠️ Sync already in progress".to_string();
        return Ok(());
    }
    
    app.sync_status = SyncStatus::Scanning;
    app.message = "🔄 Starting sync (background)...".to_string();
    
    // Get backends and paths from both panes
    let left_backend = app.left_pane.storage.clone();
    let right_backend = app.right_pane.storage.clone();
    let left_path = app.left_pane.path.clone();
    let right_path = app.right_pane.path.clone();
    
    // Create progress channel
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(100);
    
    // Create sync engine with progress
    let config = SyncConfig {
        mode: SyncMode::OneWay, // Left -> Right by default
        ..Default::default()
    };
    
    let mut engine = SyncEngine::with_progress(left_backend, right_backend, config, progress_tx);
    
    // Spawn sync task - runs in background, doesn't block TUI
    let sync_handle = tokio::spawn(async move {
        engine.sync(&left_path, &right_path).await
    });
    
    // Store handles for polling in main event loop
    app.sync_task = Some(sync_handle);
    app.sync_progress_rx = Some(progress_rx);
    
    Ok(())
}

/// Poll for sync progress updates. Called from main event loop.
/// Returns true if sync is still in progress.
pub async fn poll_sync_progress(app: &mut App) -> Result<bool> {
    use crate::app::{SyncStatus, Progress, ProgressStage};
    use crate::sync::SyncPhase;
    
    // Check if we have an active sync
    let Some(ref mut rx) = app.sync_progress_rx else {
        return Ok(false);
    };
    
    // Try to receive progress updates (non-blocking)
    match rx.try_recv() {
        Ok(p) => {
            // Update progress bar
            app.progress = Some(Progress {
                stage: match p.phase {
                    SyncPhase::Scanning => ProgressStage::Counting,
                    SyncPhase::Comparing => ProgressStage::Counting,
                    SyncPhase::Transferring => ProgressStage::Transferring,
                    SyncPhase::Verifying => ProgressStage::Extracting,
                    SyncPhase::Complete => ProgressStage::Complete,
                },
                current: p.files_done as u64,
                total: p.total_files as u64,
                current_file: p.current_file.clone(),
                files_done: p.files_done,
                total_files: p.total_files,
            });
            
            app.sync_status = SyncStatus::Syncing {
                current_file: p.current_file.clone(),
                progress: p.percentage(),
            };
            
            app.message = format!("🔄 Syncing: {}/{} files", p.files_done, p.total_files);
            
            Ok(true) // Still syncing
        }
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
            // No new progress, check if task is done
            if let Some(ref task) = app.sync_task {
                if task.is_finished() {
                    // Task finished, get result
                    let task = app.sync_task.take().unwrap();
                    app.sync_progress_rx = None;
                    
                    match task.await {
                        Ok(Ok(result)) => {
                            let files_synced = result.stats.files_copied + result.stats.dirs_created;
                            app.sync_status = SyncStatus::Complete { files_synced };
                            app.progress = None;
                            app.message = format!(
                                "✅ Sync complete: {} copied, {} created, {} skipped, {} conflicts",
                                result.stats.files_copied,
                                result.stats.dirs_created,
                                result.stats.files_skipped,
                                result.stats.conflicts
                            );
                            
                            // Refresh both panes
                            app.refresh_pane(crate::app::ActivePane::Left).await?;
                            app.refresh_pane(crate::app::ActivePane::Right).await?;
                        }
                        Ok(Err(e)) => {
                            app.sync_status = SyncStatus::Error { message: e.to_string() };
                            app.progress = None;
                            app.message = format!("❌ Sync failed: {}", e);
                        }
                        Err(e) => {
                            app.sync_status = SyncStatus::Error { message: e.to_string() };
                            app.progress = None;
                            app.message = format!("❌ Sync task failed: {}", e);
                        }
                    }
                    
                    Ok(false) // Sync finished
                } else {
                    Ok(true) // Still syncing
                }
            } else {
                Ok(false)
            }
        }
        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
            // Channel closed, sync complete
            app.sync_progress_rx = None;
            if let Some(task) = app.sync_task.take() {
                match task.await {
                    Ok(Ok(result)) => {
                        let files_synced = result.stats.files_copied + result.stats.dirs_created;
                        app.sync_status = SyncStatus::Complete { files_synced };
                        app.progress = None;
                        app.message = format!(
                            "✅ Sync complete: {} copied, {} created, {} skipped",
                            result.stats.files_copied,
                            result.stats.dirs_created,
                            result.stats.files_skipped
                        );
                        
                        app.refresh_pane(crate::app::ActivePane::Left).await?;
                        app.refresh_pane(crate::app::ActivePane::Right).await?;
                    }
                    Ok(Err(e)) => {
                        app.sync_status = SyncStatus::Error { message: e.to_string() };
                        app.progress = None;
                        app.message = format!("❌ Sync failed: {}", e);
                    }
                    Err(e) => {
                        app.sync_status = SyncStatus::Error { message: e.to_string() };
                        app.progress = None;
                        app.message = format!("❌ Sync task failed: {}", e);
                    }
                }
            }
            Ok(false)
        }
    }
}

/// Perform a dry-run sync to preview changes.
async fn handle_sync_dry_run(app: &mut App) -> Result<()> {
    use crate::app::SyncStatus;
    use crate::sync::{SyncEngine, SyncConfig, SyncMode, SyncAction};
    
    app.sync_status = SyncStatus::Scanning;
    app.message = "🔍 Analyzing changes (dry-run)...".to_string();
    
    // Get backends and paths from both panes
    let left_backend = app.left_pane.storage.clone();
    let right_backend = app.right_pane.storage.clone();
    let left_path = app.left_pane.path.clone();
    let right_path = app.right_pane.path.clone();
    
    // Create sync engine with dry-run enabled
    let config = SyncConfig {
        mode: SyncMode::OneWay,
        dry_run: true,
        ..Default::default()
    };
    
    let mut engine = SyncEngine::new(left_backend, right_backend, config);
    
    // Perform dry-run
    match engine.dry_run(&left_path, &right_path).await {
        Ok(result) => {
            // Count actions by type
            let mut copies = 0;
            let mut creates = 0;
            let mut deletes = 0;
            let mut skips = 0;
            let mut conflicts = 0;
            
            for action in &result.actions {
                match action {
                    SyncAction::CopyToDestination { .. } | SyncAction::CopyToSource { .. } => copies += 1,
                    SyncAction::CreateDirInDestination { .. } | SyncAction::CreateDirInSource { .. } => creates += 1,
                    SyncAction::DeleteFromDestination { .. } | SyncAction::DeleteFromSource { .. } => deletes += 1,
                    SyncAction::Skip { .. } => skips += 1,
                    SyncAction::Conflict { .. } => conflicts += 1,
                }
            }
            
            app.sync_status = SyncStatus::Idle;
            app.message = format!(
                "📋 Dry-run: {} to copy, {} to create, {} to delete, {} skip, {} conflicts | Ctrl+Y to apply",
                copies, creates, deletes, skips, conflicts
            );
        }
        Err(e) => {
            app.sync_status = SyncStatus::Error { message: e.to_string() };
            app.message = format!("❌ Dry-run failed: {}", e);
        }
    }
    
    Ok(())
}
