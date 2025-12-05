# Abyss Roadmap - Comprehensive Feature Plan

> A dual-pane TUI file manager supporting Kubernetes, cloud storage, and multi-backend operations.

---

## Phase 1: Core Architecture
**Status:** MVP Complete - Focus on extensibility

### 1.1 Virtual File System (VFS) Abstraction
- [ ] **Refactor `RemoteFs`**: Decouple current K8s logic into a generic `FileSystemBackend` implementation
- [ ] Define `FileSystemBackend` trait:
  - Standardize: `list_dir()`, `read_stream()`, `write_stream()`, `delete()`, `mkdir()`, `stat()`, `rename()`
  - Add capabilities flags (e.g., `supports_streaming`, `supports_append`)
- [ ] Refactor panes to use `Box<dyn FileSystemBackend>` instead of concrete types
- [ ] Enable runtime backend switching (Local ↔ K8s ↔ S3)
- [ ] Error handling standardization (custom `FsError` enum)

### 1.2 Configuration System
- [ ] Config file: `~/.config/abyss/config.toml`
- [ ] Persistent state: last directories, active pane, window size
- [ ] User preferences:
  - Default transfer location
  - Theme selection
  - Compression settings
  - Transfer behavior (auto-confirm, bandwidth limit)
- [ ] Credentials management (secure storage)

### 1.3 Enhanced Local Filesystem
- [ ] Support symlinks (follow/display info)
- [ ] File permissions viewing/editing (chmod support)
- [ ] Hardlink detection
- [ ] Hidden file toggle (Ctrl+H)
- [ ] Sort options: by name/size/date/type/extension

### 1.4 Archive & Package Support
- [ ] Mount compressed archives (zip, tar, gz, 7z) as virtual directories
- [ ] Browse contents without full extraction
- [ ] Extract specific files/folders
- [ ] Create archives from selection (context menu)

---

## Phase 2: Cloud Storage Backends
**Goal:** Seamless multi-cloud support

### 2.1 AWS S3 Integration (`s3_backend` module)
- [ ] Implement `S3Fs` backend using `aws-sdk-s3`
- [ ] Authentication: IAM roles, credentials profiles, environment variables
- [ ] Features:
  - Bucket browsing as directories
  - Object listing with metadata
  - Multipart upload/download for large files
  - S3 metadata display (ETag, storage class, retention)
  - Support: MinIO, DigitalOcean Spaces, Wasabi

### 2.2 Google Cloud Storage (GCP)
- [ ] Implement `GcsFs` backend
- [ ] Service Account & OAuth2 support
- [ ] Bucket/object operations parity with S3
- [ ] Integration with gcloud CLI credentials

### 2.3 Azure Blob Storage
- [ ] Implement `AzureFs` backend
- [ ] Connection string & SAS token support
- [ ] Container/blob browsing
- [ ] Hierarchical namespace (HNS) support for Azure Data Lake

### 2.4 SFTP/SSH Backend
- [ ] Implement `SftpFs` using `ssh2` crate
- [ ] Password & key-based authentication
- [ ] SSH agent integration
- [ ] Throughput optimization for remote transfers

### 2.5 Backend Plugin System
- [ ] Plugin discovery: `~/.config/abyss/plugins/`
- [ ] Plugin trait definition for community backends
- [ ] Load backends dynamically at startup
- [ ] Examples: OneDrive, Dropbox, FTP, WebDAV

---

## Phase 3: Sync & Replication
**Goal:** Enterprise-grade data synchronization

### 3.1 Bidirectional Sync
- [ ] Conflict detection & resolution strategies:
  - Last-write-wins (default)
  - Newer-wins
  - Manual resolution UI
  - Keep both (rename strategy)
- [ ] Change tracking (inotify on Linux, FSEvents on macOS, ReadDirectoryChanges on Windows)
- [ ] Real-time sync mode (toggle with Ctrl+S)
- [ ] Sync status indicator in status bar

### 3.2 Scheduling & Automation
- [ ] Cron-like scheduling for periodic syncs
- [ ] Retention policies:
  - Delete old files after X days
  - Keep N most recent versions
- [ ] Pre-sync validation (dry-run mode)
- [ ] Sync statistics & bandwidth monitoring

### 3.3 Compression & Bandwidth Optimization
- [ ] Stream compression (gzip, brotli, zstd) for transfers
- [ ] Auto-detect already-compressed content (skip re-compression)
- [ ] Intelligent chunking for parallel transfers
- [ ] Bandwidth throttling (configurable limits per session/global)

### 3.4 Smart Sync Strategies
- [ ] Merkle Tree hashing for efficient differential sync
- [ ] Rolling checksums (Rsync algorithm) for large file updates
- [ ] "Git-like" versioning for tracked directories

---

## Phase 4: Security & Data Integrity
**Goal:** Enterprise security compliance

### 4.1 End-to-End Encryption (E2EE)
- [ ] Client-side encryption before upload (Zero-Knowledge)
- [ ] Key derivation (PBKDF2/Argon2)
- [ ] Support: AES-256-GCM, ChaCha20-Poly1305
- [ ] Integration with `age` or `age-encrypt` for key management
- [ ] Transparent decryption on download (with cached keys)
- [ ] Key rotation support

### 4.2 Data Integrity & Verification
- [ ] Checksum calculation (MD5, SHA256, BLAKE3)
- [ ] Verify checksums during transfers
- [ ] Remote integrity check without full download (S3 ETag, GCS metadata)
- [ ] Hash display in file info (F2 key)
- [ ] Batch verification command

### 4.3 Audit & Compliance Logging
- [ ] Detailed operation logs (JSON/CSV export)
- [ ] Operation timestamps, source, destination, user
- [ ] Deleted file audit trail
- [ ] Compliance report generation (GDPR, HIPAA, SOC2)
- [ ] Immutable logs option

### 4.4 Key Management & Secrets
- [ ] Integration with HashiCorp Vault
- [ ] AWS KMS / GCP KMS support for master keys
- [ ] Secure storage of backend credentials (system keychain integration)

---

## Phase 5: Performance & Reliability
**Goal:** Handle enterprise workloads

### 5.1 Caching & Optimization
- [ ] LRU cache for directory listings (with TTL)
- [ ] Metadata prefetching for large directories
- [ ] Connection pooling for backends
- [ ] Smart caching: detect stale data, auto-refresh

### 5.2 Parallel & Concurrent Operations
- [ ] Multi-threaded transfers (configurable worker count)
- [ ] Multipart uploads for S3/GCP (split large files into chunks)
- [ ] Concurrent file copies (with dependency resolution)
- [ ] Queue management with priority levels
- [ ] Resource limits: max concurrent ops, max bandwidth

### 5.3 Resumable & Reliable Transfers
- [ ] Interrupt recovery (save transfer state to disk)
- [ ] Resume partial downloads (HTTP Range requests)
- [ ] Automatic retry with exponential backoff
- [ ] Circuit breaker for failed operations
- [ ] Transfer timeout configuration

### 5.4 Large File Handling
- [ ] Streaming support (no full load in memory)
- [ ] Sparse file detection & handling
- [ ] Partial file preview (first/last N bytes)
- [ ] Progress streaming (update UI every 100MB)

---

## Phase 6: Kubernetes Advanced Features
**Goal:** Full Kubernetes ecosystem integration

### 6.1 Enhanced Pod Management
- [ ] Multi-container pod support (select container in UI)
- [ ] Pod shell access (drop into bash/sh in pod context)
- [ ] Interactive terminal with full TTY support (VT100 emulation)
- [ ] Port forwarding manager (Ctrl+F):
  - List active forwards
  - Create/delete ephemeral forwards
  - Quick access to internal services

### 6.2 Kubernetes Objects as Virtual Files
- [ ] Secrets/ConfigMaps browsing (base64 decode display)
- [ ] Edit Secrets/ConfigMaps with validation
- [ ] View Logs (real-time streaming with filtering)
- [ ] Pod metrics display (CPU, memory)
- [ ] Resource YAML editor with schema validation

### 6.3 K8s Cluster Management
- [ ] Multi-cluster switching (kubeconfig contexts)
- [ ] Namespace quick-switch
- [ ] Pod lifecycle management (restart, delete, logs)
- [ ] Resource quotas & limits visualization
- [ ] RBAC permission checking (can-i)

### 6.4 Robust Remote Agent
- [ ] Inject ephemeral Rust/Go binary instead of `busybox` dependency
- [ ] Implement reliable RPC over `kubectl exec` (stdin/stdout)
- [ ] Remove dependency on `tar` and `ls` parsing
- [ ] High-performance direct stream copying

---

## Phase 7: DevOps & GitOps Integration
**Goal:** Manage the full delivery lifecycle from the terminal

### 7.1 GitOps (ArgoCD & Flux)
- [ ] **ArgoCD Integration**:
  - View Applications as virtual directories
  - Visual resource tree (hierarchy of managed objects)
  - Sync status indicators (Healthy, OutOfSync, Degraded)
  - Actions: Sync, Hard Refresh, Rollback, Diff
- [ ] **Flux CD Support**:
  - Browse Kustomizations and Sources
  - Trigger reconciliations (`flux reconcile`)
  - View events and applied revisions

### 7.2 Package Management (Helm)
- [ ] Browse Helm releases as virtual directories
- [ ] View "user values" vs "computed values"
- [ ] Revision history inspection
- [ ] Rollback to previous version
- [ ] Uninstall/Upgrade actions via UI

### 7.3 CI/CD Pipelines (Tekton & Jenkins)
- [ ] **Tekton**:
  - Browse PipelineRuns and TaskRuns
  - Real-time log streaming for active steps
  - Rerun failed pipelines
- [ ] **Jenkins/GitHub Actions**:
  - View job status and build history
  - Download artifacts directly to local pane

### 7.4 Infrastructure as Code
- [ ] **Crossplane**:
  - Browse Managed Resources and Compositions
  - Status checking for infrastructure provisioning
- [ ] **Terraform/OpenTofu**:
  - View state file content (if stored in K8s Secrets/S3)
  - Resource dependency visualization

---

## Phase 8: UI/UX Enhancements

### 8.1 Search & Navigation
- [ ] Fuzzy file search (Ctrl+P / Ctrl+F)
- [ ] Regex filtering with live preview
- [ ] Search history persistence
- [ ] Quick navigation to recent locations
- [ ] Breadcrumb navigation with dropdown

### 8.2 File Viewer & Editor
- [ ] Built-in text editor (syntax highlighting)
- [ ] Hex viewer for binary files
- [ ] Image preview (ASCII art / Sixel / iTerm2 protocol)
- [ ] Log viewer with filtering & tail mode
- [ ] File comparison tool (diff view)

### 8.3 Themes & Customization
- [ ] Multiple built-in themes (dark, light, neon, nord)
- [ ] Customizable color schemes (TOML config)
- [ ] Nerd Font icon support
- [ ] Status bar customization
- [ ] Key binding customization (Vim-style vs Emacs-style)

### 8.4 Additional UI Features
- [ ] Tab support (multiple directory panes)
- [ ] Bookmarks/favorites
- [ ] Operation history & undo
- [ ] Copy path shortcuts (Ctrl+C)
- [ ] Detailed info panel (properties, stats)

### 8.5 Desktop Integration
- [ ] System clipboard support (copy paths/content)
- [ ] Desktop notifications for long-running transfers
- [ ] "Open With" functionality (download temp & open in local GUI app)
- [ ] Drag & Drop support (terminal emulator dependent)

---

## Phase 9: Developer & API Features

### 9.1 REST API & Services
- [ ] GraphQL/REST API for remote operations
- [ ] WebSocket support for real-time updates
- [ ] Docker containerization (expose API on port)
- [ ] Reverse proxy integration (nginx, traefik)

### 9.2 CLI & Scripting
- [ ] Command-line tool for scripting (e.g., `abyss cp s3://bucket/file k8s://pvc/path`)
- [ ] Batch operations support
- [ ] Configuration via environment variables
- [ ] Exit codes for automation

### 9.3 Plugin Architecture
- [ ] WASM plugin support for custom backends
- [ ] Lua scripting for custom operations
- [ ] Hook system (pre/post transfer, etc.)
- [ ] Community plugin registry

---

## Phase 10: Enterprise Features

### 10.1 Multi-User & RBAC
- [ ] User accounts with authentication
- [ ] Role-based access control (RBAC)
- [ ] Per-user operation quotas
- [ ] Shared workspaces & collaboration
- [ ] Session management

### 10.2 Advanced Monitoring
- [ ] Prometheus metrics export
- [ ] Health checks & status pages
- [ ] Performance profiling
- [ ] Resource usage tracking
- [ ] Alert system (email, Slack, PagerDuty)

### 10.3 Disaster Recovery
- [ ] Backup scheduling with retention
- [ ] Point-in-time recovery
- [ ] Cross-region replication
- [ ] Disaster recovery testing
- [ ] RTO/RPO configuration

---

## Phase 11: AI & Intelligent Features
**Goal:** Smart assistance and automation

### 11.1 Natural Language Interface
- [ ] Command palette with NLP (e.g., "Find all error logs from yesterday and copy them to S3")
- [ ] Smart filtering ("Show me only large images")

### 11.2 Content Analysis
- [ ] Auto-summarization of text/log files
- [ ] Anomaly detection in log streams
- [ ] Smart tagging of files based on content

---

## Technical Debt & Ongoing Improvements

- [ ] **Priority: Extract VFS abstraction from current `RemoteFs` implementation**
- [ ] Comprehensive error handling with context (anyhow/thiserror)
- [ ] Structured logging (tracing crate integration)
- [ ] Event bus refactoring for extensibility
- [ ] Modular UI rendering system (Component trait)
- [ ] Performance benchmarks & profiling
- [ ] Security audit & penetration testing
- [ ] Documentation: user guide, API docs, plugin development

---

## Implementation Priority & Dependencies

| Priority | Feature | Phase | Dependencies | Est. Effort |
|----------|---------|-------|--------------|------------|
| 1 | VFS Abstraction | 1 | None | 2 weeks |
| 2 | Config System | 1 | serde, toml | 1 week |
| 3 | S3 Backend | 2 | aws-sdk-s3 | 3 weeks |
| 4 | GCS Backend | 2 | google-cloud | 3 weeks |
| 5 | Sync Core | 3 | notify, tokio-time | 4 weeks |
| 6 | Encryption | 4 | age, aes-gcm | 3 weeks |
| 7 | Parallel Transfers | 5 | tokio, rayon | 2 weeks |
| 8 | ArgoCD Support | 7 | kube | 3 weeks |
| 9 | Robust K8s Agent | 6 | rust-embed | 3 weeks |
| 10 | Multi-User | 10 | sqlx, argon2 | 6 weeks |

---

## Success Metrics

- [ ] Support for 8+ storage backends
- [ ] Sub-second UI response (even with 100k+ files)
- [ ] Zero data loss in sync operations
- [ ] 1000+ GitHub stars
- [ ] 50+ community contributions
- [ ] 5+ enterprise pilot customers
- [ ] 99.99% uptime in service mode
- [ ] <5% memory footprint on typical operations