//! Provider-neutral storage interfaces.
//!
//! The TUI and job engine depend on this module instead of cloud SDK types.

#[cfg(feature = "azure")]
mod azure;
mod config;
mod discovery;
#[cfg(feature = "ftp")]
mod ftp;
#[cfg(feature = "gcs")]
mod gcs;
#[cfg(feature = "kubernetes")]
mod helper_protocol;
#[cfg(feature = "kubernetes")]
mod kubernetes;
mod location;
mod registry;
pub mod resume;
#[cfg(feature = "tokio")]
mod runtime;
#[cfg(feature = "s3")]
mod s3;
#[cfg(feature = "sftp")]
mod sftp;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(feature = "tokio")]
use async_trait::async_trait;
#[cfg(feature = "tokio")]
use bytes::Bytes;
#[cfg(feature = "tokio")]
use futures_util::Stream;

#[cfg(feature = "azure")]
pub use azure::AzureFactory;
#[cfg(feature = "gcs")]
pub use config::GcsConnection;
#[cfg(feature = "sftp")]
pub use config::SftpConnection;
#[cfg(feature = "azure")]
pub use config::{AzureConnection, AzureCredentialSource, AzureMode};
pub use config::{Connection, ConnectionConfig, CredentialSource, NamedConnection};
#[cfg(feature = "ftp")]
pub use config::{FtpConnection, FtpMode};
#[cfg(feature = "kubernetes")]
pub use config::{KubernetesConnection, KubernetesHelperImage, KubernetesImagePullPolicy};
#[cfg(feature = "s3")]
pub use config::{S3Connection, S3Preset};
pub use discovery::{DiscoveryEnvironment, StorageSource, discover_sources};
#[cfg(feature = "ftp")]
pub use ftp::FtpFactory;
#[cfg(feature = "gcs")]
pub use gcs::GcsFactory;
#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesFactory;
pub use location::{Location, LocationCodec, RemoteLocation, StoragePath};
pub use registry::{ProviderDescriptor, ProviderField, ProviderRegistry};
#[cfg(feature = "tokio")]
pub use runtime::StorageRuntime;
#[cfg(feature = "s3")]
pub use s3::S3Factory;
#[cfg(feature = "sftp")]
pub use sftp::SftpFactory;

#[cfg(not(feature = "tokio"))]
#[derive(Clone, Debug, Default)]
pub struct StorageRuntime;

#[cfg(not(feature = "tokio"))]
impl StorageRuntime {
    pub fn load_default() -> Result<Arc<Self>, StorageError> {
        Ok(Arc::new(Self))
    }

    pub fn refresh_sources(&self) -> Vec<StorageSource> {
        vec![StorageSource::local()]
    }

    pub fn shutdown(&self) -> Result<(), StorageError> {
        Ok(())
    }

    pub fn backend(
        &self,
        _location: &RemoteLocation,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "remote storage is disabled in this build; build with --features remote",
        ))
    }

    pub fn block_on<F: std::future::Future>(&self, _future: F) -> F::Output {
        panic!("remote storage is disabled in this build; build with --features remote")
    }
}

#[cfg(feature = "tokio")]
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, StorageError>> + Send + 'static>>;
#[cfg(not(feature = "tokio"))]
pub type ByteStream = Pin<Box<dyn std::any::Any + Send + 'static>>;

pub type BackendFuture =
    Pin<Box<dyn Future<Output = Result<Arc<dyn StorageBackend>, StorageError>> + Send>>;
pub type WireProgress = Arc<dyn Fn(u64) + Send + Sync + 'static>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
pub struct StorageEntry {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ListPage {
    pub entries: Vec<StorageEntry>,
    pub continuation: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendCapabilities {
    pub read_only: bool,
    pub paginated_list: bool,
    pub range_read: bool,
    pub multipart_write: bool,
    pub create_dir: bool,
    pub recursive_delete: bool,
    pub server_side_copy: bool,
    pub atomic_rename: bool,
    pub conditional_write: bool,
    pub checksum: bool,
    pub real_directories: bool,
    pub bulk_tree_read: bool,
    pub bulk_tree_write: bool,
    pub server_side_tree_copy: bool,
    pub volume_snapshot: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeEntry {
    /// Path relative to the root supplied to the bulk operation.
    pub path: Vec<Vec<u8>>,
    pub kind: EntryKind,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeWriteEntry {
    pub entry: TreeEntry,
    pub overwrite: bool,
    /// Existing path, relative to the same destination root, whose verified
    /// contents can be copied locally instead of crossing the transport.
    pub clone_from: Option<Vec<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeState {
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Authentication,
    PermissionDenied,
    NotFound,
    AlreadyExists,
    Conflict,
    InvalidInput,
    Unsupported,
    RateLimited,
    Timeout,
    Transport,
    Cancelled,
    Other,
}

#[derive(Clone, Debug)]
pub struct StorageError {
    pub kind: ErrorKind,
    pub message: String,
    pub retryable: bool,
    pub request_id: Option<String>,
}

impl StorageError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
            request_id: None,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(request_id) = &self.request_id {
            write!(formatter, " (request {request_id})")?;
        }
        Ok(())
    }
}

impl std::error::Error for StorageError {}

#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    pub offset: Option<u64>,
    pub length: Option<u64>,
    pub expected_version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    pub size: Option<u64>,
    pub overwrite: bool,
    pub expected_version: Option<String>,
}

#[cfg(feature = "tokio")]
#[async_trait]
pub trait StorageBackend: Send + Sync {
    fn connection_id(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;

    /// Verify that credentials and the provider API are usable without
    /// changing provider state.
    async fn probe(&self) -> Result<(), StorageError> {
        self.list(&StoragePath::Remote(String::new()), None)
            .await
            .map(|_| ())
    }

    async fn list(
        &self,
        path: &StoragePath,
        continuation: Option<&str>,
    ) -> Result<ListPage, StorageError>;

    async fn stat(&self, path: &StoragePath) -> Result<StorageEntry, StorageError>;

    async fn read(
        &self,
        path: &StoragePath,
        options: ReadOptions,
    ) -> Result<ByteStream, StorageError>;

    async fn write(
        &self,
        path: &StoragePath,
        source: ByteStream,
        options: WriteOptions,
    ) -> Result<(), StorageError>;

    async fn create_dir(&self, path: &StoragePath) -> Result<(), StorageError>;

    async fn delete(&self, path: &StoragePath, recursive: bool) -> Result<(), StorageError>;

    async fn copy(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        overwrite: bool,
    ) -> Result<(), StorageError>;

    async fn rename(
        &self,
        source: &StoragePath,
        destination: &StoragePath,
        overwrite: bool,
    ) -> Result<(), StorageError>;

    async fn list_tree(&self, _root: &StoragePath) -> Result<Vec<TreeEntry>, StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "bulk tree listing is not supported",
        ))
    }

    async fn inspect_tree(
        &self,
        _root: &StoragePath,
        _entries: &[TreeEntry],
    ) -> Result<Vec<Option<TreeState>>, StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "bulk tree inspection is not supported",
        ))
    }

    async fn read_tree(
        &self,
        _root: &StoragePath,
        _entries: Vec<TreeEntry>,
        _wire_progress: Option<WireProgress>,
    ) -> Result<ByteStream, StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "bulk tree reads are not supported",
        ))
    }

    async fn write_tree(
        &self,
        _root: &StoragePath,
        _entries: Vec<TreeWriteEntry>,
        _source: ByteStream,
        _wire_progress: Option<WireProgress>,
    ) -> Result<(), StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "bulk tree writes are not supported",
        ))
    }

    async fn copy_tree(
        &self,
        _source: &StoragePath,
        _destination: &StoragePath,
        _entries: Vec<TreeWriteEntry>,
    ) -> Result<(), StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "server-side tree copies are not supported",
        ))
    }

    async fn create_snapshot(&self, _path: &StoragePath) -> Result<String, StorageError> {
        Err(StorageError::new(
            ErrorKind::Unsupported,
            "snapshots are not supported by this storage backend",
        ))
    }

    /// Release provider-side session resources. Most object stores need no
    /// action; backends such as Kubernetes use this to remove helper pods
    /// before the process exits.
    async fn shutdown(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

#[cfg(not(feature = "tokio"))]
pub trait StorageBackend: Send + Sync {
    fn connection_id(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;
    fn create_dir<'a>(
        &'a self,
        _path: &'a StoragePath,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(std::future::ready(Err(StorageError::new(
            ErrorKind::Unsupported,
            "remote storage is disabled in this build; build with --features remote",
        ))))
    }
}

#[cfg(feature = "tokio")]
pub trait StorageProviderFactory: Send + Sync {
    fn descriptor(&self) -> &'static ProviderDescriptor;
    fn create(&self, id: String, connection: Connection) -> BackendFuture;
}

#[cfg(not(feature = "tokio"))]
pub trait StorageProviderFactory: Send + Sync {
    fn descriptor(&self) -> &'static ProviderDescriptor;
    fn create(&self, id: String, connection: Connection) -> BackendFuture {
        let _ = (id, connection);
        Box::pin(std::future::ready(Err(StorageError::new(
            ErrorKind::Unsupported,
            "remote storage is disabled in this build",
        ))))
    }
}
