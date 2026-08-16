use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 3;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HelperRequest {
    pub version: u16,
    pub operation: HelperOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum HelperOperation {
    Capabilities,
    Usage,
    List {
        path: Vec<Vec<u8>>,
    },
    Stat {
        path: Vec<Vec<u8>>,
    },
    Read {
        path: Vec<Vec<u8>>,
        offset: u64,
        length: Option<u64>,
    },
    Write {
        path: Vec<Vec<u8>>,
        size: u64,
        overwrite: bool,
    },
    CreateDir {
        path: Vec<Vec<u8>>,
    },
    Delete {
        path: Vec<Vec<u8>>,
        recursive: bool,
    },
    Rename {
        source: Vec<Vec<u8>>,
        destination: Vec<Vec<u8>>,
        overwrite: bool,
    },
    ListTree {
        root: Vec<Vec<u8>>,
    },
    InspectTree {
        root: Vec<Vec<u8>>,
        entries: Vec<HelperTreeEntry>,
    },
    ReadTree {
        root: Vec<Vec<u8>>,
        entries: Vec<HelperTreeEntry>,
        #[serde(default)]
        compression: HelperCompression,
    },
    WriteTree {
        root: Vec<Vec<u8>>,
        entries: Vec<HelperTreeEntry>,
        #[serde(default)]
        compression: HelperCompression,
    },
    CopyTree {
        source: Vec<Vec<u8>>,
        destination: Vec<Vec<u8>>,
        entries: Vec<HelperTreeEntry>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub enum HelperCompression {
    None,
    #[default]
    Lz4,
    Brotli,
    Deflate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HelperTreeEntry {
    pub path: Vec<Vec<u8>>,
    pub kind: HelperEntryKind,
    pub size: u64,
    pub overwrite: bool,
    #[serde(default)]
    pub clone_from: Option<Vec<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum HelperEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HelperEntry {
    pub name: Vec<u8>,
    pub kind: HelperEntryKind,
    pub size: Option<u64>,
    pub modified_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum HelperResult {
    Capabilities {
        bulk_tree: bool,
        #[serde(default)]
        usage: bool,
    },
    Usage {
        capacity_bytes: u64,
        free_bytes: u64,
        total_inodes: u64,
        free_inodes: u64,
    },
    Ok,
    Entries(Vec<HelperEntry>),
    Entry(HelperEntry),
    Data {
        size: u64,
    },
    TreeEntries(Vec<HelperTreeEntry>),
    TreeStates(Vec<Option<HelperEntryKind>>),
    Error {
        kind: String,
        message: String,
    },
}
