# Abyss

A high-performance dual-pane TUI file manager for local filesystems, Kubernetes persistent volumes, and S3-compatible cloud storage. Built with Rust for speed and reliability.

## Features

### Core Functionality
- **Dual-pane interface** - Navigate and transfer files between any two storage backends
- **Multi-backend support** - Local filesystem, Kubernetes PV/PVC, S3, GCS, and more
- **File operations** - Copy, move, delete files and directories across backends
- **Background tasks** - Non-blocking operations with real-time progress tracking
- **Disk analyzer** - ncdu-style disk usage visualization

### Sync & Replication
- **Bidirectional sync** - Synchronize files between any two backends
- **Conflict resolution** - Configurable strategies (newest wins, source wins, manual)
- **Compression** - Optional gzip/zstd compression for transfers
- **Bandwidth throttling** - Rate limiting for network transfers
- **Exclude patterns** - .gitignore-style pattern matching
- **Multicore hashing** - BLAKE3 with rayon for parallel checksums
- **File watching** - Real-time change detection (notify crate)

## Installation

```bash
cargo build --release
./target/release/abyss
```

## Keyboard Controls

### Navigation
| Key | Action |
|-----|--------|
| Up/Down | Navigate file list |
| Enter | Open directory or select item |
| Backspace | Go to parent directory |
| Tab | Switch between left and right pane |

### File Operations
| Key | Action |
|-----|--------|
| F2 | Rename selected file/directory |
| F3 | View file contents (text/auto) |
| F4 | Edit file (Text editor) |
| F9 | Open disk analyzer (ncdu-style) |
| F5 | Copy selected file/directory to other pane |
| F6 | Move selected file/directory to other pane |
| F7 | Create new directory |
| F8 | Delete selected file/directory |
| Ctrl+F | Search files in current directory |

### Storage & Sync
| Key | Action |
|-----|--------|
| Ctrl+N | Change storage backend (Local/K8s/Cloud) |
| Ctrl+S | Toggle sync mode on/off |
| Ctrl+Y | Execute sync now (when sync enabled) |
| Ctrl+D | Dry-run sync (preview changes) |

### Other
| Key | Action |
|-----|--------|
| F2 | Show disk usage statistics |
| F4 | Open disk analyzer (ncdu-style) |
| q | Quit (works in all modes) |
| Ctrl+C | Quit |
| Esc | Cancel current operation/dialog |

## Environment Variables

### Local Filesystem
No configuration needed - works out of the box.

### Kubernetes
```bash
# Usually auto-detected from ~/.kube/config
export KUBECONFIG=/path/to/kubeconfig
```

### AWS S3
```bash
export AWS_ACCESS_KEY_ID=AKIAXXXXXXXXXXXXXXXX
export AWS_SECRET_ACCESS_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_REGION=us-east-1
export S3_BUCKET=my-bucket
```

**IAM Role Support**

When running on AWS infrastructure (EC2, ECS, EKS, Lambda), the application automatically uses IAM roles for authentication. No credentials needed:

```bash
# On EC2/ECS/EKS - only specify region and bucket
export AWS_REGION=us-east-1
export S3_BUCKET=my-bucket
# AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are auto-detected from instance metadata
```

Ensure your EC2 instance profile or ECS task role has appropriate S3 permissions:
```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:ListBucket"],
    "Resource": ["arn:aws:s3:::my-bucket/*", "arn:aws:s3:::my-bucket"]
  }]
}
```

### DigitalOcean Spaces
```bash
export AWS_ACCESS_KEY_ID=DO00XXXXXXXXXXXXXXXX
export AWS_SECRET_ACCESS_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_REGION=nyc3
export S3_BUCKET=my-space
export S3_ENDPOINT=https://nyc3.digitaloceanspaces.com
```

### Hetzner Object Storage
```bash
export AWS_ACCESS_KEY_ID=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_SECRET_ACCESS_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_REGION=fsn1
export S3_BUCKET=my-bucket
export S3_ENDPOINT=https://fsn1.your-objectstorage.com
```

### Cloudflare R2
```bash
export AWS_ACCESS_KEY_ID=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_SECRET_ACCESS_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_REGION=auto
export S3_BUCKET=my-bucket
export S3_ENDPOINT=https://ACCOUNT_ID.r2.cloudflarestorage.com
```

### MinIO (Self-hosted)
```bash
export AWS_ACCESS_KEY_ID=minioadmin
export AWS_SECRET_ACCESS_KEY=minioadmin
export AWS_REGION=us-east-1
export S3_BUCKET=my-bucket
export S3_ENDPOINT=http://localhost:9000
```

### Google Cloud Storage
```bash
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
export GCS_BUCKET=my-bucket
```

**Workload Identity Support**

When running on GCP infrastructure (GCE, GKE, Cloud Run, Cloud Functions), the application automatically uses Workload Identity or the instance service account. No credentials file needed:

```bash
# On GCE/GKE/Cloud Run - only specify bucket
export GCS_BUCKET=my-bucket
# GOOGLE_APPLICATION_CREDENTIALS is auto-detected from instance metadata
```

For GKE with Workload Identity, bind your Kubernetes service account to a GCP service account:
```bash
# Create GCP service account
gcloud iam service-accounts create abyss-sa

# Grant storage permissions
gcloud projects add-iam-policy-binding PROJECT_ID \
  --member="serviceAccount:abyss-sa@PROJECT_ID.iam.gserviceaccount.com" \
  --role="roles/storage.objectAdmin"

# Bind to Kubernetes service account
gcloud iam service-accounts add-iam-policy-binding \
  abyss-sa@PROJECT_ID.iam.gserviceaccount.com \
  --role roles/iam.workloadIdentityUser \
  --member "serviceAccount:PROJECT_ID.svc.id.goog[NAMESPACE/KSA_NAME]"
```

### Wasabi
```bash
export AWS_ACCESS_KEY_ID=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_SECRET_ACCESS_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
export AWS_REGION=us-east-1
export S3_BUCKET=my-bucket
export S3_ENDPOINT=https://s3.us-east-1.wasabisys.com
```

## Sync Function

The sync engine provides bidirectional file synchronization with conflict resolution and compression.

### Enabling Sync

1. Press `Ctrl+S` to toggle sync mode on/off
2. When enabled, status bar shows "Sync: Idle"
3. Press `Ctrl+Y` to execute sync
4. Press `Ctrl+D` for dry-run (preview only)

### Sync Modes

**One-Way Sync** (default)
- Changes flow from source (active pane) to destination only
- Files in destination not in source are left alone

**Bidirectional Sync**
- Changes flow both ways
- Newer files overwrite older files
- Conflicts resolved based on strategy

**Mirror Mode**
- Destination becomes exact copy of source
- Extra files in destination are deleted

### Conflict Resolution Strategies

When the same file is modified in both locations:

| Strategy | Behavior |
|----------|----------|
| NewestWins | File with latest modification time wins |
| SourceWins | Source always overwrites destination |
| DestinationWins | Destination version is kept |
| Manual | Sync stops and prompts user |

### Sync Behavior

| Source State | Destination State | Action |
|--------------|-------------------|--------|
| File exists | File missing | Copy to destination |
| File newer | File older | Update destination |
| File older | File newer | Skip (or update based on mode) |
| File missing | File exists | Skip (or delete in mirror mode) |
| Files identical | Files identical | Skip |

### Advanced Features

**Compression**
- Gzip or Zstd compression for network transfers
- Configurable compression levels
- Automatic for remote backends

**Bandwidth Throttling**
- Rate limiting in bytes/second
- Prevents network saturation
- Uses token bucket algorithm (governor crate)

**Exclude Patterns**
- .gitignore-style glob patterns
- Default excludes: .git, node_modules, target, etc.
- Custom patterns via configuration

**Verification**
- BLAKE3 checksums verify transfers
- Multicore hashing with rayon
- Optional post-transfer verification

**Progress Reporting**
- Real-time progress in status bar
- Shows current file and percentage
- Non-blocking background execution

### Example Workflow

```
1. Start abyss
2. Left pane: Navigate to /home/user/project
3. Right pane: Press Ctrl+N, select S3, configure bucket
4. Press Ctrl+S to enable sync mode
5. Press Ctrl+Y to synchronize
6. Status bar shows: "Sync: 75% uploading file.txt"
7. On completion: "Sync: Complete - 42 files synced"
```

## Architecture

### Storage Backends

**LocalBackend** (`src/fs/local.rs`)
- Direct filesystem access via std::fs
- Fast directory scanning with jwalk
- Supports all Unix permissions and symlinks

**K8sBackend** (`src/fs/remote.rs`)
- Creates temporary helper pods with volume mounts
- File operations via kubectl exec
- Tar streaming for efficient transfers
- Automatic pod cleanup

**S3Backend** (`src/fs/s3.rs`)
- S3-compatible storage via OpenDAL
- Supports AWS, DigitalOcean, Hetzner, Cloudflare R2, MinIO, Wasabi
- Streaming uploads/downloads

**GcsBackend** (`src/fs/gcs.rs`)
- Google Cloud Storage via OpenDAL
- Service account authentication

### Sync Engine

**Core Components** (`src/sync/`)
- `engine.rs` - Main sync orchestration
- `conflict.rs` - Conflict detection and resolution
- `hash.rs` - BLAKE3 multicore hashing
- `compression.rs` - Gzip/Zstd compression
- `throttle.rs` - Bandwidth rate limiting
- `exclude.rs` - Pattern matching for excludes
- `watcher.rs` - File system change detection

**Sync Process**
1. Scan both source and destination
2. Build file state maps (path -> metadata)
3. Compare and generate sync actions
4. Resolve conflicts based on strategy
5. Execute actions with progress reporting
6. Verify transfers with checksums

### UI Components

**Dual-Pane Interface** (`src/ui/`)
- `pane.rs` - File list rendering
- `components.rs` - Reusable UI widgets (help bar, status bar, progress)
- Context-sensitive help bar
- Real-time progress tracking

## Technical Details

### Dependencies

**Core**
- `ratatui` - Terminal UI framework
- `crossterm` - Cross-platform terminal control
- `tokio` - Async runtime

**Storage**
- `kube` - Kubernetes client
- `opendal` - Unified storage access (S3/GCS)
- `jwalk` - Fast directory walking

**Sync**
- `blake3` - Fast cryptographic hashing (with rayon for multicore)
- `rayon` - Data parallelism
- `notify` - File system watching
- `flate2` - Gzip compression
- `zstd` - Zstandard compression
- `governor` - Rate limiting

### Performance Optimizations

- Multicore BLAKE3 hashing for files > 1MB
- Parallel directory scanning with rayon
- Non-blocking background tasks
- Streaming transfers (no full buffering)
- Efficient tar streaming for K8s transfers

## Requirements

- Rust 1.70 or later
- kubectl (for Kubernetes features)
- Valid kubeconfig (for Kubernetes)
- Cloud credentials (for S3/GCS features)

## License

Apache 2.0