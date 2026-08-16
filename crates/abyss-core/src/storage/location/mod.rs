mod codec;
mod path;
mod types;

#[cfg(test)]
mod tests;

pub use self::codec::LocationCodec;
pub use self::path::StoragePath;
pub use self::types::{Location, RemoteLocation};
