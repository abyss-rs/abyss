use std::collections::HashMap;

mod clone;
mod copy;
mod copyfile;
mod fs_ops;
mod metadata;
mod temp;

#[cfg(test)]
mod tests;

pub use self::copy::{copy_regular_file, copy_symlink, try_hard_link};
pub use self::fs_ops::{
    move_path, recover_unremovable_directory, remove_directory_tree, remove_path,
};
pub use self::metadata::apply_path_metadata;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CopyOutcome {
    pub physical_bytes: u64,
    pub cloned: bool,
    pub linked: bool,
}

#[derive(Default)]
pub struct CloneCapabilities {
    by_destination_device: HashMap<u64, bool>,
}
