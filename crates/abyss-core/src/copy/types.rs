use std::path::Path;

use crate::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictDecision {
    Overwrite,
    Skip,
    Cancel,
}

pub trait ConflictResolver: Send + Sync {
    fn resolve(&self, destination: &Path) -> Result<ConflictDecision, Error>;
}

pub(crate) struct OverwriteAll;

impl ConflictResolver for OverwriteAll {
    fn resolve(&self, _destination: &Path) -> Result<ConflictDecision, Error> {
        Ok(ConflictDecision::Overwrite)
    }
}
