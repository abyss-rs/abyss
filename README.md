# Abyss

A dual-pane TUI file manager for Kubernetes persistent volumes, inspired by Midnight Commander.

## Features

- **Dual-pane interface** - Navigate local filesystem and K8s storage side-by-side
- **Bidirectional file transfer** - Copy files between local and Kubernetes PVs/PVCs
- **Delete operations** - Remove files and directories remotely
- **PV and PVC support** - Access both PersistentVolumes and PersistentVolumeClaims
- **Keyboard-driven** - Efficient navigation with familiar MC-style shortcuts
- **Progress tracking** - Live progress for file operations with background tasks

## Requirements

- Rust 1.70+
- kubectl access to Kubernetes cluster
- Pod creation permissions in target namespaces

## Installation & Usage

```bash
cargo build --release
./target/release/skipper
```

## Controls

| Key | Action |
|-----|--------|
| ↑/↓ | Navigate |
| Enter | Open directory |
| Backspace | Parent directory |
| Tab | Switch panes |
| Ctrl+N | Select PVC/PV |
| F5 | Copy file/directory |
| F8 | Delete file/directory |
| F10/Ctrl+C | Quit |

## Architecture

**Local FS** (std::fs) ↔ **K8s Storage** (temporary helper pods)

- LocalFs: Direct filesystem access with directory caching
- RemoteFs: File operations via temporary pods with tar streaming
- K8sClient: Manages pod lifecycle and storage discovery
- UI: Ratatui-based dual-pane interface with status tracking

## How It Works

1. Select namespace and PVC/PV via `Ctrl+N`
2. Temporary pod spawned with volume mounted at `/data`
3. File operations executed in pod (cp, rm, ls)
4. Transfers use tar streams for efficiency
5. Pods cleaned up on exit or connection change

## License

Apache 2.0
