use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[cfg(feature = "azure")]
use super::connection::AzureConnection;
#[cfg(feature = "ftp")]
use super::connection::FtpConnection;
#[cfg(feature = "gcs")]
use super::connection::GcsConnection;
#[cfg(feature = "s3")]
use super::connection::S3Connection;
#[cfg(feature = "sftp")]
use super::connection::SftpConnection;
#[cfg(feature = "kubernetes")]
use super::kubernetes::KubernetesConnection;
use crate::storage::{ErrorKind, StorageError};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "provider", rename_all = "kebab-case")]
pub enum Connection {
    #[cfg(feature = "s3")]
    S3(S3Connection),
    #[cfg(feature = "azure")]
    Azure(AzureConnection),
    #[cfg(feature = "gcs")]
    Gcs(GcsConnection),
    #[cfg(feature = "kubernetes")]
    Kubernetes(KubernetesConnection),
    #[cfg(feature = "sftp")]
    Sftp(SftpConnection),
    #[cfg(feature = "ftp")]
    Ftp(FtpConnection),
    /// Present when the config mentions a provider that is not compiled in.
    #[serde(other)]
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConnectionConfig {
    #[serde(default = "config_version")]
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<NamedConnection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct NamedConnection {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub connection: Connection,
}

const fn config_version() -> u32 {
    1
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            version: config_version(),
            connections: Vec::new(),
        }
    }
}

impl ConnectionConfig {
    pub fn default_path() -> Result<PathBuf, StorageError> {
        let directories = ProjectDirs::from("", "", "Abyss").ok_or_else(|| {
            StorageError::new(
                ErrorKind::Other,
                "could not determine the macOS Application Support directory",
            )
        })?;
        Ok(directories.config_dir().join("connections.toml"))
    }

    pub fn load(path: &Path) -> Result<Self, StorageError> {
        match fs::read_to_string(path) {
            Ok(value) => {
                let mut config: Self = toml::from_str(&value).map_err(|error| {
                    StorageError::new(
                        ErrorKind::InvalidInput,
                        format!("invalid connection configuration: {error}"),
                    )
                })?;
                if config.version != config_version() {
                    return Err(StorageError::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "unsupported connection configuration version {}",
                            config.version
                        ),
                    ));
                }
                config
                    .connections
                    .retain(|connection| !matches!(connection.connection, Connection::Unsupported));
                Ok(config)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(StorageError::new(
                ErrorKind::Other,
                format!("read connection configuration: {error}"),
            )),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let serialized = toml::to_string_pretty(self).map_err(|error| {
            StorageError::new(
                ErrorKind::Other,
                format!("serialize connection configuration: {error}"),
            )
        })?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                StorageError::new(
                    ErrorKind::Other,
                    format!("create connection configuration directory: {error}"),
                )
            })?;
        }
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, serialized).map_err(|error| {
            StorageError::new(
                ErrorKind::Other,
                format!("write connection configuration: {error}"),
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    StorageError::new(
                        ErrorKind::Other,
                        format!("protect connection configuration: {error}"),
                    )
                },
            )?;
        }
        replace_config(&temporary, path).map_err(|error| {
            StorageError::new(
                ErrorKind::Other,
                format!("install connection configuration: {error}"),
            )
        })
    }
}

#[cfg(unix)]
fn replace_config(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_config(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
