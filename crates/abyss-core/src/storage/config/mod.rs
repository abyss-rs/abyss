mod connection;
mod file;
#[cfg(feature = "kubernetes")]
mod kubernetes;

#[cfg(test)]
mod tests;

pub use self::connection::CredentialSource;
#[cfg(feature = "gcs")]
pub use self::connection::GcsConnection;
#[cfg(feature = "sftp")]
pub use self::connection::SftpConnection;
#[cfg(feature = "azure")]
pub use self::connection::{AzureConnection, AzureCredentialSource, AzureMode};
#[cfg(feature = "ftp")]
pub use self::connection::{FtpConnection, FtpMode};
#[cfg(feature = "s3")]
pub use self::connection::{S3Connection, S3Preset};
pub use self::file::{Connection, ConnectionConfig, NamedConnection};
#[cfg(feature = "kubernetes")]
pub use self::kubernetes::{
    KubernetesConnection, KubernetesHelperImage, KubernetesImagePullPolicy,
};
