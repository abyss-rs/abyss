#[cfg(not(feature = "ftp"))]
#[test]
fn ftp_docker_contract() {
    // FTP feature not enabled in this build
}

#[cfg(feature = "ftp")]
mod ftp_docker_tests {
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;
    use std::time::Duration;

    use abyss_core::storage::{
        ByteStream, Connection, ConnectionConfig, EntryKind, ErrorKind, FtpConnection, FtpFactory,
        FtpMode, Location, LocationCodec, NamedConnection, ReadOptions, StorageBackend,
        StorageProviderFactory, StorageRuntime, WriteOptions,
    };
    use abyss_core::sync::{SyncComparison, SyncStrategy, plan_locations};
    use bytes::Bytes;
    use futures_util::StreamExt;
    use uuid::Uuid;

    struct DockerFtpGuard {
        container_name: String,
    }

    impl Drop for DockerFtpGuard {
        fn drop(&mut self) {
            let _ = Command::new("docker")
                .args(["rm", "-f", &self.container_name])
                .output();
        }
    }

    fn is_docker_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn start_ftp_container(
        name: &str,
        port: u16,
        pasv_start: u16,
        pasv_end: u16,
    ) -> Option<DockerFtpGuard> {
        if !is_docker_available() {
            eprintln!("Docker is not available; skipping live FTP Docker tests");
            return None;
        }

        // Clean up any stale container with the same name
        let _ = Command::new("docker").args(["rm", "-f", name]).output();

        let run_status = Command::new("docker")
            .args([
                "run",
                "--rm",
                "-d",
                "--name",
                name,
                "-p",
                &format!("{port}:{port}"),
                "-p",
                &format!("{pasv_start}-{pasv_end}:{pasv_start}-{pasv_end}"),
                "python:3.11-alpine",
                "sh",
                "-c",
                &format!(
                    "pip install pyftpdlib && python -m pyftpdlib -p {port} -u testuser -P testpass -d /tmp -w -n 127.0.0.1 -r {pasv_start}-{pasv_end}"
                ),
            ])
            .status();

        if !run_status.map(|s| s.success()).unwrap_or(false) {
            eprintln!("Failed to start Docker FTP container");
            return None;
        }

        let guard = DockerFtpGuard {
            container_name: name.to_owned(),
        };

        // Wait for pyftpdlib server to become ready
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(300));
            let logs = Command::new("docker")
                .args(["logs", name])
                .output()
                .map(|out| {
                    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                    text
                })
                .unwrap_or_default();
            if logs.contains("starting FTP server") {
                // Short grace period to ensure port is listening
                std::thread::sleep(Duration::from_millis(200));
                return Some(guard);
            }
        }

        eprintln!("FTP container did not become ready in time");
        None
    }

    fn one_chunk(value: Bytes) -> ByteStream {
        Box::pin(futures_util::stream::once(async move { Ok(value) }))
    }

    fn chunks(value: Bytes, chunk_size: usize) -> ByteStream {
        Box::pin(futures_util::stream::unfold(
            (value, 0),
            move |(value, offset)| async move {
                if offset >= value.len() {
                    return None;
                }
                let end = (offset + chunk_size).min(value.len());
                let chunk = value.slice(offset..end);
                Some((Ok(chunk), (value, end)))
            },
        ))
    }

    async fn collect(mut stream: ByteStream) -> Result<Bytes, abyss_core::storage::StorageError> {
        let mut output = Vec::new();
        while let Some(chunk) = stream.next().await {
            output.extend_from_slice(&chunk?);
        }
        Ok(Bytes::from(output))
    }

    #[tokio::test]
    async fn test_ftp_crud_lifecycle_with_docker() {
        let port = 21210;
        let pasv_start = 21211;
        let pasv_end = 21220;
        let container_name = format!("abyss-ftp-test-{}", Uuid::new_v4().simple());

        let _guard = match start_ftp_container(&container_name, port, pasv_start, pasv_end) {
            Some(guard) => guard,
            None => return,
        };

        let factory = FtpFactory;
        let connection = Connection::Ftp(FtpConnection {
            host: "127.0.0.1".to_owned(),
            port: Some(port),
            username: "testuser".to_owned(),
            password_env: Some("ABYSS_TEST_DOCKER_FTP_PASS".to_owned()),
            root: String::new(),
            mode: FtpMode::Plain,
        });

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_FTP_PASS", "testpass");
        }

        let backend: Arc<dyn StorageBackend> = factory
            .create("docker-ftp".to_owned(), connection)
            .await
            .expect("create FTP backend");

        let test_root_name = format!("test-root-{}", Uuid::new_v4().simple());
        let root =
            LocationCodec::parse(&format!("ftp://testuser@127.0.0.1:{port}/{test_root_name}"))
                .expect("parse root location");
        let Location::Remote(root_location) = root else {
            panic!("expected remote location");
        };
        let root_path = root_location.path;

        // 1. Create root directory
        backend
            .create_dir(&root_path)
            .await
            .expect("create root dir");

        // 2. Stat root directory
        let root_stat = backend.stat(&root_path).await.expect("stat root directory");
        assert_eq!(root_stat.kind, EntryKind::Directory);

        // 3. Write small file
        let file_path = root_path.child(b"hello.txt").expect("child path");
        let initial_payload = Bytes::from_static(b"Hello FTP Storage World!");
        backend
            .write(
                &file_path,
                one_chunk(initial_payload.clone()),
                WriteOptions {
                    size: Some(initial_payload.len() as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect("write hello.txt");

        // 4. Stat file
        let stat = backend.stat(&file_path).await.expect("stat hello.txt");
        assert_eq!(stat.kind, EntryKind::File);
        assert_eq!(stat.size, Some(initial_payload.len() as u64));
        let version_1 = stat.version.expect("file version");

        // 5. Read full file
        let read_back = collect(
            backend
                .read(&file_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .expect("read back");
        assert_eq!(read_back, initial_payload);

        // 6. Ranged read
        let slice = collect(
            backend
                .read(
                    &file_path,
                    ReadOptions {
                        offset: Some(6),
                        length: Some(11),
                        expected_version: None,
                    },
                )
                .await
                .unwrap(),
        )
        .await
        .expect("ranged read");
        assert_eq!(slice.as_ref(), b"FTP Storage");

        // 7. Abandoned read (partial consume and drop)
        let mut abandoned = backend
            .read(&file_path, ReadOptions::default())
            .await
            .unwrap();
        assert!(abandoned.next().await.transpose().unwrap().is_some());
        drop(abandoned);

        // Verify connection remains completely healthy after abandoned read
        let stat_after_abandon = backend
            .stat(&file_path)
            .await
            .expect("stat after abandoned read");
        assert_eq!(stat_after_abandon.size, Some(initial_payload.len() as u64));

        // 8. Attempt overwrite with overwrite: false (MUST fail)
        let duplicate_attempt = backend
            .write(
                &file_path,
                one_chunk(Bytes::from_static(b"conflict")),
                WriteOptions {
                    size: Some(8),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await;
        assert!(
            duplicate_attempt.is_err(),
            "write with overwrite=false on existing file must fail"
        );

        // 9. Unconditional overwrite (overwrite: true, expected_version: None) (MUST succeed)
        let overwritten_payload =
            Bytes::from_static(b"New updated longer content for FTP overwrite test!");
        backend
            .write(
                &file_path,
                one_chunk(overwritten_payload.clone()),
                WriteOptions {
                    size: Some(overwritten_payload.len() as u64),
                    overwrite: true,
                    expected_version: None,
                },
            )
            .await
            .expect("unconditional overwrite must succeed");

        let read_overwritten = collect(
            backend
                .read(&file_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .expect("read overwritten");
        assert_eq!(read_overwritten, overwritten_payload);

        // 10. Conditional overwrite with stale expected_version (MUST fail)
        let stale_overwrite = backend
            .write(
                &file_path,
                one_chunk(Bytes::from_static(b"stale update")),
                WriteOptions {
                    size: Some(12),
                    overwrite: true,
                    expected_version: Some(version_1), // old version
                },
            )
            .await;
        assert!(
            stale_overwrite.is_err(),
            "conditional write with stale version must fail"
        );

        // 11. Conditional overwrite with correct expected_version (MUST succeed)
        let stat_v2 = backend.stat(&file_path).await.expect("stat v2");
        let version_2 = stat_v2.version.expect("version 2");
        let versioned_payload = Bytes::from_static(b"Version 3 final content");
        backend
            .write(
                &file_path,
                one_chunk(versioned_payload.clone()),
                WriteOptions {
                    size: Some(versioned_payload.len() as u64),
                    overwrite: true,
                    expected_version: Some(version_2),
                },
            )
            .await
            .expect("conditional write with matching version must succeed");

        // 12. Nested directory upload with auto-parent creation
        let nested_file_path = root_path
            .child(b"deep")
            .unwrap()
            .child(b"nested")
            .unwrap()
            .child(b"child_file.bin")
            .unwrap();
        let nested_content = Bytes::from_static(b"nested-content-auto-parents");
        backend
            .write(
                &nested_file_path,
                one_chunk(nested_content.clone()),
                WriteOptions {
                    size: Some(nested_content.len() as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect("write nested path with auto parent creation");

        let read_nested = collect(
            backend
                .read(&nested_file_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .expect("read nested");
        assert_eq!(read_nested, nested_content);

        // 13. File rename
        let renamed_path = root_path.child(b"renamed_file.bin").unwrap();
        backend
            .rename(&file_path, &renamed_path, false)
            .await
            .expect("rename file");
        assert!(
            backend.stat(&file_path).await.is_err(),
            "original file must be gone after rename"
        );
        let read_renamed = collect(
            backend
                .read(&renamed_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .expect("read renamed");
        assert_eq!(read_renamed, versioned_payload);

        // 14. Large 4MB multi-chunk file transfer
        let large_path = root_path.child(b"large_4mb.bin").unwrap();
        let large_size = 4 * 1024 * 1024;
        let large_bytes = Bytes::from((0..large_size).map(|i| (i % 251) as u8).collect::<Vec<_>>());
        backend
            .write(
                &large_path,
                chunks(large_bytes.clone(), 64 * 1024),
                WriteOptions {
                    size: Some(large_size as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect("write 4MB file");

        let large_slice = collect(
            backend
                .read(
                    &large_path,
                    ReadOptions {
                        offset: Some(1_500_000),
                        length: Some(500_000),
                        expected_version: None,
                    },
                )
                .await
                .unwrap(),
        )
        .await
        .expect("read large slice");
        assert_eq!(large_slice.as_ref(), &large_bytes[1_500_000..2_000_000]);

        // 15. Listing entries
        let list_page = backend.list(&root_path, None).await.expect("list root");
        let names = list_page
            .entries
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|n| n == b"renamed_file.bin"));
        assert!(names.iter().any(|n| n == b"deep"));
        assert!(names.iter().any(|n| n == b"large_4mb.bin"));

        // 16. Recursive deletion
        backend
            .delete(&root_path, true)
            .await
            .expect("recursive delete");
        assert!(
            backend.stat(&root_path).await.is_err(),
            "deleted tree must be gone"
        );
    }

    #[tokio::test]
    async fn test_ftp_zero_byte_and_rollback_with_docker() {
        let port = 21230;
        let pasv_start = 21231;
        let pasv_end = 21240;
        let container_name = format!("abyss-ftp-edge-{}", Uuid::new_v4().simple());

        let _guard = match start_ftp_container(&container_name, port, pasv_start, pasv_end) {
            Some(guard) => guard,
            None => return,
        };

        let factory = FtpFactory;
        let connection = Connection::Ftp(FtpConnection {
            host: "127.0.0.1".to_owned(),
            port: Some(port),
            username: "testuser".to_owned(),
            password_env: Some("ABYSS_TEST_DOCKER_FTP_PASS".to_owned()),
            root: String::new(),
            mode: FtpMode::Plain,
        });

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_FTP_PASS", "testpass");
        }

        let backend = factory
            .create("docker-ftp-edge".to_owned(), connection)
            .await
            .expect("create FTP backend");

        let test_root_name = format!("test-edge-{}", Uuid::new_v4().simple());
        let root =
            LocationCodec::parse(&format!("ftp://testuser@127.0.0.1:{port}/{test_root_name}"))
                .expect("parse root location");
        let Location::Remote(root_location) = root else {
            panic!("expected remote location");
        };
        let root_path = root_location.path;
        backend
            .create_dir(&root_path)
            .await
            .expect("create root dir");

        // 1. Zero-byte file write and read
        let zero_path = root_path.child(b"empty.txt").unwrap();
        backend
            .write(
                &zero_path,
                one_chunk(Bytes::new()),
                WriteOptions {
                    size: Some(0),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect("write 0-byte file");

        let zero_stat = backend.stat(&zero_path).await.expect("stat empty file");
        assert_eq!(zero_stat.size, Some(0));
        let zero_content = collect(
            backend
                .read(&zero_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .expect("read 0-byte file");
        assert_eq!(zero_content.len(), 0);

        // 2. Stat non-existent file returns NotFound
        let missing = root_path.child(b"does_not_exist.bin").unwrap();
        let stat_missing = backend.stat(&missing).await;
        assert!(stat_missing.is_err());
        assert_eq!(stat_missing.unwrap_err().kind, ErrorKind::NotFound);

        // 3. Truncated upload error simulation and rollback verification
        let broken_path = root_path.child(b"truncated.bin").unwrap();
        let broken_source = Box::pin(futures_util::stream::iter(vec![
            Ok(Bytes::from_static(b"partial payload")),
            Err(abyss_core::storage::StorageError::new(
                ErrorKind::Transport,
                "intentional network disconnect",
            )),
        ])) as ByteStream;

        let broken_result = backend
            .write(
                &broken_path,
                broken_source,
                WriteOptions {
                    size: Some(1024),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await;
        assert!(broken_result.is_err());
        // Verify target file is not present after failed upload
        assert!(backend.stat(&broken_path).await.is_err());

        backend.delete(&root_path, true).await.expect("cleanup");
    }

    #[test]
    fn test_ftp_sync_planning_with_docker() {
        let port = 21250;
        let pasv_start = 21251;
        let pasv_end = 21260;
        let container_name = format!("abyss-ftp-sync-{}", Uuid::new_v4().simple());

        let _guard = match start_ftp_container(&container_name, port, pasv_start, pasv_end) {
            Some(guard) => guard,
            None => return,
        };

        // Create temporary config
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let config_path = temp_dir.path().join("connections.toml");
        ConnectionConfig {
            version: 1,
            connections: vec![NamedConnection {
                id: "ftp-sync-conn".to_owned(),
                name: "FTP Sync Connection".to_owned(),
                connection: Connection::Ftp(FtpConnection {
                    host: "127.0.0.1".to_owned(),
                    port: Some(port),
                    username: "testuser".to_owned(),
                    password_env: Some("ABYSS_TEST_DOCKER_FTP_PASS".to_owned()),
                    root: String::new(),
                    mode: FtpMode::Plain,
                }),
            }],
        }
        .save(&config_path)
        .expect("save config");

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_FTP_PASS", "testpass");
        }

        let runtime = StorageRuntime::load(&config_path).expect("load storage runtime");
        let sources = runtime.refresh_sources();
        let ftp_source = sources
            .iter()
            .find(|s| s.name == "FTP Sync Connection")
            .expect("find ftp source");

        let backend = runtime
            .backend(&match &ftp_source.location {
                Location::Remote(r) => r.clone(),
                _ => panic!("remote location expected"),
            })
            .expect("open backend");

        let ftp_root = match &ftp_source.location {
            Location::Remote(r) => r.path.child(b"sync_target").unwrap(),
            _ => panic!(),
        };
        runtime
            .block_on(backend.create_dir(&ftp_root))
            .expect("create ftp root");

        // Populate local directory with files
        let local_dir = temp_dir.path().join("local_sync");
        fs::create_dir(&local_dir).unwrap();
        fs::write(local_dir.join("a.txt"), b"file a").unwrap();
        fs::create_dir(local_dir.join("sub")).unwrap();
        fs::write(local_dir.join("sub").join("b.txt"), b"file b").unwrap();

        let local_loc = Location::Local(local_dir);
        let remote_loc = LocationCodec::parse("ftp://ftp-sync-conn/sync_target").unwrap();

        // Run sync planning from local to remote
        let plan = plan_locations(
            Arc::clone(&runtime),
            local_loc,
            remote_loc,
            SyncComparison::Metadata,
            SyncStrategy::Mirror,
        )
        .expect("plan sync");

        // The remote is empty, so it should report 2 missing files and 1 missing directory
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.directories.len(), 1);
        assert_eq!(plan.unchanged, 0);

        runtime
            .block_on(backend.delete(&ftp_root, true))
            .expect("cleanup");
    }
}
