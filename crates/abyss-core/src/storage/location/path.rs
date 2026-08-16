use std::ffi::OsString;
use std::path::PathBuf;

use super::codec::normalize_remote_path;
use crate::storage::{ErrorKind, StorageError};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum StoragePath {
    Local(PathBuf),
    Remote(String),
    #[cfg(feature = "kubernetes")]
    Kubernetes(Vec<Vec<u8>>),
}

impl StoragePath {
    pub fn remote(path: impl Into<String>) -> Result<Self, StorageError> {
        let path = normalize_remote_path(path.into())?;
        Ok(Self::Remote(path))
    }

    pub fn display(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(path) => path.clone(),
            #[cfg(feature = "kubernetes")]
            Self::Kubernetes(components) => components
                .iter()
                .map(|value| String::from_utf8_lossy(value))
                .collect::<Vec<_>>()
                .join("/"),
        }
    }

    pub fn components(&self) -> Vec<OsString> {
        match self {
            Self::Local(path) => path
                .components()
                .map(|component| component.as_os_str().to_owned())
                .collect(),
            Self::Remote(path) => path
                .split('/')
                .filter(|part| !part.is_empty())
                .map(Into::into)
                .collect(),
            #[cfg(feature = "kubernetes")]
            Self::Kubernetes(path) => path
                .iter()
                .map(|part| String::from_utf8_lossy(part).into_owned().into())
                .collect(),
        }
    }
}

impl StoragePath {
    pub fn child(&self, name: &[u8]) -> Result<Self, StorageError> {
        if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "storage path component is invalid",
            ));
        }
        match self {
            Self::Local(path) => Ok(Self::Local(path.join(local_component(name)))),
            Self::Remote(path) => {
                let name = std::str::from_utf8(name).map_err(|_| {
                    StorageError::new(
                        ErrorKind::InvalidInput,
                        "object storage names must be UTF-8",
                    )
                })?;
                let joined = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}/{name}")
                };
                Self::remote(joined)
            }
            #[cfg(feature = "kubernetes")]
            Self::Kubernetes(parts) => {
                let mut joined = parts.clone();
                joined.push(name.to_vec());
                Ok(Self::Kubernetes(joined))
            }
        }
    }

    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Local(path) => path.parent().map(|value| Self::Local(value.to_owned())),
            Self::Remote(path) => {
                if path.is_empty() {
                    None
                } else {
                    Some(Self::Remote(
                        path.rsplit_once('/')
                            .map_or_else(String::new, |(parent, _)| parent.to_owned()),
                    ))
                }
            }
            #[cfg(feature = "kubernetes")]
            Self::Kubernetes(parts) => {
                if parts.is_empty() {
                    None
                } else {
                    Some(Self::Kubernetes(parts[..parts.len() - 1].to_vec()))
                }
            }
        }
    }

    pub fn file_name(&self) -> Option<Vec<u8>> {
        match self {
            Self::Local(path) => path
                .file_name()
                .map(|value| value.as_encoded_bytes().to_vec()),
            Self::Remote(path) => path
                .rsplit('/')
                .next()
                .filter(|value| !value.is_empty())
                .map(|value| value.as_bytes().to_vec()),
            #[cfg(feature = "kubernetes")]
            Self::Kubernetes(parts) => parts.last().cloned(),
        }
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(left), Self::Local(right)) => {
                left == right || left.starts_with(right) || right.starts_with(left)
            }
            (Self::Remote(left), Self::Remote(right)) => {
                let left = left
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>();
                let right = right
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>();
                left.starts_with(&right) || right.starts_with(&left)
            }
            #[cfg(feature = "kubernetes")]
            (Self::Kubernetes(left), Self::Kubernetes(right)) => {
                left.starts_with(right) || right.starts_with(left)
            }
            _ => false,
        }
    }
}

pub(super) fn local_component(value: &[u8]) -> OsString {
    // Every local caller obtains these bytes from `OsStr::as_encoded_bytes` or
    // from UTF-8 user input, both of which are valid for the current platform.
    unsafe { OsString::from_encoded_bytes_unchecked(value.to_vec()) }
}

#[cfg(unix)]
pub(super) fn transfer_local_component(value: &[u8]) -> Result<OsString, StorageError> {
    Ok(local_component(value))
}

#[cfg(windows)]
pub(super) fn transfer_local_component(value: &[u8]) -> Result<OsString, StorageError> {
    let value = std::str::from_utf8(value).map_err(|_| {
        StorageError::new(
            ErrorKind::InvalidInput,
            "the remote filename is not valid Unicode and cannot be created on Windows",
        )
    })?;
    let invalid_character = value.chars().any(|character| {
        character < '\u{20}'
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
    });
    let stem = value.split('.').next().unwrap_or_default();
    let reserved = matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if value.is_empty() || value.ends_with([' ', '.']) || invalid_character || reserved {
        return Err(StorageError::new(
            ErrorKind::InvalidInput,
            format!("the remote filename is not valid on Windows: {value:?}"),
        ));
    }
    Ok(value.into())
}
