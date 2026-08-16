use std::path::PathBuf;

pub use quichash_core::Algorithm as HashAlgorithm;
pub use quichash_core::database::DatabaseFormat as HashDatabaseFormat;

pub struct HashCreateOptions {
    pub sources: Vec<PathBuf>,
    pub root: PathBuf,
    pub destination: PathBuf,
    pub algorithm: HashAlgorithm,
    pub format: HashDatabaseFormat,
    pub compressed: bool,
    pub parallel: bool,
}
