#[cfg(feature = "tokio")]
use std::collections::HashMap;
#[cfg(feature = "kubernetes")]
use std::ffi::OsString;
use std::sync::atomic::AtomicU64;
#[cfg(feature = "tokio")]
use std::sync::atomic::Ordering as AtomicOrdering;

#[cfg(feature = "tokio")]
use crate::browser::scanner::local::os_string_from_external;
use crate::browser::types::BrowserEntry;
#[cfg(feature = "tokio")]
use crate::browser::types::BrowserKind;
#[cfg(feature = "tokio")]
use crate::storage::EntryKind as StorageEntryKind;
use crate::storage::{RemoteLocation, StorageRuntime};

#[cfg(feature = "tokio")]
pub(crate) fn read_remote_directory(
    location: &RemoteLocation,
    storage: &StorageRuntime,
    generation: u64,
    latest: &AtomicU64,
    emit: impl Fn(Vec<BrowserEntry>) -> bool,
) -> Result<Vec<BrowserEntry>, String> {
    let backend = storage
        .backend(location)
        .map_err(|error| error.to_string())?;
    storage.block_on(async {
        let mut continuation = None;
        let mut entries = HashMap::new();
        let mut ordinal = 1_u64;
        loop {
            if latest.load(AtomicOrdering::Acquire) != generation {
                return Ok(entries.into_values().collect());
            }
            let page = backend
                .list(&location.path, continuation.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            let mut batch = Vec::with_capacity(page.entries.len());
            for entry in page.entries {
                if entry.name.starts_with(b"._") {
                    continue;
                }
                let display_name = {
                    #[cfg(feature = "kubernetes")]
                    {
                        if location.scheme == "kube" {
                            kubernetes_usage_name(&entry.name, entry.version.as_deref())
                        } else {
                            os_string_from_external(entry.name.clone())
                        }
                    }
                    #[cfg(not(feature = "kubernetes"))]
                    {
                        os_string_from_external(entry.name.clone())
                    }
                };
                let kind = match entry.kind {
                    StorageEntryKind::Directory => BrowserKind::Directory,
                    StorageEntryKind::File => BrowserKind::File,
                    StorageEntryKind::Symlink => BrowserKind::Symlink,
                    StorageEntryKind::Other => BrowserKind::Other,
                };
                batch.push(BrowserEntry {
                    name: display_name,
                    raw_name: Some(entry.name),
                    kind,
                    size: entry.size,
                    modified: entry.modified,
                    mode: None,
                    ordinal,
                });
                ordinal = ordinal.saturating_add(1);
            }
            for entry in &batch {
                entries.insert(entry.name.clone(), entry.clone());
            }
            if !batch.is_empty() && !emit(batch) {
                return Ok(entries.into_values().collect());
            }
            continuation = page.continuation;
            if continuation.is_none() {
                break;
            }
        }
        Ok(entries.into_values().collect())
    })
}

#[cfg(not(feature = "tokio"))]
pub(crate) fn read_remote_directory(
    _location: &RemoteLocation,
    _storage: &StorageRuntime,
    _generation: u64,
    _latest: &AtomicU64,
    _emit: impl Fn(Vec<BrowserEntry>) -> bool,
) -> Result<Vec<BrowserEntry>, String> {
    Err("remote storage is disabled in this build; build with --features remote".to_owned())
}

#[cfg(feature = "kubernetes")]
pub(crate) fn kubernetes_usage_name(name: &[u8], version: Option<&str>) -> OsString {
    let base = os_string_from_external(name.to_vec());
    let Some(version) = version else {
        return base;
    };
    let fields = version.split('|').collect::<Vec<_>>();
    if fields.first() != Some(&"abyss-usage") {
        return base;
    }
    if fields.get(1) == Some(&"unavailable") {
        return format!("{}  [usage unavailable]", base.to_string_lossy()).into();
    }
    let Some(percent) = fields.get(1).and_then(|value| value.parse::<u64>().ok()) else {
        return base;
    };
    let used = fields
        .get(2)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let capacity = fields
        .get(3)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let inode_percent = fields
        .get(4)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let filled = (percent.min(100) / 10) as usize;
    let gauge = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
    let warning = if percent >= 85 { "⚠ " } else { "" };
    format!(
        "{warning}{}  [{gauge}] {percent}% {}/{}  inode {inode_percent}%",
        base.to_string_lossy(),
        compact_bytes(used),
        compact_bytes(capacity)
    )
    .into()
}

#[cfg(feature = "kubernetes")]
pub(crate) fn compact_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}
