# abyss-tui

[![Crates.io](https://img.shields.io/crates/v/abyss-tui.svg)](https://crates.io/crates/abyss-tui)
[![Documentation](https://docs.rs/abyss-tui/badge.svg)](https://docs.rs/abyss-tui)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

Fast, modern dual-pane terminal file manager for local, cloud, and Kubernetes storage.

## Installation

```bash
cargo install abyss-tui --locked
```

## Features

- **Dual-Pane & Multi-Tab Interface**: Independent dual panes with multi-tab browsing, persistent sessions, synchronized scrolling, and directory difference inspection.
- **Cross-Platform Storage**: Browse and manage files seamlessly across local drives, S3, Azure, GCS, SFTP, FTP, and Kubernetes PVCs.
- **Integrated Tools**: Built-in disk analyzer (Cleaner), cryptographic hasher (QuicHash), archive manager, and syntax-highlighted hex/text viewer.
- **Delta Sync**: Differential bandwidth-efficient transfers with BLAKE3 delta signatures.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE).
