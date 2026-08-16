use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use super::extract_member;
use crate::archive::types::{ArchiveIndex, ArchiveLoadResult, ArchiveOpenError, ArchiveRequest};
pub(crate) fn load_request(
    request: ArchiveRequest,
    password: Option<Zeroizing<String>>,
) -> ArchiveLoadResult {
    let opened = match request.clone() {
        ArchiveRequest::Path { path, .. } => {
            let multipart = match join_multipart(&path) {
                Ok(value) => value,
                Err(error) => return map_load_error(request, error),
            };
            let open_path = multipart
                .as_ref()
                .map(NamedTempFile::path)
                .unwrap_or(path.as_path());
            match ArchiveIndex::open(open_path, password.as_deref().map(String::as_str)) {
                Ok(index) => {
                    return ArchiveLoadResult::Opened {
                        request,
                        index,
                        temporary: multipart,
                        password,
                    };
                }
                Err(ArchiveOpenError::NotArchive) => {
                    let viewer_path = multipart
                        .as_ref()
                        .map(|file| file.path().to_owned())
                        .unwrap_or(path);
                    return ArchiveLoadResult::Viewer {
                        request,
                        temporary: multipart,
                        path: viewer_path,
                    };
                }
                Err(error) => Err(error),
            }
        }
        ArchiveRequest::Member {
            parent,
            member,
            parent_password,
            display_name,
            try_archive,
            ..
        } => {
            let suffix = archive_suffix(&display_name);
            let mut builder = tempfile::Builder::new();
            if !suffix.is_empty() {
                builder.suffix(&suffix);
            }
            let mut temporary = match builder.tempfile() {
                Ok(file) => file,
                Err(error) => {
                    return ArchiveLoadResult::Failed {
                        message: error.to_string(),
                    };
                }
            };
            if let Err(error) = extract_member(
                &parent,
                &member,
                parent_password.as_deref().map(String::as_str),
                temporary.as_file_mut(),
            ) {
                return map_load_error(request, error);
            }
            if let Err(error) = temporary.as_file_mut().flush() {
                return ArchiveLoadResult::Failed {
                    message: error.to_string(),
                };
            }
            if !try_archive {
                let path = temporary.path().to_owned();
                return ArchiveLoadResult::Viewer {
                    request,
                    temporary: Some(temporary),
                    path,
                };
            }
            match ArchiveIndex::open(temporary.path(), password.as_deref().map(String::as_str)) {
                Ok(index) => {
                    return ArchiveLoadResult::Opened {
                        request,
                        index,
                        temporary: Some(temporary),
                        password,
                    };
                }
                Err(ArchiveOpenError::NotArchive) => {
                    let path = temporary.path().to_owned();
                    return ArchiveLoadResult::Viewer {
                        request,
                        temporary: Some(temporary),
                        path,
                    };
                }
                Err(error) => Err(error),
            }
        }
    };
    match opened {
        Ok(()) => unreachable!(),
        Err(error) => map_load_error(request, error),
    }
}

pub(crate) fn multipart_part_path(destination: &Path, part: u64) -> PathBuf {
    let mut value = destination.as_os_str().to_os_string();
    value.push(format!(".{part:03}"));
    PathBuf::from(value)
}

pub(crate) fn multipart_base(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let (base, part) = name.rsplit_once('.')?;
    if part != "001" || base.is_empty() {
        return None;
    }
    Some(path.with_file_name(base))
}

pub(crate) fn join_multipart(path: &Path) -> Result<Option<NamedTempFile>, ArchiveOpenError> {
    let Some(base) = multipart_base(path) else {
        return Ok(None);
    };
    let suffix = archive_suffix(
        base.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    );
    let mut builder = tempfile::Builder::new();
    if !suffix.is_empty() {
        builder.suffix(&suffix);
    }
    let mut joined = builder
        .tempfile()
        .map_err(|error| ArchiveOpenError::Other(format!("create multipart staging: {error}")))?;
    let mut part = 1u64;
    loop {
        let part_path = multipart_part_path(&base, part);
        match File::open(&part_path) {
            Ok(mut input) => {
                io::copy(&mut input, joined.as_file_mut()).map_err(|error| {
                    ArchiveOpenError::Other(format!(
                        "join archive volume {}: {error}",
                        part_path.display()
                    ))
                })?;
                part += 1;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && part > 1 => break,
            Err(error) => {
                return Err(ArchiveOpenError::Other(format!(
                    "open archive volume {}: {error}",
                    part_path.display()
                )));
            }
        }
    }
    joined
        .as_file_mut()
        .flush()
        .map_err(|error| ArchiveOpenError::Other(format!("flush multipart staging: {error}")))?;
    Ok(Some(joined))
}

pub(crate) fn archive_suffix(name: &str) -> String {
    name.find('.')
        .map(|index| name[index..].to_owned())
        .unwrap_or_default()
}

pub(crate) fn map_load_error(
    request: ArchiveRequest,
    error: ArchiveOpenError,
) -> ArchiveLoadResult {
    match error {
        ArchiveOpenError::PasswordRequired(message) => ArchiveLoadResult::Password {
            request,
            invalid: false,
            message,
        },
        ArchiveOpenError::InvalidPassword(message) => ArchiveLoadResult::Password {
            request,
            invalid: true,
            message,
        },
        ArchiveOpenError::NotArchive => ArchiveLoadResult::Failed {
            message: "not a supported archive".to_owned(),
        },
        ArchiveOpenError::Other(message) => ArchiveLoadResult::Failed { message },
    }
}
