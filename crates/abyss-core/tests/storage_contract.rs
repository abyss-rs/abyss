#[cfg(not(feature = "remote"))]
#[test]
fn configured_backend_contract() {
    // remotes disabled; nothing to contract-test
}

#[cfg(feature = "remote")]
mod remote_contract {
    use std::sync::Arc;

    use abyss_core::storage::{
        ByteStream, Connection, ConnectionConfig, EntryKind, FtpConnection, FtpMode,
        KubernetesConnection, Location, LocationCodec, NamedConnection, ReadOptions, S3Connection,
        S3Preset, SftpConnection, StorageRuntime, TreeEntry, TreeWriteEntry, WriteOptions,
    };
    use bytes::Bytes;
    use futures_util::StreamExt;
    use uuid::Uuid;

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
                            run_as_user: Some(65_532),
                            run_as_group: Some(65_532),
                            fs_group: Some(65_532),
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
                            preset: S3Preset::Minio,
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

    async fn run_contract(
        backend: Arc<dyn abyss_core::storage::StorageBackend>,
        root: &abyss_core::storage::StoragePath,
        supports_raw_names: bool,
    ) -> Result<(), abyss_core::storage::StorageError> {
        backend.create_dir(root).await?;
        let source = root.child(b"source.bin")?;
        let copied = root.child(b"copied.bin")?;
        let renamed = root.child(b"renamed.bin")?;
        let content = Bytes::from_static(b"0123456789abcdef");
        backend
            .write(
                &source,
                one_chunk(content.clone()),
                WriteOptions {
                    size: Some(content.len() as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await?;

        let stat = backend.stat(&source).await?;
        assert_eq!(stat.kind, EntryKind::File);
        assert_eq!(stat.size, Some(content.len() as u64));

        let mut names = Vec::new();
        let mut continuation = None;
        loop {
            let page = backend.list(root, continuation.as_deref()).await?;
            names.extend(page.entries.into_iter().map(|entry| entry.name));
            continuation = page.continuation;
            if continuation.is_none() {
                break;
            }
        }
        assert!(names.iter().any(|name| name == b"source.bin"));

        let read = collect(
            backend
                .read(
                    &source,
                    ReadOptions {
                        offset: Some(4),
                        length: Some(6),
                        expected_version: stat.version.clone(),
                    },
                )
                .await?,
        )
        .await?;
        assert_eq!(read.as_ref(), b"456789");

        if backend.capabilities().server_side_copy {
            backend.copy(&source, &copied, false).await?;
            assert_eq!(
                collect(backend.read(&copied, ReadOptions::default()).await?).await?,
                content
            );

            backend.rename(&copied, &renamed, false).await?;
            assert_eq!(
                collect(backend.read(&renamed, ReadOptions::default()).await?).await?,
                content
            );
        } else {
            backend.rename(&source, &renamed, false).await?;
            assert_eq!(
                collect(backend.read(&renamed, ReadOptions::default()).await?).await?,
                content
            );
        }

        let existing_file = if backend.capabilities().server_side_copy {
            &source
        } else {
            &renamed
        };

        let duplicate = backend
            .write(
                existing_file,
                one_chunk(Bytes::from_static(b"must-not-overwrite")),
                WriteOptions {
                    size: Some(18),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await;
        assert!(
            duplicate.is_err(),
            "conditional create silently overwrote a file"
        );

        let broken = root.child(b"incomplete.bin")?;
        let incomplete = backend
            .write(
                &broken,
                one_chunk(Bytes::from_static(b"short")),
                WriteOptions {
                    size: Some(32),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await;
        assert!(
            incomplete.is_err(),
            "truncated upload unexpectedly succeeded"
        );
        assert!(
            backend.stat(&broken).await.is_err(),
            "truncated upload left a visible destination"
        );

        let large = root.child(b"large.bin")?;
        let large_copy = root.child(b"large-copy.bin")?;
        let large_size: usize = std::env::var("ABYSS_CONTRACT_LARGE_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4 * 1024 * 1024);
        let large_content = Bytes::from(
            (0..large_size)
                .map(|index| ((index * 31 + 7) % 251) as u8)
                .collect::<Vec<_>>(),
        );
        if large_size > 64 * 1024 * 1024 {
            let interrupted = root.child(b"interrupted-multipart.bin")?;
            let interrupted_source = Box::pin(futures_util::stream::iter(vec![
                Ok(Bytes::from(vec![0x5a; 16 * 1024 * 1024])),
                Err(abyss_core::storage::StorageError::new(
                    abyss_core::storage::ErrorKind::Cancelled,
                    "intentional multipart interruption",
                )),
            ])) as ByteStream;
            let result = backend
                .write(
                    &interrupted,
                    interrupted_source,
                    WriteOptions {
                        size: Some(large_size as u64),
                        overwrite: false,
                        expected_version: None,
                    },
                )
                .await;
            assert!(result.is_err(), "interrupted multipart upload succeeded");
            assert!(
                backend.stat(&interrupted).await.is_err(),
                "interrupted multipart upload left a visible destination"
            );
        }
        backend
            .write(
                &large,
                chunks(large_content.clone(), 63 * 1024),
                WriteOptions {
                    size: Some(large_content.len() as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await?;
        let ranged = collect(
            backend
                .read(
                    &large,
                    ReadOptions {
                        offset: Some(1_000_003),
                        length: Some(700_019),
                        expected_version: None,
                    },
                )
                .await?,
        )
        .await?;
        assert_eq!(
            ranged.as_ref(),
            &large_content[1_000_003..1_700_022],
            "large ranged read returned different bytes"
        );

        let mut abandoned = backend.read(&large, ReadOptions::default()).await?;
        assert!(abandoned.next().await.transpose()?.is_some());
        drop(abandoned);
        assert_eq!(
            backend.stat(&large).await?.size,
            Some(large_content.len() as u64),
            "aborting a read damaged its source"
        );

        if backend.capabilities().server_side_copy {
            backend.copy(&large, &large_copy, false).await?;
            let copied_tail = collect(
                backend
                    .read(
                        &large_copy,
                        ReadOptions {
                            offset: Some((large_content.len() - 131_071) as u64),
                            length: None,
                            expected_version: None,
                        },
                    )
                    .await?,
            )
            .await?;
            assert_eq!(
                copied_tail.as_ref(),
                &large_content[large_content.len() - 131_071..]
            );
        }

        let nested = root.child(b"nested")?;
        let nested_child = nested.child(b"child")?;
        let nested_file = nested_child.child(b"value.bin")?;
        backend.create_dir(&nested_child).await?;
        backend
            .write(
                &nested_file,
                one_chunk(Bytes::from_static(b"nested")),
                WriteOptions {
                    size: Some(6),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await?;
        backend.delete(&nested, true).await?;
        assert!(
            backend.stat(&nested).await.is_err(),
            "recursive directory delete left the directory visible"
        );

        if supports_raw_names {
            let raw_name = b"raw-\xFF.bin";
            let raw = root.child(raw_name)?;
            backend
                .write(
                    &raw,
                    one_chunk(Bytes::from_static(b"raw-name")),
                    WriteOptions {
                        size: Some(8),
                        overwrite: false,
                        expected_version: None,
                    },
                )
                .await?;
            let entries = backend.list(root, None).await?.entries;
            assert!(
                entries.iter().any(|entry| entry.name == raw_name),
                "non-UTF-8 filename did not round-trip through listing"
            );
            assert_eq!(
                collect(backend.read(&raw, ReadOptions::default()).await?).await?,
                Bytes::from_static(b"raw-name")
            );
            backend.delete(&raw, false).await?;

            #[cfg(feature = "kubernetes")]
            run_kubernetes_bulk_contract(Arc::clone(&backend), root).await?;
        }

        if backend.capabilities().server_side_copy {
            backend.delete(&large_copy, false).await?;
        }
        backend.delete(&large, false).await?;
        backend.delete(&renamed, false).await?;
        Ok(())
    }

    #[cfg(feature = "kubernetes")]
    async fn run_kubernetes_bulk_contract(
        backend: Arc<dyn abyss_core::storage::StorageBackend>,
        root: &abyss_core::storage::StoragePath,
    ) -> Result<(), abyss_core::storage::StorageError> {
        let source = root.child(b"bulk-source")?;
        let copied = root.child(b"bulk-copy")?;
        let file_count = std::env::var("ABYSS_KUBE_BULK_FILES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1_000);
        let mut entries = vec![TreeEntry {
            path: vec![b"nested".to_vec()],
            kind: EntryKind::Directory,
            size: 0,
        }];
        let mut expected = Vec::new();
        for index in 0..file_count {
            let contents = format!("bulk-file-{index:06}-").repeat(32).into_bytes();
            expected.extend_from_slice(&contents);
            entries.push(TreeEntry {
                path: vec![
                    b"nested".to_vec(),
                    format!("file-{index:06}.txt").into_bytes(),
                ],
                kind: EntryKind::File,
                size: contents.len() as u64,
            });
        }
        let writes = entries
            .iter()
            .cloned()
            .map(|entry| TreeWriteEntry {
                entry,
                overwrite: false,
                clone_from: None,
            })
            .collect::<Vec<_>>();
        backend
            .write_tree(
                &source,
                writes.clone(),
                one_chunk(Bytes::from(expected.clone())),
                None,
            )
            .await?;

        let listed = backend.list_tree(&source).await?;
        assert_eq!(
            listed
                .iter()
                .filter(|entry| entry.kind == EntryKind::File)
                .count(),
            file_count
        );
        let files = entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .cloned()
            .collect::<Vec<_>>();
        let downloaded = collect(backend.read_tree(&source, files, None).await?).await?;
        assert_eq!(downloaded.as_ref(), expected.as_slice());

        let states = backend.inspect_tree(&source, &entries).await?;
        assert!(states.iter().all(Option::is_some));
        backend.copy_tree(&source, &copied, writes).await?;
        let copied_files = backend.list_tree(&copied).await?;
        assert_eq!(
            copied_files
                .iter()
                .filter(|entry| entry.kind == EntryKind::File)
                .count(),
            file_count
        );
        backend.delete(&copied, true).await?;
        backend.delete(&source, true).await?;
        Ok(())
    }

    fn one_chunk(value: Bytes) -> ByteStream {
        Box::pin(futures_util::stream::once(async move { Ok(value) }).fuse())
    }

    fn chunks(value: Bytes, chunk_size: usize) -> ByteStream {
        Box::pin(
            futures_util::stream::unfold((value, 0), move |(value, offset)| async move {
                if offset >= value.len() {
                    return None;
                }
                let end = (offset + chunk_size).min(value.len());
                let chunk = value.slice(offset..end);
                Some((Ok(chunk), (value, end)))
            })
            .fuse(),
        )
    }

    async fn collect(mut stream: ByteStream) -> Result<Bytes, abyss_core::storage::StorageError> {
        let mut output = Vec::new();
        while let Some(chunk) = stream.next().await {
            output.extend_from_slice(&chunk?);
        }
        Ok(Bytes::from(output))
    }
}
