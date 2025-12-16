use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, AppMode, LargeFileAction};

pub async fn handle_events(app: &mut App) -> Result<()> {
    // Poll for sync progress updates (non-blocking)
    let _ = poll_sync_progress(app).await;
    
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            // Global quit handlers - work in ALL modes
            if key.code == KeyCode::Char('q') || 
               (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                app.should_quit = true;
                return Ok(());
            }
            
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
                AppMode::Rename => handle_rename_mode(app, key).await?,
                AppMode::ViewFile => handle_view_file_mode(app, key).await?,
                AppMode::Search => handle_search_mode(app, key).await?,
                AppMode::EditFile => handle_edit_file_mode(app, key).await?,
                AppMode::ConfirmLargeLoad => handle_confirm_large_load_mode(app, key).await?,
                AppMode::EditorSearch => handle_editor_search_mode(app, key).await?,
                AppMode::HashMenu => handle_hash_menu(app, key).await?,
            }
        }
    }
    Ok(())
}

async fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
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
            // Rename file/directory
            handle_rename_start(app)?;
        }
        KeyCode::F(3) => {
            // View file contents
            handle_view_file(app).await?;
        }
        KeyCode::F(4) => {
            // Edit file (nano-like)
            handle_edit_file_start(app).await?;
        }
        KeyCode::F(9) => {
            // ncdu-like disk analyzer (moved from F4)
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
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Search/find files
            handle_search_start(app)?;
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
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Hash operations menu
            app.mode = AppMode::HashMenu;
            
            // Build hash menu options
            let hash_options = vec![
                crate::fs::types::FileEntry {
                    name: "🔍 Scan - Generate hash database".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "✓ Verify - Check files against database".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "🔄 Dedup - Find duplicate files".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "⚖ Compare - Compare two hash databases".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "⏱ Benchmark - Test hash algorithm speeds".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "📋 List Algorithms - Show available hash algorithms".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
            ];
            
            let pane = app.active_pane_mut();
            pane.entries = hash_options;
            pane.state.select(Some(0));
            pane.storage = std::sync::Arc::new(crate::fs::SelectingBackend);
            
            app.message = "Hash Menu: ↑/↓ navigate, Enter select, Esc cancel".to_string();
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
            // Return to storage selection menu
            app.mode = AppMode::SelectStorage;
            let pane = app.active_pane_mut();
            pane.entries = vec![
                crate::fs::types::FileEntry {
                    name: "📂 Local Filesystem".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "🗄 PersistentVolumes (PV) - Direct access".to_string(),
                    size: 0,
                    is_dir: true,
                    modified: None,
                    permissions: None,
                },
                crate::fs::types::FileEntry {
                    name: "📦 PersistentVolumeClaims (PVC) - Namespace scoped".to_string(),
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
            pane.state.select(Some(0));
            app.message = "Select storage type".to_string();
        }
        KeyCode::Up => {
            app.active_pane_mut().select_previous();
        }
        KeyCode::Down => {
            app.active_pane_mut().select_next();
        }
        KeyCode::Enter => {
            if let Some(entry) = app.active_pane().selected_entry().cloned() {
                let provider_name = entry.name.trim_start_matches("☁ ").to_string();
                
                // Helper to get env var or error
                let get_env = |key: &str| -> Result<String> {
                    std::env::var(key).map_err(|_| anyhow::anyhow!("Missing env var: {}", key))
                };
                
                let backend_result: Result<std::sync::Arc<dyn crate::fs::StorageBackend>> = match provider_name.as_str() {
                    "AWS S3" => {
                        let bucket = get_env("S3_BUCKET")?;
                        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
                        
                        if let (Ok(key), Ok(secret)) = (std::env::var("AWS_ACCESS_KEY_ID"), std::env::var("AWS_SECRET_ACCESS_KEY")) {
                             Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_aws(&bucket, &region, &key, &secret)?))
                        } else {
                             // Try IAM
                             Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_with_iam(&bucket, &region)?))
                        }
                    },
                    "Google Cloud Storage (GCS)" => {
                        let bucket = get_env("GCS_BUCKET")?;
                        if let Ok(cred) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
                             Ok(std::sync::Arc::new(crate::fs::gcs::GcsFs::from_service_account(&bucket, &cred)?))
                        } else {
                             // Try Workload Identity / ADC
                             Ok(std::sync::Arc::new(crate::fs::gcs::GcsFs::new_with_workload_identity(&bucket)?))
                        }
                    },
                    "DigitalOcean Spaces" => {
                        let bucket = get_env("DO_BUCKET")?;
                        let region = get_env("DO_REGION")?;
                        let key = get_env("DO_ACCESS_KEY_ID")?;
                        let secret = get_env("DO_SECRET_ACCESS_KEY")?;
                        Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_digitalocean(&bucket, &region, &key, &secret)?))
                    },
                    "Hetzner Object Storage" => {
                        let bucket = get_env("HETZNER_BUCKET")?;
                        let region = get_env("HETZNER_REGION")?;
                        let key = get_env("HETZNER_ACCESS_KEY")?;
                        let secret = get_env("HETZNER_SECRET_ACCESS_KEY")?;
                        Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_hetzner(&bucket, &region, &key, &secret)?))
                    },
                    "Cloudflare R2" => {
                        let bucket = get_env("R2_BUCKET")?;
                        let account = get_env("R2_ACCOUNT_ID")?;
                        let key = get_env("R2_ACCESS_KEY_ID")?;
                        let secret = get_env("R2_SECRET_ACCESS_KEY")?;
                        Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_cloudflare_r2(&bucket, &account, &key, &secret)?))
                    },
                    "MinIO (Local/Self-hosted)" => {
                        let bucket = get_env("MINIO_BUCKET")?;
                        let key = get_env("MINIO_ACCESS_KEY")?;
                        let secret = get_env("MINIO_SECRET_KEY")?;
                        Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_minio(&bucket, &key, &secret)?))
                    },
                    "Wasabi" => {
                        let bucket = get_env("WASABI_BUCKET")?;
                        let region = get_env("WASABI_REGION")?;
                        let key = get_env("WASABI_ACCESS_KEY")?;
                        let secret = get_env("WASABI_SECRET_KEY")?;
                        Ok(std::sync::Arc::new(crate::fs::s3::S3Fs::new_wasabi(&bucket, &region, &key, &secret)?))
                    },
                    _ => Err(anyhow::anyhow!("Unknown provider: {}", provider_name)),
                };
                
                match backend_result {
                    Ok(backend) => {
                        let pane = app.active_pane_mut();
                        pane.storage = backend;
                        pane.path = "".to_string(); // Root of bucket
                        app.refresh_active_pane().await?;
                        app.mode = AppMode::Normal;
                        app.message = format!("Connected to {}", provider_name);
                    }
                    Err(e) => {
                         app.message = format!("❌ Connection failed: {}", e);
                    }
                }
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

/// Handle hash menu selection
async fn handle_hash_menu(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Return to normal mode and restore directory
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let pane = app.active_pane_mut();
            pane.storage = std::sync::Arc::new(crate::fs::LocalBackend::new(std::path::PathBuf::from(&home)));
            pane.path = home.clone();
            app.refresh_active_pane().await?;
            app.mode = AppMode::Normal;
            app.message = "Hash menu cancelled".to_string();
        }
        KeyCode::Up => {
            app.active_pane_mut().select_previous();
        }
        KeyCode::Down => {
            app.active_pane_mut().select_next();
        }
        KeyCode::Enter => {
            let entry_name = app.active_pane().selected_entry().map(|e| e.name.clone());
            
            if let Some(name) = entry_name {
                // Restore pane to original state first
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let pane = app.active_pane_mut();
                let current_path = pane.path.clone();
                pane.storage = std::sync::Arc::new(crate::fs::LocalBackend::new(std::path::PathBuf::from(&home)));
                if current_path.is_empty() || current_path == "/" {
                    pane.path = home.clone();
                }
                
                app.mode = AppMode::Normal;
                
                if name.contains("Scan") {
                    // Scan current directory and generate hash database
                    let dir = app.active_pane().path.clone();
                    let output_path = format!("{}/hashes.txt", dir);
                    
                    app.message = format!("Scanning {}...", dir);
                    
                    // Run scan in background
                    let dir_clone = dir.clone();
                    let output_clone = output_path.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        use crate::hash::ScanEngine;
                        let engine = ScanEngine::with_parallel(true);
                        
                        let result = engine.scan_directory(
                            std::path::Path::new(&dir_clone),
                            "blake3",
                            std::path::Path::new(&output_clone),
                        );
                        
                        match result {
                            Ok(stats) => Ok(format!("✓ Scanned {} files → {}", stats.files_processed, output_clone)),
                            Err(e) => Err(anyhow::anyhow!("Scan failed: {}", e)),
                        }
                    });
                    
                    app.background_task = Some(tokio::spawn(async move {
                        handle.await.map_err(|e| anyhow::anyhow!("{}", e))?
                    }));
                    
                } else if name.contains("Verify") {
                    // Verify files against database in current directory
                    let dir = app.active_pane().path.clone();
                    let db_path = format!("{}/hashes.txt", dir);
                    
                    if !std::path::Path::new(&db_path).exists() {
                        app.message = format!("No database found at {}. Run Scan first.", db_path);
                    } else {
                        app.message = format!("Verifying {}...", dir);
                        
                        let dir_clone = dir.clone();
                        let db_clone = db_path.clone();
                        let handle = tokio::task::spawn_blocking(move || {
                            use crate::hash::VerifyEngine;
                            let engine = VerifyEngine::new();
                            
                            let result = engine.verify(
                                std::path::Path::new(&db_clone),
                                std::path::Path::new(&dir_clone),
                            );
                            
                            match result {
                                Ok(report) => {
                                    let status = if report.mismatches.is_empty() && report.missing_files.is_empty() {
                                        format!("✓ All {} files OK", report.matches)
                                    } else {
                                        format!("⚠ {} OK, {} changed, {} missing, {} new",
                                            report.matches,
                                            report.mismatches.len(),
                                            report.missing_files.len(),
                                            report.new_files.len())
                                    };
                                    Ok(status)
                                }
                                Err(e) => Err(anyhow::anyhow!("Verify failed: {}", e)),
                            }
                        });
                        
                        app.background_task = Some(tokio::spawn(async move {
                            handle.await.map_err(|e| anyhow::anyhow!("{}", e))?
                        }));
                    }
                    
                } else if name.contains("Dedup") {
                    // Find duplicate files in current directory
                    let dir = app.active_pane().path.clone();
                    
                    app.message = format!("Finding duplicates in {}...", dir);
                    
                    let dir_clone = dir.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        use crate::hash::DedupEngine;
                        let engine = DedupEngine::new().with_parallel(true);
                        
                        let result = engine.find_duplicates(
                            std::path::Path::new(&dir_clone),
                        );
                        
                        match result {
                            Ok(report) => {
                                let dup_count = report.duplicate_groups.len();
                                let wasted = report.stats.wasted_space;
                                if dup_count == 0 {
                                    Ok("✓ No duplicates found".to_string())
                                } else {
                                    Ok(format!("Found {} duplicate groups ({} bytes wasted)", dup_count, wasted))
                                }
                            }
                            Err(e) => Err(anyhow::anyhow!("Dedup failed: {}", e)),
                        }
                    });
                    
                    app.background_task = Some(tokio::spawn(async move {
                        handle.await.map_err(|e| anyhow::anyhow!("{}", e))?
                    }));
                    
                } else if name.contains("Benchmark") {
                    // Run hash algorithm benchmarks
                    app.message = "Running benchmarks (10MB data)...".to_string();
                    
                    let handle = tokio::task::spawn_blocking(move || {
                        use crate::hash::BenchmarkEngine;
                        let engine = BenchmarkEngine::new();
                        
                        let results = engine.run_benchmarks(10); // 10MB
                        
                        let results = results?;
                        
                        // Format results concisely
                        let fastest = results.iter()
                            .max_by(|a, b| a.throughput_mbps.partial_cmp(&b.throughput_mbps).unwrap());
                        
                        if let Some(best) = fastest {
                            Ok(format!("✓ Fastest: {} ({:.0} MB/s)", best.algorithm, best.throughput_mbps))
                        } else {
                            Ok("Benchmark complete".to_string())
                        }
                    });
                    
                    app.background_task = Some(tokio::spawn(async move {
                        handle.await.map_err(|e| anyhow::anyhow!("{}", e))?
                    }));
                    
                } else if name.contains("Compare") {
                    // Compare needs two database files - show message
                    app.message = "Compare: Select two hash database files to compare. Use F3 to view.".to_string();
                    
                } else if name.contains("List Algorithms") {
                    // List available hash algorithms
                    use crate::hash::HashRegistry;
                    let algorithms = HashRegistry::list_algorithms();
                    let algo_list: Vec<String> = algorithms.iter()
                        .map(|a| format!("{} ({}b)", a.name, a.output_bits))
                        .collect();
                    app.message = format!("Algorithms: {}", algo_list.join(", "));
                }
                
                app.refresh_active_pane().await?;
            }
        }
        _ => {}
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
        
        // Spawn move task
        let handle = tokio::spawn(async move {
            crate::fs::copy::move_between_backends(
                src_backend.as_ref(),
                &src_path,
                dest_backend.as_ref(),
                &dest_path,
            ).await?;
            
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
                "Dry-run: {} to copy, {} to create, {} to delete, {} skip, {} conflicts | Ctrl+Y to apply",
                copies, creates, deletes, skips, conflicts
            );
        }
        Err(e) => {
            app.sync_status = SyncStatus::Error { message: e.to_string() };
            app.message = format!("Dry-run failed: {}", e);
        }
    }
    
    Ok(())
}

// ============================================================================
// Rename Handlers
// ============================================================================

/// Start rename mode for selected file.
fn handle_rename_start(app: &mut App) -> Result<()> {
    if let Some(entry) = app.active_pane().selected_entry().cloned() {
        if entry.name == ".." {
            app.message = "Cannot rename '..'".to_string();
            return Ok(());
        }
        
        app.text_input = crate::app::TextInput::new(&entry.name);
        app.mode = AppMode::Rename;
        app.message = "Enter new name (Enter to confirm, Esc to cancel)".to_string();
    } else {
        app.message = "No file selected".to_string();
    }
    Ok(())
}

/// Handle rename mode input.
async fn handle_rename_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Rename cancelled".to_string();
        }
        KeyCode::Enter => {
            let new_name = app.text_input.value.trim().to_string();
            let old_name = app.text_input.original.clone();
            
            if new_name.is_empty() {
                app.message = "Name cannot be empty".to_string();
                return Ok(());
            }
            
            if new_name == old_name {
                app.mode = AppMode::Normal;
                app.message = "Name unchanged".to_string();
                return Ok(());
            }
            
            // Build paths
            let pane = app.active_pane();
            let base_path = &pane.path;
            let old_path = if base_path.ends_with('/') || base_path.is_empty() {
                format!("{}{}", base_path, old_name)
            } else {
                format!("{}/{}", base_path, old_name)
            };
            let new_path = if base_path.ends_with('/') || base_path.is_empty() {
                format!("{}{}", base_path, new_name)
            } else {
                format!("{}/{}", base_path, new_name)
            };
            
            let backend = app.active_pane().storage.clone();
            
            match backend.rename(&old_path, &new_path).await {
                Ok(_) => {
                    app.message = format!("Renamed '{}' to '{}'", old_name, new_name);
                    app.mode = AppMode::Normal;
                    app.refresh_active_pane().await?;
                }
                Err(e) => {
                    app.message = format!("Rename failed: {}", e);
                }
            }
        }
        KeyCode::Backspace => {
            app.text_input.delete_back();
        }
        KeyCode::Left => {
            app.text_input.move_left();
        }
        KeyCode::Right => {
            app.text_input.move_right();
        }
        KeyCode::Char(c) => {
            app.text_input.insert(c);
        }
        _ => {}
    }
    Ok(())
}

// ============================================================================
// View File Handlers
// ============================================================================

/// View selected file contents (uses editor in readonly mode).
async fn handle_view_file(app: &mut App) -> Result<()> {
    if let Some(entry) = app.active_pane().selected_entry().cloned() {
        if entry.name == ".." {
            app.message = "Cannot view '..'".to_string();
            return Ok(());
        }
        
        if entry.is_dir {
            app.message = "Cannot view directory".to_string();
            return Ok(());
        }
        
        let pane = app.active_pane();
        let path = if pane.path.ends_with('/') || pane.path.is_empty() {
            format!("{}{}", pane.path, entry.name)
        } else {
            format!("{}/{}", pane.path, entry.name)
        };
        
        let backend = pane.storage.clone();
        
        // Size check for remote files
        let is_remote = matches!(backend.backend_type(), crate::fs::BackendType::S3 { .. } | crate::fs::BackendType::Gcs { .. });
        
        if is_remote && entry.size > 40 * 1024 * 1024 {
            app.pending_large_action = Some(LargeFileAction::View);
            app.view_file_path = path;
            app.view_file_size = entry.size;
            app.mode = AppMode::ConfirmLargeLoad;
            app.message = format!("Remote file is large ({} MB). View? (y/n)", entry.size / 1024 / 1024);
            return Ok(());
        }
        
        // Read file content and load into editor as readonly
        match backend.read_bytes(&path).await {
            Ok(data) => {
                let content = String::from_utf8_lossy(&data).to_string();
                let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                
                app.editor = crate::app::TextEditor {
                    content: if lines.is_empty() { vec![String::new()] } else { lines },
                    cursor_row: 0,
                    cursor_col: 0,
                    scroll_offset: 0,
                    filename: entry.name.clone(),
                    modified: false,
                    cut_buffer: None,
                    visible_height: 0,
                    readonly: true,  // View mode is readonly
                };
                
                app.mode = AppMode::EditFile;  // Use same mode, but readonly flag prevents edits
                app.message = format!("Viewing: {} (readonly) - q/Esc to close", entry.name);
            }
            Err(e) => {
                app.message = format!("Failed to read file: {}", e);
            }
        }
    } else {
        app.message = "No file selected".to_string();
    }
    Ok(())
}

async fn load_view_chunk(app: &mut App, backend: std::sync::Arc<dyn crate::fs::StorageBackend>, path: &str, offset: u64, total_size: u64) -> Result<()> {
    match backend.read_range(path, offset, 64 * 1024).await { // 64KB chunk
        Ok(data) => {
             let content = String::from_utf8_lossy(&data);
             app.view_content = content.lines().map(|s| s.to_string()).collect();
             // Add continuation marker if we truncated a line or middle of file?
             // Simple approach: just show lines.
             app.view_scroll = 0;
             app.view_file_size = total_size;
             app.view_file_offset = offset;
             app.view_file_path = path.to_string();
             app.mode = AppMode::ViewFile;
             app.message = format!("Viewing (Stream {}%): {} - q/Esc to close", (offset * 100) / total_size.max(1), path);
        }
        Err(e) => {
             app.message = format!("Failed to stream: {}", e);
        }
    }
    Ok(())
}

/// Handle view file mode input.
async fn handle_view_file_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = AppMode::Normal;
            app.view_content.clear();
            app.message = String::new();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.view_scroll > 0 {
                app.view_scroll -= 1;
            } else if app.view_file_size > 0 && app.view_file_offset >= 64 * 1024 {
                // Prev Chunk
                let prev_offset = app.view_file_offset - 64 * 1024;
                let pane = app.active_pane();
                let backend = pane.storage.clone();
                return load_view_chunk(app, backend, &app.view_file_path.clone(), prev_offset, app.view_file_size).await;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.view_scroll < app.view_content.len().saturating_sub(20) {
                app.view_scroll += 1;
            } else if app.view_file_size > 0 && app.view_file_offset + 64 * 1024 < app.view_file_size {
                // Next Chunk
                let next_offset = app.view_file_offset + 64 * 1024;
                let pane = app.active_pane();
                let backend = pane.storage.clone();
                return load_view_chunk(app, backend, &app.view_file_path.clone(), next_offset, app.view_file_size).await;
            }
        }
        KeyCode::PageUp => {
            if app.view_scroll > 0 {
                app.view_scroll = app.view_scroll.saturating_sub(20);
            } else if app.view_file_size > 0 && app.view_file_offset >= 64 * 1024 {
                // Prev Chunk
                let prev_offset = app.view_file_offset - 64 * 1024;
                let pane = app.active_pane();
                let backend = pane.storage.clone();
                return load_view_chunk(app, backend, &app.view_file_path.clone(), prev_offset, app.view_file_size).await;
            }
        }
        KeyCode::PageDown => {
            if app.view_scroll < app.view_content.len().saturating_sub(20) {
                 app.view_scroll = (app.view_scroll + 20).min(app.view_content.len().saturating_sub(20));
            } else if app.view_file_size > 0 && app.view_file_offset + 64 * 1024 < app.view_file_size {
                 // Next Chunk
                 let next_offset = app.view_file_offset + 64 * 1024;
                 let pane = app.active_pane();
                 let backend = pane.storage.clone();
                 return load_view_chunk(app, backend, &app.view_file_path.clone(), next_offset, app.view_file_size).await;
            }
        }
        KeyCode::Home => {
            app.view_scroll = 0;
            // Optionally: Jump to offset 0?
             if app.view_file_size > 0 && app.view_file_offset > 0 {
                  let pane = app.active_pane();
                  let backend = pane.storage.clone();
                  return load_view_chunk(app, backend, &app.view_file_path.clone(), 0, app.view_file_size).await;
             }
        }
        KeyCode::End => {
            app.view_scroll = app.view_content.len().saturating_sub(20);
             // Optionally: Jump to last chunk? (Tricky to calculate offset without ceil)
        }
        _ => {}
    }
    Ok(())
}

/// Handle confirmation for large file load.
async fn handle_confirm_large_load_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            if let Some(action) = app.pending_large_action.clone() {
                // Clear pending
                app.pending_large_action = None;
                
                match action {
                    LargeFileAction::View => {
                         let pane = app.active_pane();
                         let backend = pane.storage.clone();
                         // Load first chunk
                         load_view_chunk(app, backend, &app.view_file_path.clone(), 0, app.view_file_size).await?;
                    }
                    LargeFileAction::Edit => {
                         // Proceed to edit
                         // We need to call logic similar to handle_edit_file_start but bypass check
                         // Since we don't have separate function for "load editor", we duplicate logic for now
                         let pane = app.active_pane();
                         let backend = pane.storage.clone();
                         match backend.read_bytes(&app.view_file_path).await {
                             Ok(data) => {
                                 let content = String::from_utf8_lossy(&data).to_string();
                                 let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                                 app.editor = crate::app::TextEditor {
                                     content: if lines.is_empty() { vec![String::new()] } else { lines },
                                     cursor_row: 0,
                                     cursor_col: 0,
                                     scroll_offset: 0,
                                     filename: app.view_file_path.rsplit('/').next().unwrap_or("").to_string(), // Approximate default
                                     modified: false,
                                     cut_buffer: None,
                                     visible_height: 0,
                                     readonly: false,
                                 };
                                 app.mode = AppMode::EditFile;
                                 app.message = format!("Editing: {} - ^O: WriteOut, ^X: Exit, ^K: Cut, ^U: Uncut", app.editor.filename);
                             }
                             Err(e) => {
                                 app.message = format!("Failed to read file: {}", e);
                                 app.mode = AppMode::Normal;
                             }
                         }
                    }
                }
            } else {
                 app.mode = AppMode::Normal;
            }
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Cancelled load".to_string();
            app.pending_large_action = None;
        }
        _ => {}
    }
    Ok(())
}

// ============================================================================
// Search Handlers
// ============================================================================

/// Start search mode.
fn handle_search_start(app: &mut App) -> Result<()> {
    app.text_input = crate::app::TextInput::new("");
    app.mode = AppMode::Search;
    app.message = "Search: (Enter to find, Esc to cancel)".to_string();
    Ok(())
}

/// Handle search mode input.
async fn handle_search_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Normal;
            app.message = "Search cancelled".to_string();
        }
        KeyCode::Enter => {
            let pattern = app.text_input.value.to_lowercase();
            if pattern.is_empty() {
                app.mode = AppMode::Normal;
                app.message = "Empty search pattern".to_string();
                return Ok(());
            }
            
            // Search in current pane entries
            let pane = app.active_pane();
            let mut found_idx = None;
            let mut found_name = String::new();
            
            for (i, entry) in pane.entries.iter().enumerate() {
                if entry.name.to_lowercase().contains(&pattern) {
                    found_idx = Some(i);
                    found_name = entry.name.clone();
                    break;
                }
            }
            
            if let Some(i) = found_idx {
                app.active_pane_mut().state.select(Some(i));
                app.message = format!("Found: {}", found_name);
            } else {
                app.message = format!("No match for '{}'", pattern);
            }
            
            app.mode = AppMode::Normal;
        }
        KeyCode::Backspace => {
            app.text_input.delete_back();
        }
        KeyCode::Char(c) => {
            app.text_input.insert(c);
        }
        _ => {}
    }
    Ok(())
}

// ============================================================================
// File Editor Handlers
// ============================================================================

/// Start edit mode for selected file.
async fn handle_edit_file_start(app: &mut App) -> Result<()> {
    if let Some(entry) = app.active_pane().selected_entry().cloned() {
        if entry.name == ".." {
            app.message = "Cannot edit '..'".to_string();
            return Ok(());
        }
        
        
        if entry.is_dir {
            app.message = "Cannot edit directory".to_string();
            return Ok(());
        }

        let pane = app.active_pane();
        
        // Size check for remote
        let backend = pane.storage.clone();
        let is_remote = matches!(backend.backend_type(), crate::fs::BackendType::S3 { .. } | crate::fs::BackendType::Gcs { .. });
        
        if is_remote && entry.size > 40 * 1024 * 1024 {
             // Build path just for storing state
             let path = if pane.path.ends_with('/') || pane.path.is_empty() {
                format!("{}{}", pane.path, entry.name)
             } else {
                format!("{}/{}", pane.path, entry.name)
             };
             
             app.view_file_path = path;
             app.view_file_size = entry.size;
             app.pending_large_action = Some(LargeFileAction::Edit);
             app.mode = AppMode::ConfirmLargeLoad;
             app.message = format!("Remote file is large ({} MB). Download/Edit? (y/n)", entry.size / 1024 / 1024);
             return Ok(());
        }
        let path = if pane.path.ends_with('/') || pane.path.is_empty() {
            format!("{}{}", pane.path, entry.name)
        } else {
            format!("{}/{}", pane.path, entry.name)
        };
        
        let backend = pane.storage.clone();
        
        // Read file content
        match backend.read_bytes(&path).await {
            Ok(data) => {
                let content = String::from_utf8_lossy(&data).to_string();
                let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                
                app.editor = crate::app::TextEditor {
                    content: if lines.is_empty() { vec![String::new()] } else { lines },
                    cursor_row: 0,
                    cursor_col: 0,
                    scroll_offset: 0,
                    filename: entry.name.clone(),
                    modified: false,
                    cut_buffer: None,
                    visible_height: 0,
                    readonly: false,
                };
                
                app.mode = AppMode::EditFile;
                app.message = format!("Editing: {} - ^O: WriteOut, ^X: Exit, ^K: Cut, ^U: Uncut", entry.name);
            }
            Err(e) => {
                app.message = format!("Failed to read file: {}", e);
            }
        }
    } else {
        app.message = "No file selected".to_string();
    }
    Ok(())
}

/// Handle edit file mode input.
async fn handle_edit_file_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    let readonly = app.editor.readonly;
    
    match key.code {
        // Exit: Ctrl+X or Ctrl+Q or Esc (or just q/Esc in readonly mode)
        KeyCode::Esc => {
            if app.editor.modified {
                app.message = "Changes discarded".to_string();
            }
            app.mode = AppMode::Normal;
        }
        KeyCode::Char('q') if readonly || key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.editor.modified {
                app.message = "Changes discarded".to_string();
            }
            app.mode = AppMode::Normal;
        }
        KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.editor.modified {
                app.message = "Changes discarded".to_string();
            }
            app.mode = AppMode::Normal;
        }
        // Save: Ctrl+O (Write Out) or Ctrl+S - blocked in readonly mode
        KeyCode::Char('o') | KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if readonly {
                app.message = "Cannot save: file is readonly".to_string();
            } else {
                let content = app.editor.content.join("\n");
                let pane = app.active_pane();
                let path = if pane.path.ends_with('/') || pane.path.is_empty() {
                    format!("{}{}", pane.path, app.editor.filename)
                } else {
                    format!("{}/{}", pane.path, app.editor.filename)
                };
                
                let backend = pane.storage.clone();
                match backend.write_bytes(&path, content.as_bytes().to_vec()).await {
                    Ok(_) => {
                        app.editor.modified = false;
                        app.message = format!("Saved '{}'", app.editor.filename);
                        app.refresh_active_pane().await?;
                    }
                    Err(e) => {
                        app.message = format!("Save failed: {}", e);
                    }
                }
            }
        }
        // Cut Line: Ctrl+K - blocked in readonly mode
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !readonly {
                app.editor.cut_line();
            }
        }
        // Uncut (Paste) Line: Ctrl+U - blocked in readonly mode
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !readonly {
                app.editor.uncut_line();
            }
        }
        // Search: Ctrl+W - allowed in readonly mode
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.mode = AppMode::EditorSearch;
            app.text_input.clear();
            app.message = "Search (Where Is): ".to_string();
        }
        // Navigation - always allowed
        KeyCode::Up => {
            if app.editor.cursor_row > 0 {
                app.editor.cursor_row -= 1;
                let line_len = app.editor.content[app.editor.cursor_row].len();
                if app.editor.cursor_col > line_len {
                    app.editor.cursor_col = line_len;
                }
            }
        }
        KeyCode::Down => {
            if app.editor.cursor_row < app.editor.content.len().saturating_sub(1) {
                app.editor.cursor_row += 1;
                let line_len = app.editor.content[app.editor.cursor_row].len();
                if app.editor.cursor_col > line_len {
                    app.editor.cursor_col = line_len;
                }
            }
        }
        KeyCode::Left => {
            if app.editor.cursor_col > 0 {
                app.editor.cursor_col -= 1;
            } else if app.editor.cursor_row > 0 {
                app.editor.cursor_row -= 1;
                app.editor.cursor_col = app.editor.content[app.editor.cursor_row].len();
            }
        }
        KeyCode::Right => {
            let line_len = app.editor.content[app.editor.cursor_row].len();
            if app.editor.cursor_col < line_len {
                app.editor.cursor_col += 1;
            } else if app.editor.cursor_row < app.editor.content.len().saturating_sub(1) {
                app.editor.cursor_row += 1;
                app.editor.cursor_col = 0;
            }
        }
        KeyCode::Home => app.editor.cursor_col = 0,
        KeyCode::End => app.editor.cursor_col = app.editor.content[app.editor.cursor_row].len(),
        KeyCode::PageUp => {
            let visible = if app.editor.visible_height > 0 { app.editor.visible_height } else { 20 };
            app.editor.cursor_row = app.editor.cursor_row.saturating_sub(visible);
        }
        KeyCode::PageDown => {
            let visible = if app.editor.visible_height > 0 { app.editor.visible_height } else { 20 };
            app.editor.cursor_row = (app.editor.cursor_row + visible).min(app.editor.content.len().saturating_sub(1));
        }
        // Editing operations - blocked in readonly mode
        KeyCode::Backspace => {
            if !readonly {
                app.editor.delete_back();
            }
        }
        KeyCode::Enter => {
            if !readonly {
                app.editor.insert_newline();
            }
        }
        KeyCode::Char(c) => {
            if !readonly {
                app.editor.insert_char(c);
            }
        }
        _ => {}
    }
    
    // Adjust scroll logic - use visible_height if available, fallback to 20
    let visible = if app.editor.visible_height > 0 { app.editor.visible_height } else { 20 };
    if app.editor.cursor_row < app.editor.scroll_offset {
        app.editor.scroll_offset = app.editor.cursor_row;
    } else if app.editor.cursor_row >= app.editor.scroll_offset + visible {
         app.editor.scroll_offset = app.editor.cursor_row.saturating_sub(visible - 1);
    }
    
    Ok(())
}

/// Handle editor search mode input.
async fn handle_editor_search_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::EditFile;
            app.message = format!("Editing: {} - ^O: WriteOut, ^X: Exit, ^K: Cut, ^U: Uncut", app.editor.filename);
        }
        KeyCode::Enter => {
            let pattern = app.text_input.value.clone();
            if pattern.is_empty() {
                app.mode = AppMode::EditFile;
                return Ok(());
            }
            
            // Search in editor content
            let start_row = app.editor.cursor_row;
            let start_col = app.editor.cursor_col;
            
            let mut found = false;
            
            // Simple search: forward from current line
            for (i, line) in app.editor.content.iter().enumerate().skip(start_row) {
                let check_col = if i == start_row { start_col } else { 0 };
                let line_search = line.to_lowercase();
                let pattern_search = pattern.to_lowercase();
                
                if let Some(idx) = line_search[check_col..].find(&pattern_search) {
                    app.editor.cursor_row = i;
                    app.editor.cursor_col = check_col + idx;
                    found = true;
                    // Adjust scroll
                    let visible = if app.editor.visible_height > 0 { app.editor.visible_height } else { 20 };
                    if app.editor.cursor_row >= app.editor.scroll_offset + visible {
                        app.editor.scroll_offset = app.editor.cursor_row.saturating_sub(visible / 2);
                    }
                    break;
                }
            }
            
            // Wrap around
            if !found {
                 for (i, line) in app.editor.content.iter().enumerate().take(start_row + 1) {
                     let line_search = line.to_lowercase();
                     let pattern_search = pattern.to_lowercase();
                     if let Some(idx) = line_search.find(&pattern_search) {
                        app.editor.cursor_row = i;
                        app.editor.cursor_col = idx;
                        found = true;
                        // Adjust scroll
                        let visible = if app.editor.visible_height > 0 { app.editor.visible_height } else { 20 };
                        if app.editor.cursor_row < app.editor.scroll_offset {
                             app.editor.scroll_offset = i;
                        } else if app.editor.cursor_row >= app.editor.scroll_offset + visible {
                             app.editor.scroll_offset = app.editor.cursor_row.saturating_sub(visible / 2);
                        }
                        break;
                     }
                 }
            }
            
            app.mode = AppMode::EditFile;
            if found {
                app.message = format!("Found '{}'", pattern);
            } else {
                app.message = format!("Not found '{}'", pattern);
            }
        }
        KeyCode::Backspace => app.text_input.delete_back(),
        KeyCode::Left => app.text_input.move_left(),
        KeyCode::Right => app.text_input.move_right(),
        KeyCode::Char(c) => app.text_input.insert(c),
        _ => {}
    }
    Ok(())
}
