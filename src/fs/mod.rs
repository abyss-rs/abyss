pub mod backend;
pub mod copy;
pub mod gcs;
pub mod local;
pub mod remote;
pub mod s3;
pub mod selecting;
pub mod types;

pub use backend::{BackendType, StorageBackend};
pub use copy::copy_between_backends;
pub use local::{LocalBackend, LocalFs};
pub use remote::{K8sBackend, RemoteFs};
pub use selecting::SelectingBackend;
pub use types::*;
