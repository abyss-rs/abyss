# Abyss

A fast, crossplatform dual-pane file manager for local, cloud, and Kubernetes storage.

MacOs, Linux, Windows x64/arm64

## Capabilities & Features

- **Core File Operations**: Ultra-fast copy, move, rename, directory creation, and safe recycling (macOS Trash, Windows Recycle Bin, Freedesktop Trash).
- **APFS & OS-Native Acceleration**: Instant APFS `clonefile` / `fcopyfile` zero-copy clones, AppleDouble (`._*`) metadata isolation, Linux `copy_file_range`, and Windows `IoCopyFile`.
- **BLAKE3 SIMD Delta Sync Engine**:
  - Multi-core **Rayon** + **BLAKE3 SIMD** parallel signature generation and 1-byte sliding-window `Rollsum` matching.
  - Generates compact delta patches (`ABDEL1`) with up to 97%+ bandwidth savings and byte-for-byte cryptographic verification.
  - Interactive preview and execution of differential sync strategies: **Update Only**, **Mirror**, **Two-Way**, and **Delta Patchable**.
- **Cryptographic Hashing & QuicHash Standard**:
  - Native verification and creation of **BLAKE3**, **SHA-256**, **MD5**, **SHA-1**, **QuicHash standard**, and **Hashdeep** database checksums with foreground and background jobs.
- **Comprehensive Archive Suite (25+ Formats)**:
  - **Creation**: Interactive Auto, 7z, ZIP (AES-256), and TAR creation with format-specific codecs, levels, threads, solid-mode controls, and Tar.zst embedded Table of Contents (TOC) for instant random-access member reading.
  - **Browsing & Extraction**: Archive VFS with stacked nested archives. Supports every [unarc-rs](https://crates.io/crates/unarc-rs) format (**ace**, **arc/pak**, **arj**, **zoo**, **sq/sqz**, **Z**, **gz**, **bz2**, **ice/pack-ice**, **hyp**, **ha**, **lha/lzh**, **zip**, **rar**, **7z**, **tar**, **tar.gz/tgz**, **tar.bz2/tbz**, **tar.Z**, **uc2/ue2**) plus Abyss native codecs (**xz**, **zstd**, **lz4**, **lzip**, **brotli**).
- **High-Speed Kubernetes PVC Transfers & Snapshots**:
  - Compressed chunked streaming directly to/from Kubernetes PVCs via automated helper pods.
  - Direct creation, status tracking, and restore of Kubernetes CSI `VolumeSnapshots`.
- **Cross-Provider Storage**: Seamless operations between local disks, cloud buckets (S3/Azure/GCS), remote servers (SFTP/FTP), and Kubernetes storage.
- **Dual-Pane & Tabbed Interface**: Independent dual panes with full **multi-tab support** per pane (`Ctrl+T` new tab, `Ctrl+W` close tab, `[` / `]` cycle tabs, header indicators `[ 1/3 ]`), persistent across sessions, directory difference highlighting (`[DIFF]`), synchronized scrolling (`[SYNC]`), and natural alphanumeric/episode sorting.
- **Integrated File & Media Inspector**: Real-time audio/media tag inspection (ID3/FLAC/OGG/WAV/MP4), hex/text viewer, and editor integration.
- **Bookmarks & Fast Navigation**: Persistent bookmarks (`Ctrl+1..9`), fuzzy directory history (`Alt+H`), and Zoxide / Autojump integration (`Alt+J`).
- **Disk Analyze**: `4` launches the embedded cleaner disk-usage analyzer; `Esc`/`q` returns to dual-pane browsing.

## Workspace

- `abyss-core` contains the frontend-neutral browser, file operations, archives, hashing, jobs, sync, workspace persistence, and storage providers.
- `abyss-tui` builds the `abyss` terminal application.

From the repository root, build the TUI (local-only):

```sh
cargo build -p abyss-tui --bin abyss
```

Or enable individual providers, for example:

```sh
cargo build -p abyss-tui --bin abyss --features s3,sftp
```

## Supported Storage Providers

- Local Filesystem (always available)
- AWS S3 & S3-Compatible Storage (`s3`)
- Azure Blob & ADLS Gen2 (`azure`)
- Google Cloud Storage / GCS (`gcs`)
- Kubernetes PersistentVolumeClaims (`kubernetes`)
- SFTP (`sftp`), FTP/FTPS (`ftp`)

## License

Dual-licensed under [MIT](licenses/LICENSE-MIT) or [Apache-2.0](licenses/LICENSE-APACHE).

Archive creation uses [Zstandard](https://facebook.github.io/zstd/) (libzstd) under the BSD license; see [NOTICE](licenses/NOTICE) and [LICENSE-ZSTD](licenses/LICENSE-ZSTD).
