use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::archive::{self, ArchiveIndex};
use crate::copy::{self, ConflictDecision, ConflictResolver};
use crate::progress::{CopyStats, OperationPhase};

pub(crate) fn copy_paths(
    sources: &[PathBuf],
    destination: &Path,
    cancelled: Arc<AtomicBool>,
    stats: Arc<CopyStats>,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    if sources.len() == 1 {
        copy::run_with_stats(&sources[0], destination, cancelled, stats, conflicts)
    } else {
        copy::run_batch(sources, destination, cancelled, stats, conflicts)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_paths(
    index: &ArchiveIndex,
    roots: &[String],
    base: &str,
    destination: &Path,
    password: Option<&str>,
    cancelled: &AtomicBool,
    stats: &CopyStats,
    conflicts: &dyn ConflictResolver,
) -> Result<(), Error> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(Error::message(format!(
                "extraction destination is not a directory: {}",
                destination.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(destination)
                .map_err(|error| Error::io("create extraction directory", destination, error))?;
        }
        Err(error) => {
            return Err(Error::io(
                "inspect extraction destination",
                destination,
                error,
            ));
        }
    }
    reject_symlink(destination)?;

    let selected = index
        .members
        .iter()
        .filter(|member| {
            roots.iter().any(|root| {
                member.path == *root
                    || member
                        .path
                        .strip_prefix(root)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        })
        .collect::<Vec<_>>();
    let total_bytes = selected
        .iter()
        .filter(|member| !member.is_directory)
        .map(|member| member.size)
        .sum();
    stats.reset();
    stats.set_totals(selected.len() as u64, total_bytes);
    stats.set_phase(OperationPhase::Extracting);
    for member in selected.iter().filter(|member| member.is_directory) {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let relative = if base.is_empty() {
            member.path.as_str()
        } else {
            member
                .path
                .strip_prefix(base)
                .and_then(|path| path.strip_prefix('/'))
                .ok_or_else(|| Error::message("archive selection escaped its current directory"))?
        };
        let target = safe_destination(destination, relative)?;
        ensure_safe_directory(destination, &target)?;
        stats.complete_object(&target);
    }

    let file_paths = selected
        .iter()
        .filter(|member| !member.is_directory)
        .map(|member| member.path.clone())
        .collect::<HashSet<_>>();
    archive::read_selected(index, &file_paths, password, |member, reader| {
        let result = (|| -> Result<(), Error> {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let relative = if base.is_empty() {
                member.path.as_str()
            } else {
                member
                    .path
                    .strip_prefix(base)
                    .and_then(|path| path.strip_prefix('/'))
                    .ok_or_else(|| {
                        Error::message("archive selection escaped its current directory")
                    })?
            };
            let target = safe_destination(destination, relative)?;
            let parent = target
                .parent()
                .ok_or_else(|| Error::message("archive member has no destination parent"))?;
            ensure_safe_directory(destination, parent)?;
            match fs::symlink_metadata(&target) {
                Ok(metadata) => match conflicts.resolve(&target)? {
                    ConflictDecision::Skip => {
                        stats.skip_object(&target, member.size);
                        return Ok(());
                    }
                    ConflictDecision::Cancel => return Err(Error::Cancelled),
                    ConflictDecision::Overwrite if metadata.is_dir() => {
                        return Err(Error::message(format!(
                            "cannot overwrite directory with archive file: {}",
                            target.display()
                        )));
                    }
                    ConflictDecision::Overwrite => {}
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::io("inspect extraction target", &target, error));
                }
            }

            stats.begin_file(&target, member.size);
            let mut temporary = tempfile::NamedTempFile::new_in(parent)
                .map_err(|error| Error::io("create temporary extracted file in", parent, error))?;
            let bytes = {
                let mut writer = ExtractionWriter {
                    inner: temporary.as_file_mut(),
                    cancelled,
                    stats,
                };
                std::io::copy(reader, &mut writer)
                    .map_err(|error| Error::io("extract archive member to", &target, error))?
            };
            temporary
                .as_file_mut()
                .flush()
                .map_err(|error| Error::io("flush extracted file", &target, error))?;
            temporary
                .persist(&target)
                .map_err(|error| Error::io("install extracted file", &target, error.error))?;
            stats.complete_file(member.size.max(bytes), bytes, false, false);
            Ok(())
        })();
        result.map_err(|error| archive::ArchiveOpenError::Other(error.to_string()))
    })
    .map_err(|error| {
        if cancelled.load(Ordering::Relaxed) {
            Error::Cancelled
        } else {
            Error::message(error.to_string())
        }
    })?;

    Ok(())
}

pub(crate) fn test_archive(
    index: &ArchiveIndex,
    password: Option<&str>,
    cancelled: &AtomicBool,
    stats: &CopyStats,
) -> Result<(), Error> {
    let files = index
        .members
        .iter()
        .filter(|member| !member.is_directory)
        .collect::<Vec<_>>();
    let total_bytes = files.iter().map(|member| member.size).sum();
    stats.reset();
    stats.set_totals(files.len() as u64, total_bytes);
    stats.set_phase(OperationPhase::Testing);

    let file_paths = files
        .iter()
        .map(|member| member.path.clone())
        .collect::<HashSet<_>>();
    archive::read_selected(index, &file_paths, password, |member, reader| {
        let result = (|| -> Result<(), Error> {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let path = PathBuf::from(&member.path);
            stats.begin_file(&path, member.size);
            let bytes = {
                let mut writer = ExtractionWriter {
                    inner: io::sink(),
                    cancelled,
                    stats,
                };
                io::copy(reader, &mut writer).map_err(|error| {
                    Error::io("verify archive member", Path::new(&member.path), error)
                })?
            };
            stats.complete_file(member.size.max(bytes), bytes, false, false);
            Ok(())
        })();
        result.map_err(|error| archive::ArchiveOpenError::Other(error.to_string()))
    })
    .map_err(|error| {
        if cancelled.load(Ordering::Relaxed) {
            Error::Cancelled
        } else {
            Error::message(error.to_string())
        }
    })?;

    Ok(())
}

pub(crate) struct ExtractionWriter<'a, W> {
    pub(crate) inner: W,
    pub(crate) cancelled: &'a AtomicBool,
    pub(crate) stats: &'a CopyStats,
}

impl<W: Write> Write for ExtractionWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "extraction cancelled",
            ));
        }
        if !self
            .stats
            .wait_for_transfer(self.cancelled, buffer.len() as u64)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "extraction cancelled",
            ));
        }
        let written = self.inner.write(buffer)?;
        self.stats
            .current_copied
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn safe_destination(root: &Path, member: &str) -> Result<PathBuf, Error> {
    let mut output = root.to_owned();
    for part in member.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(Error::message(format!(
                "unsafe archive member path: {member}"
            )));
        }
        output.push(part);
    }
    Ok(output)
}

pub(crate) fn ensure_safe_directory(root: &Path, directory: &Path) -> Result<(), Error> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| Error::message("archive destination escaped extraction root"))?;
    let mut current = root.to_owned();
    reject_symlink(&current)?;
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::message(format!(
                    "refusing to extract through symbolic link: {}",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(Error::message(format!(
                    "archive directory conflicts with file: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|error| Error::io("create extracted directory", &current, error))?;
            }
            Err(error) => {
                return Err(Error::io("inspect extracted directory", &current, error));
            }
        }
    }
    Ok(())
}

pub(crate) fn reject_symlink(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Error::io("inspect", path, error))?;
    if metadata.file_type().is_symlink() {
        Err(Error::message(format!(
            "refusing to extract through symbolic link: {}",
            path.display()
        )))
    } else {
        Ok(())
    }
}
