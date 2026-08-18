# abyss-core

[![Crates.io](https://img.shields.io/crates/v/abyss-core.svg)](https://crates.io/crates/abyss-core)
[![Documentation](https://docs.rs/abyss-core/badge.svg)](https://docs.rs/abyss-core)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

Frontend-neutral file, archive, hash, sync, and storage engine for Abyss.

## Features

- **Multi-Cloud & Remote VFS**: Unified asynchronous abstraction over local filesystems, S3, Azure Blob, Google Cloud Storage, FTP, and Kubernetes PVC storage.
- **Delta Sync Engine**: Fast rolling-checksum differential synchronization using BLAKE3 SIMD.
- **Archive VFS**: Unified archive inspection, navigation, and extraction across 25+ archive formats.
- **Zero-Copy Acceleration**: Platform-native copy-on-write acceleration (macOS `clonefile`, Linux `copy_file_range`, Windows `IoCopyFile`).
- **Cryptographic Hashing**: Multi-threaded foreground and background hash generation and manifest verification.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE).
