use std::collections::{BTreeMap, HashSet};

#[cfg(feature = "azure")]
use crate::storage::AzureMode;
use crate::storage::{
    Connection, ConnectionConfig, Location, NamedConnection, RemoteLocation, StoragePath,
};

#[cfg(feature = "s3")]
use super::aws::{discover_aws, s3_endpoint, s3_provider};
#[cfg(feature = "azure")]
use super::cloud::discover_azure;
#[cfg(feature = "gcs")]
use super::cloud::discover_gcs;
#[cfg(feature = "kubernetes")]
use super::kubernetes::discover_kubernetes;
use super::{Candidate, DiscoveryEnvironment, StorageSource, collision_safe_id, provider_key};

pub fn discover_sources(
    config: &ConnectionConfig,
    environment: &DiscoveryEnvironment,
) -> Vec<StorageSource> {
    let mut candidates = BTreeMap::<String, Candidate>::new();
    #[cfg(feature = "s3")]
    discover_aws(environment, &mut candidates);
    #[cfg(feature = "kubernetes")]
    discover_kubernetes(environment, &mut candidates);
    #[cfg(feature = "azure")]
    discover_azure(environment, &mut candidates);
    #[cfg(feature = "gcs")]
    discover_gcs(environment, &mut candidates);
    #[cfg(not(any(
        feature = "s3",
        feature = "azure",
        feature = "gcs",
        feature = "kubernetes"
    )))]
    let _ = environment;
    #[cfg(not(any(
        feature = "s3",
        feature = "azure",
        feature = "gcs",
        feature = "kubernetes",
        feature = "sftp",
        feature = "ftp"
    )))]
    let _ = &mut candidates;

    // Persistent metadata deliberately wins over inferred metadata.
    #[allow(unused_mut, unused_variables, unreachable_code)]
    for connection in &config.connections {
        let mut candidate = configured_candidate(connection.clone());
        candidates.remove(&candidate.dedup);
        let configured_key = format!(
            "configured:{}:{}",
            provider_key(&connection.connection),
            connection.id
        );
        candidate.dedup = configured_key.clone();
        candidates.insert(configured_key, candidate);
    }

    let reserved = config
        .connections
        .iter()
        .flat_map(|connection| {
            std::iter::once(connection.id.clone()).chain(match &connection.connection {
                #[cfg(feature = "kubernetes")]
                Connection::Kubernetes(value) => Some(value.context.clone()),
                _ => None,
            })
        })
        .collect::<HashSet<_>>();
    let mut used = reserved.clone();
    let mut sources = vec![StorageSource::local()];
    for (_, mut candidate) in candidates {
        let id = if candidate.persistent {
            candidate.connection.id.clone()
        } else {
            collision_safe_id(&candidate.preferred_id, &mut used)
        };
        if candidate.persistent {
            used.insert(id.clone());
        }
        candidate.connection.id = id.clone();
        let path = match candidate.scheme {
            #[cfg(feature = "kubernetes")]
            "kube" => StoragePath::Kubernetes(Vec::new()),
            _ => StoragePath::Remote(String::new()),
        };
        sources.push(StorageSource {
            id: id.clone(),
            provider: candidate.provider,
            name: candidate.name,
            context: candidate.context,
            endpoint: candidate.endpoint,
            location: Location::Remote(RemoteLocation {
                scheme: candidate.scheme.to_owned(),
                connection: id,
                path,
            }),
            persistent: candidate.persistent,
            connection: Some(candidate.connection),
        });
    }
    sources
}

#[allow(unreachable_code)]
fn configured_candidate(connection: NamedConnection) -> Candidate {
    // `&Connection` is always inhabited even when Connection has no variants.
    let (provider, context, endpoint, scheme, dedup) = match &connection.connection {
        #[cfg(feature = "s3")]
        Connection::S3(value) => {
            let context = value
                .profile
                .clone()
                .unwrap_or_else(|| connection.id.clone());
            (
                s3_provider(value.preset).to_owned(),
                context.clone(),
                s3_endpoint(value),
                "s3",
                format!("s3:{}", value.profile.as_deref().unwrap_or("default-chain")),
            )
        }
        #[cfg(feature = "azure")]
        Connection::Azure(value) => (
            match value.mode {
                AzureMode::Blob => "Azure Blob".to_owned(),
                AzureMode::AdlsGen2 => "Azure ADLS Gen2".to_owned(),
            },
            value.account.clone(),
            value
                .endpoint
                .clone()
                .unwrap_or_else(|| format!("{}.blob.core.windows.net", value.account)),
            match value.mode {
                AzureMode::Blob => "az",
                AzureMode::AdlsGen2 => "adls",
            },
            format!("azure:{:?}:{}", value.mode, value.account),
        ),
        #[cfg(feature = "gcs")]
        Connection::Gcs(value) => (
            "Google Cloud Storage".to_owned(),
            value.project.clone(),
            value.endpoint.clone().unwrap_or_default(),
            "gs",
            format!("gcs:{}", value.project),
        ),
        #[cfg(feature = "kubernetes")]
        Connection::Kubernetes(value) => (
            "Kubernetes".to_owned(),
            value.context.clone(),
            value.namespaces.join(", "),
            "kube",
            format!("kube:{}", value.context),
        ),
        #[cfg(feature = "sftp")]
        Connection::Sftp(value) => (
            "SFTP".to_owned(),
            value.username.clone(),
            format!("{}:{}", value.host, value.port),
            "sftp",
            format!("sftp:{}@{}:{}", value.username, value.host, value.port),
        ),
        #[cfg(feature = "ftp")]
        Connection::Ftp(value) => (
            match value.mode {
                crate::storage::FtpMode::Plain => "FTP".to_owned(),
                _ => "FTPS".to_owned(),
            },
            value.username.clone(),
            format!(
                "{}:{}",
                value.host,
                value.port.unwrap_or(match value.mode {
                    crate::storage::FtpMode::ImplicitTls => 990,
                    _ => 21,
                })
            ),
            match value.mode {
                crate::storage::FtpMode::Plain => "ftp",
                _ => "ftps",
            },
            format!("ftp:{:?}:{}:{}", value.mode, value.host, value.username),
        ),
        Connection::Unsupported => {
            unreachable!("unsupported connections are filtered when loading configuration")
        }
    };
    Candidate {
        dedup,
        preferred_id: connection.id.clone(),
        provider,
        name: connection.name.clone(),
        context,
        endpoint,
        scheme,
        connection,
        persistent: true,
    }
}
