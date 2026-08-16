use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

const RESUME_DIRECTORY_ENV: &str = "ABYSS_RESUME_DIR";

pub fn journal_path(namespace: &str, identity: &[&str]) -> io::Result<PathBuf> {
    journal_path_in(&resume_directory()?, namespace, identity)
}

pub fn journal_path_in(root: &Path, namespace: &str, identity: &[&str]) -> io::Result<PathBuf> {
    let directory = root.join(namespace);
    fs::create_dir_all(&directory)?;
    let mut digest = Sha256::new();
    for value in identity {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    Ok(directory.join(format!("{}.json", hex(&digest.finalize()))))
}

pub fn load<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    serde_json::to_writer(temporary.as_file_mut(), value)?;
    temporary.as_file().sync_all()?;
    set_private_permissions(temporary.path())?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .map(|_| ())
}

pub fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn resume_directory() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(RESUME_DIRECTORY_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    ProjectDirs::from("", "", "Abyss")
        .map(|directories| directories.data_local_dir().join("transfer-resume"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no application data directory"))
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::{journal_path_in, load, remove, save};

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Example {
        upload_id: String,
        object: String,
    }

    #[test]
    fn journal_names_are_collision_safe_and_contents_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let first = journal_path_in(directory.path(), "s3", &["a/b", "c"]).unwrap();
        let second = journal_path_in(directory.path(), "s3", &["a", "b/c"]).unwrap();
        assert_ne!(first, second);
        let value = Example {
            upload_id: "opaque-provider-id".to_owned(),
            object: "bucket/key".to_owned(),
        };
        save(&first, &value).unwrap();
        assert_eq!(load::<Example>(&first).unwrap(), Some(value));
        remove(&first).unwrap();
        assert_eq!(load::<Example>(&first).unwrap(), None);
    }
}
