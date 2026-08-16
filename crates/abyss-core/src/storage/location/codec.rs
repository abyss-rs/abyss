use std::path::PathBuf;

use url::Url;

use super::path::StoragePath;
use super::types::{Location, RemoteLocation};
use crate::storage::{ErrorKind, StorageError};

pub struct LocationCodec;

impl LocationCodec {
    pub fn parse(value: &str) -> Result<Location, StorageError> {
        let Some(scheme_end) = value.find("://") else {
            return Ok(Location::Local(PathBuf::from(value)));
        };
        let scheme = value[..scheme_end].to_ascii_lowercase();
        if !scheme_supported(&scheme) {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                format!("unsupported storage URI scheme: {scheme}"),
            ));
        }
        for raw_component in value[scheme_end + 3..]
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .split('/')
            .skip(1)
        {
            if percent_decode_bytes(raw_component) == b".." {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "remote paths cannot contain '..'",
                ));
            }
        }
        let parsed = Url::parse(value).map_err(|error| {
            StorageError::new(
                ErrorKind::InvalidInput,
                format!("invalid storage URI: {error}"),
            )
        })?;
        let connection = parsed.host_str().unwrap_or_default().to_owned();
        if connection.is_empty() {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "remote location is missing a connection name",
            ));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "storage locations cannot contain query strings or fragments",
            ));
        }
        let decoded = parsed
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .map(percent_decode_bytes)
            .collect::<Vec<_>>();
        let path = {
            #[cfg(feature = "kubernetes")]
            {
                if scheme == "kube" {
                    StoragePath::Kubernetes(decoded)
                } else {
                    StoragePath::remote(
                        decoded
                            .into_iter()
                            .map(|value| {
                                String::from_utf8(value).map_err(|_| {
                                    StorageError::new(
                                        ErrorKind::InvalidInput,
                                        "object storage paths must be UTF-8",
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                            .join("/"),
                    )?
                }
            }
            #[cfg(not(feature = "kubernetes"))]
            {
                StoragePath::remote(
                    decoded
                        .into_iter()
                        .map(|value| {
                            String::from_utf8(value).map_err(|_| {
                                StorageError::new(
                                    ErrorKind::InvalidInput,
                                    "object storage paths must be UTF-8",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("/"),
                )?
            }
        };
        Ok(Location::Remote(RemoteLocation {
            scheme,
            connection,
            path,
        }))
    }

    pub fn format(location: &Location) -> String {
        location.display()
    }
}

pub(super) fn normalize_remote_path(path: String) -> Result<String, StorageError> {
    let mut normalized = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "remote paths cannot contain '..'",
                ));
            }
            part => normalized.push(part),
        }
    }
    Ok(normalized.join("/"))
}

fn percent_decode_bytes(value: &str) -> Vec<u8> {
    percent_encoding::percent_decode_str(value).collect()
}

pub(super) fn encode_storage_path(path: &StoragePath) -> String {
    let components = match path {
        StoragePath::Local(path) => return path.display().to_string(),
        StoragePath::Remote(path) => path
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| part.as_bytes())
            .collect::<Vec<_>>(),
        #[cfg(feature = "kubernetes")]
        StoragePath::Kubernetes(parts) => parts.iter().map(Vec::as_slice).collect(),
    };
    components
        .into_iter()
        .map(|part| percent_encoding::percent_encode(part, URI_COMPONENT_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

const URI_COMPONENT_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'%')
    .add(b'#')
    .add(b'?')
    .add(b'/');

fn scheme_supported(scheme: &str) -> bool {
    // URI identity is independent of compiled providers so workspace history and
    // bookmarks keep working when remotes are feature-gated off.
    matches!(
        scheme,
        "s3" | "az" | "adls" | "gs" | "kube" | "sftp" | "ftp" | "ftps" | "smb"
    )
}
