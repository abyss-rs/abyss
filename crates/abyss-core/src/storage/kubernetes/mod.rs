pub(crate) mod backend;
pub(crate) mod compression;
pub(crate) mod descriptor;
pub(crate) mod ops;
pub(crate) mod protocol;
pub(crate) mod session;

#[cfg(test)]
mod tests;

pub use self::descriptor::KubernetesFactory;
