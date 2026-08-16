use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use crate::archive::ArchiveIndex;
use crate::storage::{Location, StorageSource};

#[derive(Default)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.rotate_left(5) ^ (byte as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
}

pub type FastHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserKind {
    Parent,
    Directory,
    File,
    Symlink,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortMode {
    Hybrid,
    Name,
    Extension,
    Size,
    Modified,
    Unsorted,
}

impl SortMode {
    pub const ALL: [Self; 6] = [
        Self::Hybrid,
        Self::Name,
        Self::Extension,
        Self::Size,
        Self::Modified,
        Self::Unsorted,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hybrid => "Hybrid",
            Self::Name => "Name",
            Self::Extension => "Extension",
            Self::Size => "Size",
            Self::Modified => "Modified",
            Self::Unsorted => "Unsorted",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SortSpec {
    pub mode: SortMode,
    pub reverse: bool,
    pub directories_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            mode: SortMode::Hybrid,
            reverse: false,
            directories_first: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BrowserEntry {
    pub name: OsString,
    pub(crate) raw_name: Option<Vec<u8>>,
    pub kind: BrowserKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    #[cfg_attr(windows, allow(dead_code))]
    pub mode: Option<u32>,
    pub(crate) ordinal: u64,
}

impl BrowserEntry {
    pub(crate) fn parent() -> Self {
        Self {
            name: OsString::from(".."),
            raw_name: None,
            kind: BrowserKind::Parent,
            size: None,
            modified: None,
            mode: None,
            ordinal: 0,
        }
    }

    pub fn unknown(name: OsString, ordinal: u64) -> Self {
        Self {
            name,
            raw_name: None,
            kind: BrowserKind::Unknown,
            size: None,
            modified: None,
            mode: None,
            ordinal,
        }
    }

    pub fn is_markable(&self) -> bool {
        self.kind != BrowserKind::Parent
    }

    pub(crate) fn component_bytes(&self) -> &[u8] {
        self.raw_name
            .as_deref()
            .unwrap_or_else(|| self.name.as_encoded_bytes())
    }
}

#[derive(Debug)]
pub enum BrowserEvent {
    DirectoryChunk {
        pane: usize,
        generation: u64,
        path: Location,
        entries: Vec<BrowserEntry>,
    },
    DirectoryComplete {
        pane: usize,
        generation: u64,
        path: Location,
        sort: SortSpec,
        result: Result<Vec<BrowserEntry>, String>,
    },
    Resolved {
        token: u64,
        path: PathBuf,
        result: Result<BrowserKind, String>,
    },
    SourcesDiscovered {
        pane: usize,
        generation: u64,
        sources: Vec<StorageSource>,
    },
    SourceProbed {
        pane: usize,
        generation: u64,
        source_id: String,
        result: Result<(), String>,
    },
}

pub(crate) enum DirectoryRequest {
    Load {
        pane: usize,
        generation: u64,
        path: Location,
        sort: SortSpec,
    },
}

pub(crate) struct ResolveRequest {
    pub(crate) token: u64,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceProbeStatus {
    Checking,
    Ready,
    Unavailable(String),
}

impl SourceProbeStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Checking => "Checking…",
            Self::Ready => "Ready",
            Self::Unavailable(_) => "Unavailable",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceEntry {
    pub source: StorageSource,
    pub status: SourceProbeStatus,
}

#[derive(Clone, Debug)]
pub struct SourceView {
    pub entries: Vec<SourceEntry>,
    pub selected: usize,
    pub offset: usize,
    pub generation: u64,
}

pub(crate) struct ArchiveLayer {
    pub(crate) index: Arc<ArchiveIndex>,
    pub(crate) temporary: Option<Arc<NamedTempFile>>,
    pub(crate) password: Option<Zeroizing<String>>,
    pub(crate) return_directory: String,
    pub(crate) return_name: OsString,
    pub(crate) display_name: String,
}
