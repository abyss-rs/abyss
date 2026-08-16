use std::fmt;
use std::path::PathBuf;

use super::codec::encode_storage_path;
use super::path::{StoragePath, local_component, transfer_local_component};
use crate::storage::StorageError;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RemoteLocation {
    pub scheme: String,
    pub connection: String,
    pub path: StoragePath,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Location {
    Local(PathBuf),
    Remote(RemoteLocation),
}

impl Location {
    pub fn display(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(remote) => {
                let path = encode_storage_path(&remote.path);
                if path.is_empty() {
                    format!("{}://{}", remote.scheme, remote.connection)
                } else {
                    format!("{}://{}/{}", remote.scheme, remote.connection, path)
                }
            }
        }
    }

    pub fn child(&self, name: &[u8]) -> Result<Self, StorageError> {
        match self {
            Self::Local(path) => Ok(Self::Local(path.join(local_component(name)))),
            Self::Remote(remote) => Ok(Self::Remote(RemoteLocation {
                scheme: remote.scheme.clone(),
                connection: remote.connection.clone(),
                path: remote.path.child(name)?,
            })),
        }
    }

    /// Append a name received from another storage backend. Windows cannot
    /// represent every byte sequence or object-store name, so validate instead
    /// of silently changing it.
    pub fn child_transfer(&self, name: &[u8]) -> Result<Self, StorageError> {
        match self {
            Self::Local(path) => {
                let component = transfer_local_component(name)?;
                Ok(Self::Local(path.join(component)))
            }
            Self::Remote(_) => self.child(name),
        }
    }

    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Local(path) => path.parent().map(|value| Self::Local(value.to_owned())),
            Self::Remote(remote) => remote.path.parent().map(|path| {
                Self::Remote(RemoteLocation {
                    scheme: remote.scheme.clone(),
                    connection: remote.connection.clone(),
                    path,
                })
            }),
        }
    }

    pub fn file_name(&self) -> Option<Vec<u8>> {
        match self {
            Self::Local(path) => path
                .file_name()
                .map(|value| value.as_encoded_bytes().to_vec()),
            Self::Remote(remote) => remote.path.file_name(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(left), Self::Local(right)) => {
                left == right || left.starts_with(right) || right.starts_with(left)
            }
            (Self::Remote(left), Self::Remote(right))
                if left.scheme == right.scheme && left.connection == right.connection =>
            {
                left.path.overlaps(&right.path)
            }
            _ => false,
        }
    }
}

impl From<PathBuf> for Location {
    fn from(path: PathBuf) -> Self {
        Self::Local(path)
    }
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display())
    }
}
