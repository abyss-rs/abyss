use std::fs::File;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::Error;

const APPLEDOUBLE_MAGIC: [u8; 4] = [0x00, 0x05, 0x16, 0x07];

pub fn is_candidate(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"._"))
}

pub fn is_verified(path: &Path) -> Result<bool, Error> {
    if !is_candidate(path) {
        return Ok(false);
    }
    match open_no_follow(path) {
        Ok(mut file) => {
            let metadata = file
                .metadata()
                .map_err(|error| Error::io("inspect possible AppleDouble file", path, error))?;
            if !metadata.file_type().is_file() {
                return Ok(false);
            }
            Ok(has_magic(&mut file, path)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) if is_symlink_loop(&error) => Ok(false),
        Err(error) => Err(Error::io("open possible AppleDouble file", path, error)),
    }
}

fn open_no_follow(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }
    #[cfg(windows)]
    {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppleDouble candidate is a symbolic link",
            ));
        }
        File::open(path)
    }
}

#[cfg(unix)]
fn is_symlink_loop(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(windows)]
fn is_symlink_loop(_error: &io::Error) -> bool {
    false
}

fn has_magic(file: &mut File, path: &Path) -> Result<bool, Error> {
    let mut magic = [0_u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(magic == APPLEDOUBLE_MAGIC),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(Error::io("read AppleDouble header from", path, error)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::is_verified;
    use crate::test_support::TempDir;

    #[test]
    fn verifies_appledouble_magic_without_modifying_the_file() {
        let temp = TempDir::new();
        let companion = temp.path().join("._video");
        fs::write(&companion, [0x00, 0x05, 0x16, 0x07, 0, 0]).unwrap();
        assert!(is_verified(&companion).unwrap());
        assert!(companion.exists());
    }

    #[test]
    fn rejects_an_ordinary_dot_underscore_file() {
        let temp = TempDir::new();
        let companion = temp.path().join("._video");
        fs::write(&companion, b"ordinary user file").unwrap();
        assert!(!is_verified(&companion).unwrap());
        assert_eq!(fs::read(companion).unwrap(), b"ordinary user file");
    }
}
