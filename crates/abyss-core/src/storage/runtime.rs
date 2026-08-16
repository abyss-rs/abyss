use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::runtime::{Builder, Runtime};

use super::{
    Connection, ConnectionConfig, DiscoveryEnvironment, ErrorKind, NamedConnection,
    ProviderRegistry, RemoteLocation, StorageBackend, StorageError, StorageSource,
    discover_sources,
};

pub struct StorageRuntime {
    runtime: Arc<Runtime>,
    registry: Arc<ProviderRegistry>,
    config: ConnectionConfig,
    discovered: Mutex<HashMap<String, NamedConnection>>,
    backends: Mutex<HashMap<String, Arc<dyn StorageBackend>>>,
}

impl StorageRuntime {
    pub fn load_default() -> Result<Arc<Self>, StorageError> {
        let path = ConnectionConfig::default_path()?;
        Self::load(&path)
    }

    pub fn load(path: &Path) -> Result<Arc<Self>, StorageError> {
        // Several current cloud SDKs select aws-lc while kube enables ring.
        // rustls deliberately refuses to guess when both providers are linked,
        // so choose the provider used by the AWS, Azure, and GCS clients before
        // any TLS client is constructed. A host application may have installed
        // its own provider already; in that case install_default safely leaves
        // it in place.
        #[cfg(any(
            feature = "s3",
            feature = "azure",
            feature = "gcs",
            feature = "kubernetes",
            feature = "ftp"
        ))]
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = ConnectionConfig::load(path)?;
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("abyss-storage")
            .build()
            .map_err(|error| {
                StorageError::new(ErrorKind::Other, format!("create storage runtime: {error}"))
            })?;
        Ok(Arc::new(Self {
            runtime: Arc::new(runtime),
            registry: Arc::new(ProviderRegistry::with_builtin_providers()),
            config,
            discovered: Mutex::new(HashMap::new()),
            backends: Mutex::new(HashMap::new()),
        }))
    }

    pub fn config(&self) -> &ConnectionConfig {
        &self.config
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// Rediscover session-only sources and atomically replace the runtime
    /// overlay. Persistent connections always take precedence during lookup.
    pub fn refresh_sources(&self) -> Vec<StorageSource> {
        self.refresh_sources_with(&DiscoveryEnvironment::capture())
    }

    pub fn refresh_sources_with(&self, environment: &DiscoveryEnvironment) -> Vec<StorageSource> {
        let sources = discover_sources(&self.config, environment);
        let replacement = sources
            .iter()
            .filter(|source| !source.persistent)
            .filter_map(|source| {
                source
                    .connection
                    .clone()
                    .map(|connection| (connection.id.clone(), connection))
            })
            .collect::<HashMap<_, _>>();
        let old = {
            let mut discovered = self
                .discovered
                .lock()
                .unwrap_or_else(|value| value.into_inner());
            std::mem::replace(&mut *discovered, replacement.clone())
        };

        // Successful discovered backends remain cached between refreshes.
        // Remove only sources that disappeared or whose inferred metadata
        // changed.
        let changed = old
            .iter()
            .filter(|(id, connection)| replacement.get(*id) != Some(*connection))
            .map(|(id, _)| id.clone())
            .chain(
                replacement
                    .iter()
                    .filter(|(id, connection)| old.get(*id) != Some(*connection))
                    .map(|(id, _)| id.clone()),
            )
            .collect::<std::collections::HashSet<_>>();
        if !changed.is_empty() {
            let evicted = {
                let mut backends = self
                    .backends
                    .lock()
                    .unwrap_or_else(|value| value.into_inner());
                let keys = backends
                    .keys()
                    .filter(|key| {
                        key.split_once(':')
                            .is_some_and(|(_, id)| changed.contains(id))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                keys.into_iter()
                    .filter_map(|key| backends.remove(&key))
                    .collect::<Vec<_>>()
            };
            self.block_on(async move {
                for backend in evicted {
                    let _ = backend.shutdown().await;
                }
            });
        }
        sources
    }

    pub fn backend(
        &self,
        location: &RemoteLocation,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        self.block_on(self.backend_async(location))
    }

    pub async fn backend_async(
        &self,
        location: &RemoteLocation,
    ) -> Result<Arc<dyn StorageBackend>, StorageError> {
        let key = format!("{}:{}", location.scheme, location.connection);
        if let Some(backend) = self
            .backends
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(backend);
        }
        let persistent = self.config.connections.iter().find(|connection| {
            connection.id == location.connection || {
                #[cfg(feature = "kubernetes")]
                {
                    matches!(
                        &connection.connection,
                        Connection::Kubernetes(config)
                            if location.scheme == "kube"
                                && config.context == location.connection
                    )
                }
                #[cfg(not(feature = "kubernetes"))]
                {
                    false
                }
            }
        });
        let discovered = if persistent.is_none() {
            self.discovered
                .lock()
                .unwrap_or_else(|value| value.into_inner())
                .get(&location.connection)
                .cloned()
        } else {
            None
        };
        let named = persistent.cloned().or(discovered).ok_or_else(|| {
            StorageError::new(
                ErrorKind::NotFound,
                format!(
                    "connection '{}' is not configured in {}",
                    location.connection,
                    ConnectionConfig::default_path()
                        .map(|value| value.display().to_string())
                        .unwrap_or_else(|_| "connections.toml".to_owned())
                ),
            )
        })?;
        let provider = provider_id(&named.connection, &location.scheme)?;
        let factory = self.registry.factory(provider).ok_or_else(|| {
            StorageError::new(
                ErrorKind::Unsupported,
                format!("storage provider '{provider}' is not available"),
            )
        })?;
        let backend = factory
            .create(named.id.clone(), named.connection.clone())
            .await?;
        self.backends
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .insert(key, Arc::clone(&backend));
        Ok(backend)
    }

    pub fn shutdown(&self) -> Result<(), StorageError> {
        let backends = self
            .backends
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        self.block_on(async move {
            let mut first_error = None;
            for backend in backends {
                if let Err(error) = backend.shutdown().await
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            first_error.map_or(Ok(()), Err)
        })
    }
}

fn provider_id(connection: &Connection, scheme: &str) -> Result<&'static str, StorageError> {
    match (connection, scheme) {
        #[cfg(feature = "s3")]
        (Connection::S3(_), "s3") => Ok("s3"),
        #[cfg(feature = "azure")]
        (Connection::Azure(config), "az") if config.mode == super::AzureMode::Blob => Ok("azure"),
        #[cfg(feature = "azure")]
        (Connection::Azure(config), "adls") if config.mode == super::AzureMode::AdlsGen2 => {
            Ok("azure")
        }
        #[cfg(feature = "gcs")]
        (Connection::Gcs(_), "gs") => Ok("gcs"),
        #[cfg(feature = "kubernetes")]
        (Connection::Kubernetes(_), "kube") => Ok("kubernetes"),
        #[cfg(feature = "sftp")]
        (Connection::Sftp(_), "sftp") => Ok("sftp"),
        #[cfg(feature = "ftp")]
        (Connection::Ftp(config), "ftp") if config.mode == super::FtpMode::Plain => Ok("ftp"),
        #[cfg(feature = "ftp")]
        (Connection::Ftp(config), "ftps") if config.mode != super::FtpMode::Plain => Ok("ftp"),
        _ => Err(StorageError::new(
            ErrorKind::InvalidInput,
            format!("connection provider does not support the '{scheme}' URI scheme"),
        )),
    }
}
