#[cfg(any(
    feature = "s3",
    feature = "azure",
    feature = "gcs",
    feature = "kubernetes"
))]
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use std::env;
#[cfg(feature = "s3")]
use std::path::Path;
use std::path::PathBuf;

use super::{Connection, Location, NamedConnection};

#[cfg(feature = "s3")]
mod aws;
#[cfg(any(feature = "azure", feature = "gcs"))]
mod cloud;
#[cfg(feature = "kubernetes")]
mod kubernetes;
mod sources;

#[cfg(test)]
mod tests;

pub use self::sources::discover_sources;

/// Non-secret process and filesystem metadata used while finding storage sources.
///
/// Keeping this as a value makes discovery deterministic in tests and ensures
/// credential values never become part of a source row.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryEnvironment {
    #[allow(dead_code)]
    variables: HashMap<String, String>,
    #[allow(dead_code)]
    home: Option<PathBuf>,
}

impl DiscoveryEnvironment {
    pub fn capture() -> Self {
        Self {
            variables: env::vars().collect(),
            home: env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }

    #[cfg(test)]
    pub fn for_test(
        variables: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        home: Option<PathBuf>,
    ) -> Self {
        Self {
            variables: variables
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
            home,
        }
    }

    #[allow(dead_code)]
    fn value(&self, name: &str) -> Option<&str> {
        self.variables
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    #[cfg(feature = "s3")]
    fn path(&self, variable: &str, default: impl FnOnce(&Path) -> PathBuf) -> Option<PathBuf> {
        self.value(variable)
            .map(PathBuf::from)
            .or_else(|| self.home.as_deref().map(default))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StorageSource {
    /// Stable, collision-safe identifier used in generated URIs.
    pub id: String,
    pub provider: String,
    pub name: String,
    /// Profile, Kubernetes context, account, or project.
    pub context: String,
    /// Endpoint or other useful non-secret connection detail.
    pub endpoint: String,
    pub location: Location,
    pub persistent: bool,
    pub connection: Option<NamedConnection>,
}

impl StorageSource {
    pub fn local() -> Self {
        Self {
            id: "local".to_owned(),
            provider: "Local".to_owned(),
            name: "Local filesystem".to_owned(),
            context: String::new(),
            endpoint: String::new(),
            location: Location::Local(PathBuf::new()),
            persistent: true,
            connection: None,
        }
    }
}

#[derive(Clone)]
struct Candidate {
    dedup: String,
    preferred_id: String,
    provider: String,
    name: String,
    context: String,
    endpoint: String,
    scheme: &'static str,
    connection: NamedConnection,
    persistent: bool,
}

fn provider_key(connection: &Connection) -> &'static str {
    match connection {
        #[cfg(feature = "s3")]
        Connection::S3(_) => "s3",
        #[cfg(feature = "azure")]
        Connection::Azure(_) => "azure",
        #[cfg(feature = "gcs")]
        Connection::Gcs(_) => "gcs",
        #[cfg(feature = "kubernetes")]
        Connection::Kubernetes(_) => "kube",
        #[cfg(feature = "sftp")]
        Connection::Sftp(_) => "sftp",
        #[cfg(feature = "ftp")]
        Connection::Ftp(_) => "ftp",
        Connection::Unsupported => "unsupported",
    }
}

fn collision_safe_id(preferred: &str, used: &mut HashSet<String>) -> String {
    let base = format!("discovered-{}", slug(preferred));
    if used.insert(base.clone()) {
        return base;
    }
    for suffix in 2_u64.. {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !result.is_empty() {
            result.push('-');
            separator = true;
        }
    }
    result.trim_end_matches('-').to_owned().if_empty("source")
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

#[cfg(any(
    feature = "s3",
    feature = "azure",
    feature = "gcs",
    feature = "kubernetes"
))]
#[allow(clippy::too_many_arguments)]
fn insert_discovered(
    output: &mut BTreeMap<String, Candidate>,
    dedup: String,
    preferred_id: String,
    provider: impl Into<String>,
    name: impl Into<String>,
    context: String,
    endpoint: String,
    scheme: &'static str,
    connection: Connection,
) {
    output.insert(
        dedup.clone(),
        Candidate {
            dedup,
            preferred_id,
            provider: provider.into(),
            name: name.into(),
            context,
            endpoint,
            scheme,
            connection: NamedConnection {
                id: String::new(),
                name: String::new(),
                connection,
            },
            persistent: false,
        },
    );
}
