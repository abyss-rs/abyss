use std::collections::HashMap;

mod copy;
mod fs_ops;
mod metadata;
mod reparse;
mod util;

#[cfg(test)]
mod tests;

pub use self::copy::{copy_regular_file, try_hard_link};
pub use self::fs_ops::{
    move_path, recover_unremovable_directory, remove_directory_tree, remove_path,
};
pub use self::metadata::{apply_path_metadata, path_identity};
pub use self::reparse::copy_symlink;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsFileIdentity {
    pub volume: u64,
    pub index: u64,
    pub links: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CopyOutcome {
    pub physical_bytes: u64,
    pub cloned: bool,
    pub linked: bool,
}

#[derive(Default)]
pub struct CloneCapabilities {
    by_destination_volume: HashMap<u64, bool>,
}

impl CloneCapabilities {
    pub(super) fn observe(&mut self, volume: u64) {
        self.by_destination_volume.entry(volume).or_insert(false);
    }
}
