use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::appledouble;
use crate::progress::CopyStats;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub relative: PathBuf,
    pub kind: EntryKind,
    pub len: u64,
    pub device: u64,
    pub inode: u64,
    pub links: u64,
    pub mode: u32,
    pub accessed_sec: i64,
    #[cfg_attr(windows, allow(dead_code))]
    pub accessed_nsec: i64,
    pub modified_sec: i64,
    #[cfg_attr(windows, allow(dead_code))]
    pub modified_nsec: i64,
}

impl Entry {
    fn from_metadata(
        _path: &Path,
        relative: PathBuf,
        metadata: &fs::Metadata,
    ) -> Result<Self, Error> {
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };

        #[cfg(unix)]
        let (device, inode, links, mode, accessed_sec, accessed_nsec, modified_sec, modified_nsec) = (
            metadata.dev(),
            metadata.ino(),
            metadata.nlink(),
            metadata.mode(),
            metadata.atime(),
            metadata.atime_nsec(),
            metadata.mtime(),
            metadata.mtime_nsec(),
        );
        #[cfg(windows)]
        let (device, inode, links, mode, accessed_sec, accessed_nsec, modified_sec, modified_nsec) = {
            let identity = crate::native::path_identity(_path)
                .map_err(|error| Error::io("identify", _path, error))?;
            (
                identity.volume,
                identity.index,
                identity.links,
                metadata.file_attributes(),
                metadata.last_access_time() as i64,
                0,
                metadata.last_write_time() as i64,
                0,
            )
        };

        Ok(Self {
            relative,
            kind,
            len: if kind == EntryKind::File {
                metadata.len()
            } else {
                0
            },
            device,
            inode,
            links,
            mode,
            accessed_sec,
            accessed_nsec,
            modified_sec,
            modified_nsec,
        })
    }
}

#[derive(Debug)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub logical_bytes: u64,
    pub files: u64,
    pub directories: u64,
    pub symlinks: u64,
    pub other: u64,
}

impl Inventory {
    #[cfg(test)]
    pub fn scan(source: &Path, cancelled: &AtomicBool) -> Result<Self, Error> {
        Self::scan_with_progress(source, cancelled, None)
    }

    pub fn scan_with_progress(
        source: &Path,
        cancelled: &AtomicBool,
        progress: Option<&CopyStats>,
    ) -> Result<Self, Error> {
        Self::scan_impl(source, cancelled, progress, false, true)
    }

    pub fn scan_for_copy_with_progress(
        source: &Path,
        cancelled: &AtomicBool,
        progress: Option<&CopyStats>,
    ) -> Result<Self, Error> {
        if appledouble::is_verified(source)? {
            return Err(Error::message(format!(
                "refusing to copy AppleDouble metadata file directly: {}",
                source.display()
            )));
        }
        let inventory = Self::scan_impl(source, cancelled, progress, true, false)?;
        if inventory.other > 0 {
            return Err(Error::message(format!(
                "source contains {} unsupported filesystem object(s): {}",
                inventory.other,
                source.display()
            )));
        }
        Ok(inventory)
    }

    fn scan_impl(
        source: &Path,
        cancelled: &AtomicBool,
        progress: Option<&CopyStats>,
        skip_apple_double: bool,
        allow_other: bool,
    ) -> Result<Self, Error> {
        let mut inventory = Self {
            entries: Vec::new(),
            logical_bytes: 0,
            files: 0,
            directories: 0,
            symlinks: 0,
            other: 0,
        };
        inventory.visit(
            source,
            Path::new(""),
            None,
            cancelled,
            progress,
            skip_apple_double,
            allow_other,
        )?;
        Ok(inventory)
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        &mut self,
        path: &Path,
        relative: &Path,
        preset_metadata: Option<fs::Metadata>,
        cancelled: &AtomicBool,
        progress: Option<&CopyStats>,
        skip_apple_double: bool,
        allow_other: bool,
    ) -> Result<(), Error> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }

        let metadata = match preset_metadata {
            Some(metadata) => metadata,
            None => {
                fs::symlink_metadata(path).map_err(|error| Error::io("inspect", path, error))?
            }
        };
        let entry = Entry::from_metadata(path, relative.to_owned(), &metadata)?;
        if let Some(progress) = progress {
            progress.observe_scan(path);
        }

        match entry.kind {
            EntryKind::Directory => self.directories += 1,
            EntryKind::File => {
                self.files += 1;
                self.logical_bytes = self.logical_bytes.saturating_add(entry.len);
            }
            EntryKind::Symlink => self.symlinks += 1,
            EntryKind::Other if allow_other => self.other += 1,
            EntryKind::Other => {
                return Err(Error::message(format!(
                    "unsupported filesystem object: {}",
                    path.display()
                )));
            }
        }
        let is_directory = entry.kind == EntryKind::Directory;
        self.entries.push(entry);

        if is_directory {
            let mut children = fs::read_dir(path)
                .map_err(|error| Error::io("read directory", path, error))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| Error::io("read directory entry in", path, error))?;
            children.sort_by(|left, right| {
                os_bytes(&left.file_name()).cmp(os_bytes(&right.file_name()))
            });

            for child in children {
                let child_path = child.path();
                if skip_apple_double
                    && appledouble::is_candidate(&child_path)
                    && appledouble::is_verified(&child_path)?
                {
                    continue;
                }
                let child_relative = relative.join(child.file_name());
                let child_metadata = child.metadata().ok();
                self.visit(
                    &child_path,
                    &child_relative,
                    child_metadata,
                    cancelled,
                    progress,
                    skip_apple_double,
                    allow_other,
                )?;
            }
        }

        Ok(())
    }
}

fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_encoded_bytes()
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(unix)]
    use std::os::unix::net::UnixDatagram;
    use std::sync::atomic::AtomicBool;

    #[cfg(unix)]
    use super::EntryKind;
    use super::Inventory;
    use crate::test_support::TempDir;

    #[test]
    #[cfg(unix)]
    fn scans_without_following_symlinks() {
        let temp = TempDir::new();
        let root = temp.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("one"), b"1234").unwrap();
        fs::write(root.join("._one"), [0x00, 0x05, 0x16, 0x07, 0, 0]).unwrap();
        fs::create_dir(root.join("folder")).unwrap();
        fs::write(root.join("folder/two"), b"12").unwrap();
        symlink("folder", root.join("link")).unwrap();

        let inventory =
            Inventory::scan_for_copy_with_progress(&root, &AtomicBool::new(false), None).unwrap();

        assert_eq!(inventory.logical_bytes, 6);
        assert_eq!(inventory.files, 2);
        assert_eq!(inventory.directories, 2);
        assert_eq!(inventory.symlinks, 1);
        assert!(
            inventory
                .entries
                .iter()
                .all(|entry| !entry.relative.to_string_lossy().starts_with("._"))
        );
        assert_eq!(
            inventory
                .entries
                .iter()
                .filter(|entry| entry.kind == EntryKind::Symlink)
                .count(),
            1
        );
    }

    #[test]
    fn normal_scan_keeps_apple_double_for_explicit_deletion() {
        let temp = TempDir::new();
        let root = temp.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("one"), b"x").unwrap();
        fs::write(root.join("._one"), [0x00, 0x05, 0x16, 0x07]).unwrap();
        let inventory = Inventory::scan(&root, &AtomicBool::new(false)).unwrap();
        assert_eq!(inventory.files, 2);
    }

    #[test]
    #[cfg(unix)]
    fn delete_inventory_accepts_special_objects_but_copy_inventory_rejects_them() {
        let temp = TempDir::new();
        let socket_path = temp.path().join("service.socket");
        let _socket = UnixDatagram::bind(&socket_path).unwrap();

        let delete_inventory = Inventory::scan(temp.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(delete_inventory.other, 1);
        assert!(
            Inventory::scan_for_copy_with_progress(temp.path(), &AtomicBool::new(false), None,)
                .is_err()
        );
    }
}
