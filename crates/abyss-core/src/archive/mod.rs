pub(crate) mod formats;
pub(crate) mod reader;
mod types;
pub(crate) mod writer;

#[cfg(test)]
mod tests;

pub use self::reader::{extract_member, looks_like_archive, normalize_member_path, read_selected};
pub use self::types::*;
pub use self::writer::{create_archive, create_suffix};
