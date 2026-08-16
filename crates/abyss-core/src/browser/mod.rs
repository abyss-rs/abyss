pub(crate) mod pane;
pub(crate) mod scanner;
pub(crate) mod service;
pub(crate) mod sort;
mod types;

#[cfg(test)]
mod tests;

pub use self::pane::Pane;
pub use self::service::BrowserService;
pub use self::types::{
    BrowserEntry, BrowserEvent, BrowserKind, FastHashMap, FxHasher, SortMode, SortSpec,
    SourceEntry, SourceProbeStatus, SourceView,
};
