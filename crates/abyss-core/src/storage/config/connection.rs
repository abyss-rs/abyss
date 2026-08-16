use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialSource {
    #[default]
    DefaultChain,
    AwsProfile {
        profile: String,
    },
    AzureDeveloperTools,
    GoogleAdc {
        credential_path: Option<PathBuf>,
    },
    Kubeconfig,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(feature = "s3")]
pub enum S3Preset {
    Aws,
    CloudflareR2,
    DigitalOceanSpaces,
    BackblazeB2,
    Wasabi,
    Minio,
    CephRgw,
    Custom,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "s3")]
pub struct S3Connection {
    pub preset: S3Preset,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub force_path_style: Option<bool>,
    #[serde(default)]
    pub buckets: Vec<String>,
    #[serde(default)]
    pub disable_payload_signing: bool,
    #[serde(default)]
    pub disable_checksums: bool,
    #[serde(default)]
    pub disable_multipart: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(feature = "azure")]
pub enum AzureMode {
    Blob,
    AdlsGen2,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(feature = "azure")]
pub enum AzureCredentialSource {
    #[default]
    DeveloperTools,
    WorkloadIdentity,
    ManagedIdentity,
    ClientSecretEnvironment,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "azure")]
pub struct AzureConnection {
    pub mode: AzureMode,
    pub account: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub credential: AzureCredentialSource,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "gcs")]
pub struct GcsConnection {
    pub project: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub credential_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "sftp")]
pub struct SftpConnection {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub private_key: Option<PathBuf>,
    #[serde(default)]
    pub password_env: Option<String>,
    /// Executable followed by arguments. It must print only the password.
    #[serde(default)]
    pub password_command: Vec<String>,
    #[serde(default)]
    pub known_hosts: Option<PathBuf>,
    #[serde(default)]
    pub accept_new_host_keys: bool,
}

#[cfg(feature = "sftp")]
const fn default_ssh_port() -> u16 {
    22
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(feature = "ftp")]
pub enum FtpMode {
    #[default]
    Plain,
    ExplicitTls,
    ImplicitTls,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "ftp")]
pub struct FtpConnection {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default = "default_ftp_username")]
    pub username: String,
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub mode: FtpMode,
}

#[cfg(feature = "ftp")]
fn default_ftp_username() -> String {
    "anonymous".to_owned()
}
