use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use tempfile::NamedTempFile;
use unarc_rs::unified::ArchiveFormat as UnifiedFormat;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveContainer {
    Auto,
    SevenZip,
    Zip,
    Tar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionMethod {
    Store,
    Zstd,
    Gzip,
    Xz,
    Bzip2,
    Lz4,
    Brotli,
    Lzma2,
    Lzma,
    Ppmd,
    Deflate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionPreset {
    Fast,
    Balanced,
    Maximum,
    Ultra,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionThreads {
    Auto,
    Count(u8),
}

#[derive(Clone)]
pub struct ArchiveCreateOptions {
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    pub buffer_capacity: usize,
    pub container: ArchiveContainer,
    pub method: CompressionMethod,
    pub preset: CompressionPreset,
    pub level: u8,
    pub threads: CompressionThreads,
    pub solid: bool,
    pub password: Option<Zeroizing<String>>,
}

impl ArchiveCreateOptions {
    #[cfg(test)]
    pub fn zstd_default(
        sources: Vec<PathBuf>,
        destination: PathBuf,
        buffer_capacity: usize,
    ) -> Self {
        Self {
            sources,
            destination,
            buffer_capacity,
            container: ArchiveContainer::Auto,
            method: CompressionMethod::Zstd,
            preset: CompressionPreset::Balanced,
            level: 3,
            threads: CompressionThreads::Auto,
            solid: false,
            password: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveFormat {
    Unified(UnifiedFormat),
    Rar,
    TarXz,
    TarLzip,
    TarZstd,
    TarLz4,
    TarBrotli,
    Xz,
    Lzip,
    Zstd,
    Lz4,
    Brotli,
}

#[derive(Clone, Debug)]
pub struct ArchiveMember {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
}

#[derive(Clone, Debug)]
pub struct ArchiveIndex {
    pub source: PathBuf,
    pub format: ArchiveFormat,
    pub members: Vec<ArchiveMember>,
}

#[derive(Clone)]
pub enum ArchiveRequest {
    Path {
        pane: usize,
        path: PathBuf,
    },
    Member {
        pane: usize,
        parent: Arc<ArchiveIndex>,
        member: String,
        parent_password: Option<Zeroizing<String>>,
        display_name: String,
        try_archive: bool,
    },
}

impl ArchiveRequest {
    pub fn pane(&self) -> usize {
        match self {
            Self::Path { pane, .. } | Self::Member { pane, .. } => *pane,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Path { path, .. } => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            Self::Member { display_name, .. } => display_name.clone(),
        }
    }
}

pub enum ArchiveLoadResult {
    Opened {
        request: ArchiveRequest,
        index: ArchiveIndex,
        temporary: Option<NamedTempFile>,
        password: Option<Zeroizing<String>>,
    },
    Viewer {
        request: ArchiveRequest,
        temporary: Option<NamedTempFile>,
        path: PathBuf,
    },
    Password {
        request: ArchiveRequest,
        invalid: bool,
        message: String,
    },
    Failed {
        message: String,
    },
}

pub struct ArchiveLoad {
    receiver: Receiver<ArchiveLoadResult>,
}

impl ArchiveLoad {
    pub fn start(request: ArchiveRequest, password: Option<Zeroizing<String>>) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = crate::archive::reader::load_request(request, password);
            let _ = sender.send(result);
        });
        Self { receiver }
    }

    pub fn try_recv(&self) -> Option<ArchiveLoadResult> {
        self.receiver.try_recv().ok()
    }
}

impl ArchiveIndex {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self, ArchiveOpenError> {
        crate::archive::reader::open_index(path, password)
    }

    pub fn member(&self, path: &str) -> Option<&ArchiveMember> {
        self.members.iter().find(|member| member.path == path)
    }
}

#[derive(Debug)]
pub enum ArchiveOpenError {
    NotArchive,
    PasswordRequired(String),
    InvalidPassword(String),
    Other(String),
}

impl std::fmt::Display for ArchiveOpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotArchive => formatter.write_str("not a supported archive"),
            Self::PasswordRequired(message)
            | Self::InvalidPassword(message)
            | Self::Other(message) => formatter.write_str(message),
        }
    }
}
