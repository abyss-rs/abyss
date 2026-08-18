#[path = "storage_contract/bulk.rs"]
mod bulk;
#[path = "storage_contract/crud.rs"]
mod crud;

#[cfg(not(feature = "remote"))]
#[test]
fn configured_backend_contract() {
    // remotes disabled; nothing to contract-test
}

#[cfg(feature = "remote")]
mod remote_contract {
    use std::sync::Arc;

    #[cfg(feature = "sftp")]
    use abyss_core::storage::SftpConnection;
    use abyss_core::storage::{
        Connection, ConnectionConfig, FtpConnection, FtpMode, KubernetesConnection, Location,
        LocationCodec, NamedConnection, S3Connection, S3Preset, StorageRuntime,
    };
    use uuid::Uuid;

    use super::crud::run_contract;

    /// Run the common backend contract against any configured provider:
    ///
    /// `ABYSS_CONTRACT_URI=s3://minio/test-bucket cargo test --test storage_contract --features remote`
    ///
    /// The URI must point at a disposable directory-capable root. Live credentials
    /// remain in the provider's normal external credential store. Kubernetes can
    /// be tested without touching the user's config by also setting
    /// `ABYSS_KUBE_HELPER_IMAGE`; the URI authority is used as the kube context.
    #[test]
    fn configured_backend_contract() {
        let Ok(root_uri) = std::env::var("ABYSS_CONTRACT_URI") else {
            return;
        };
        let root = LocationCodec::parse(&root_uri).expect("parse ABYSS_CONTRACT_URI");
        let Location::Remote(root) = root else {
            panic!("ABYSS_CONTRACT_URI must be a remote URI");
        };
        let mut temporary_config = None;
        let storage = if root.scheme == "kube" {
            if let Ok(helper_image) = std::env::var("ABYSS_KUBE_HELPER_IMAGE") {
                let namespace = root
                    .path
                    .components()
                    .first()
                    .and_then(|value| value.to_str())
                    .expect("Kubernetes contract URI must contain a UTF-8 namespace")
                    .to_owned();
                let directory = tempfile::tempdir().expect("create temporary config directory");
                let path = directory.path().join("connections.toml");
                ConnectionConfig {
                    version: 1,
                    connections: vec![NamedConnection {
                        id: root.connection.clone(),
                        name: "Kubernetes contract".to_owned(),
                        connection: Connection::Kubernetes(KubernetesConnection {
                            kubeconfig: std::env::var_os("ABYSS_KUBE_CONFIG")
                                .map(|value| std::env::split_paths(&value).collect())
                                .unwrap_or_default(),
                            context: root.connection.clone(),
                            namespaces: vec![namespace],
                            helper_image,
                            helper_images: Vec::new(),
                            image_pull_secrets: Vec::new(),
                            service_account: None,
                            run_as_user: None,
                            run_as_group: None,
                            fs_group: None,
                            migration_workers: 4,
                        }),
                    }],
                }
                .save(&path)
                .expect("save temporary Kubernetes connection");
                let runtime =
                    StorageRuntime::load(&path).expect("load temporary Kubernetes connection");
                temporary_config = Some(directory);
                runtime
            } else {
                StorageRuntime::load_default().expect("load connection configuration")
            }
        } else if root.scheme == "s3" {
            if let Ok(endpoint) = std::env::var("ABYSS_S3_ENDPOINT") {
                let bucket = root
                    .path
                    .components()
                    .first()
                    .and_then(|value| value.to_str())
                    .expect("S3 contract URI must contain a UTF-8 bucket")
                    .to_owned();
                let directory = tempfile::tempdir().expect("create temporary config directory");
                let path = directory.path().join("connections.toml");
                ConnectionConfig {
                    version: 1,
                    connections: vec![NamedConnection {
                        id: root.connection.clone(),
                        name: "S3 contract".to_owned(),
                        connection: Connection::S3(S3Connection {
                            preset: S3Preset::Custom,
                            endpoint: Some(endpoint),
                            region: Some(
                                std::env::var("ABYSS_S3_REGION")
                                    .unwrap_or_else(|_| "us-east-1".to_owned()),
                            ),
                            profile: None,
                            account_id: None,
                            force_path_style: Some(true),
                            buckets: vec![bucket],
                            disable_payload_signing: false,
                            disable_checksums: false,
                            disable_multipart: false,
                        }),
                    }],
                }
                .save(&path)
                .expect("save temporary S3 connection");
                let runtime = StorageRuntime::load(&path).expect("load temporary S3 connection");
                temporary_config = Some(directory);
                runtime
            } else {
                StorageRuntime::load_default().expect("load connection configuration")
            }
        } else if root.scheme == "sftp" {
            #[cfg(feature = "sftp")]
            {
                let directory = tempfile::tempdir().expect("create temporary config directory");
                let path = directory.path().join("connections.toml");
                ConnectionConfig {
                    version: 1,
                    connections: vec![NamedConnection {
                        id: root.connection.clone(),
                        name: "SFTP contract".to_owned(),
                        connection: Connection::Sftp(SftpConnection {
                            host: std::env::var("ABYSS_SFTP_HOST")
                                .unwrap_or_else(|_| "127.0.0.1".to_owned()),
                            port: std::env::var("ABYSS_SFTP_PORT")
                                .ok()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(2222),
                            username: std::env::var("ABYSS_SFTP_USER")
                                .unwrap_or_else(|_| "testuser".to_owned()),
                            root: std::env::var("ABYSS_SFTP_ROOT")
                                .unwrap_or_else(|_| "/upload".to_owned()),
                            private_key: None,
                            password_env: Some("ABYSS_SFTP_PASSWORD".to_owned()),
                            password_command: vec![],
                            known_hosts: Some(directory.path().join("known_hosts")),
                            accept_new_host_keys: true,
                        }),
                    }],
                }
                .save(&path)
                .expect("save temporary SFTP connection");
                let runtime = StorageRuntime::load(&path).expect("load temporary SFTP connection");
                temporary_config = Some(directory);
                runtime
            }
            #[cfg(not(feature = "sftp"))]
            {
                panic!("SFTP support is not enabled in this build");
            }
        } else if root.scheme == "ftp" || root.scheme == "ftps" {
            let directory = tempfile::tempdir().expect("create temporary config directory");
            let path = directory.path().join("connections.toml");
            ConnectionConfig {
                version: 1,
                connections: vec![NamedConnection {
                    id: root.connection.clone(),
                    name: "FTP contract".to_owned(),
                    connection: Connection::Ftp(FtpConnection {
                        host: std::env::var("ABYSS_FTP_HOST")
                            .unwrap_or_else(|_| "127.0.0.1".to_owned()),
                        port: std::env::var("ABYSS_FTP_PORT")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .or(Some(2121)),
                        username: std::env::var("ABYSS_FTP_USER")
                            .unwrap_or_else(|_| "testuser".to_owned()),
                        password_env: Some("ABYSS_FTP_PASSWORD".to_owned()),
                        root: String::new(),
                        mode: if root.scheme == "ftps" {
                            FtpMode::ExplicitTls
                        } else {
                            FtpMode::Plain
                        },
                    }),
                }],
            }
            .save(&path)
            .expect("save temporary FTP connection");
            let runtime = StorageRuntime::load(&path).expect("load temporary FTP connection");
            temporary_config = Some(directory);
            runtime
        } else {
            StorageRuntime::load_default().expect("load connection configuration")
        };
        let sources = storage.refresh_sources();
        let source_location = sources
            .iter()
            .find_map(|source| {
                let Location::Remote(location) = &source.location else {
                    return None;
                };
                (location.scheme == root.scheme
                    && (location.connection == root.connection
                        || source.context == root.connection))
                    .then_some(location)
            })
            .expect("contract connection was not exposed as a storage source");
        let backend = storage
            .backend(source_location)
            .expect("open backend through discovered source");
        let test_root = root
            .path
            .child(format!("abyss-contract-{}", Uuid::new_v4().simple()).as_bytes())
            .expect("create test root path");

        let result = storage.block_on(run_contract(
            Arc::clone(&backend),
            &test_root,
            root.scheme == "kube",
        ));
        if result.is_ok()
            && root.scheme == "kube"
            && std::env::var_os("ABYSS_KUBE_EVICTION_TEST").is_some()
        {
            let namespace = root
                .path
                .components()
                .first()
                .and_then(|value| value.to_str())
                .expect("Kubernetes contract URI must contain a namespace")
                .to_owned();
            let status = std::process::Command::new("kubectl")
                .args([
                    "delete",
                    "pod",
                    "-n",
                    &namespace,
                    "-l",
                    "app.kubernetes.io/created-by=abyss",
                    "--wait=true",
                ])
                .status()
                .expect("run kubectl for helper eviction test");
            assert!(status.success(), "kubectl could not evict the helper pod");
            let stale = storage.block_on(backend.list(&test_root, None));
            assert!(
                stale.is_err(),
                "cached evicted helper unexpectedly remained usable"
            );
            storage
                .block_on(backend.list(&test_root, None))
                .expect("backend did not recreate an evicted helper");
        }
        let cleanup = storage.block_on(backend.delete(&test_root, true));
        let _ = storage.shutdown();
        drop(temporary_config);
        if let Err(ref error) = result {
            eprintln!("CONTRACT FAILURE: {error:?}");
            eprintln!("CLEANUP RESULT:  {cleanup:?}");
            panic!("backend contract failed: {error}");
        }
        cleanup.expect("clean contract test root");
    }
}
