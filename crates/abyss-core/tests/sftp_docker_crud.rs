mod sftp_common;

#[cfg(not(feature = "sftp"))]
#[test]
fn sftp_docker_crud_contract() {
    // SFTP feature not enabled in this build
}

#[cfg(feature = "sftp")]
mod sftp_docker_crud_tests {
    use std::sync::Arc;

    use abyss_core::storage::{
        Connection, EntryKind, ErrorKind, Location, LocationCodec, ReadOptions, SftpConnection,
        SftpFactory, StorageBackend, StorageProviderFactory, WriteOptions,
    };
    use bytes::Bytes;
    use uuid::Uuid;

    use super::sftp_common::{chunks, collect, one_chunk, start_sftp_container};

    #[tokio::test]
    async fn test_sftp_crud_lifecycle_with_docker() {
        let port = 22230;
        let container_name = format!("abyss-sftp-test-{}", Uuid::new_v4().simple());

        let _guard = match start_sftp_container(&container_name, port) {
            Some(guard) => guard,
            None => return,
        };

        let temp_dir = tempfile::tempdir().expect("create temp dir for known_hosts");
        let known_hosts_path = temp_dir.path().join("known_hosts");

        let factory = SftpFactory;
        let connection = Connection::Sftp(SftpConnection {
            host: "127.0.0.1".to_owned(),
            port,
            username: "testuser".to_owned(),
            root: "/home/testuser/upload".to_owned(),
            private_key: None,
            password_env: Some("ABYSS_TEST_DOCKER_SFTP_PASS".to_owned()),
            password_command: vec![],
            known_hosts: Some(known_hosts_path.clone()),
            accept_new_host_keys: true,
        });

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_SFTP_PASS", "testpass");
        }

        let backend: Arc<dyn StorageBackend> = factory
            .create("docker-sftp".to_owned(), connection)
            .await
            .expect("create SFTP backend");

        let test_root_name = format!("test-root-{}", Uuid::new_v4().simple());
        let root = LocationCodec::parse(&format!(
            "sftp://testuser@127.0.0.1:{port}/{test_root_name}"
        ))
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

        // 3. Write file
        let file_path = root_path.child(b"hello.txt").expect("child path");
        let initial_payload = Bytes::from_static(b"Hello SFTP Storage World via russh!");
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
                .expect("read hello.txt"),
        )
        .await
        .expect("collect bytes");
        assert_eq!(read_back, initial_payload);

        // 6. Range read (offset & length)
        let range_read = collect(
            backend
                .read(
                    &file_path,
                    ReadOptions {
                        offset: Some(6),
                        length: Some(4),
                        expected_version: None,
                    },
                )
                .await
                .expect("range read"),
        )
        .await
        .expect("collect range");
        assert_eq!(range_read, Bytes::from_static(b"SFTP"));

        // 7. Write overwrite=false when file exists -> fails with AlreadyExists
        let err = backend
            .write(
                &file_path,
                one_chunk(Bytes::from_static(b"Should fail")),
                WriteOptions {
                    size: None,
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect_err("overwrite false on existing file must fail");
        assert_eq!(err.kind, ErrorKind::AlreadyExists);

        // 8. Write with stale expected_version -> fails with Conflict
        let err = backend
            .write(
                &file_path,
                one_chunk(Bytes::from_static(b"Should conflict")),
                WriteOptions {
                    size: None,
                    overwrite: true,
                    expected_version: Some("stale-version-9999".to_owned()),
                },
            )
            .await
            .expect_err("stale expected_version must fail");
        assert_eq!(err.kind, ErrorKind::Conflict);

        // 9. Overwrite with valid expected_version
        let updated_payload = Bytes::from_static(b"Updated SFTP Payload with new data");
        backend
            .write(
                &file_path,
                one_chunk(updated_payload.clone()),
                WriteOptions {
                    size: Some(updated_payload.len() as u64),
                    overwrite: true,
                    expected_version: Some(version_1),
                },
            )
            .await
            .expect("overwrite with expected_version");

        let stat_updated = backend.stat(&file_path).await.expect("stat updated");
        assert_eq!(stat_updated.size, Some(updated_payload.len() as u64));
        let read_updated = collect(
            backend
                .read(&file_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(read_updated, updated_payload);

        // 10. Chunked write of larger binary payload
        let large_path = root_path.child(b"large.bin").expect("large path");
        let large_payload = Bytes::from(vec![0x42_u8; 1024 * 1024 + 123]); // ~1 MB
        backend
            .write(
                &large_path,
                chunks(large_payload.clone(), 64 * 1024),
                WriteOptions {
                    size: Some(large_payload.len() as u64),
                    overwrite: false,
                    expected_version: None,
                },
            )
            .await
            .expect("write large.bin");

        let stat_large = backend.stat(&large_path).await.expect("stat large.bin");
        assert_eq!(stat_large.size, Some(large_payload.len() as u64));

        let read_large = collect(
            backend
                .read(&large_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(read_large, large_payload);

        // 11. Nested directory creation (mkdir -p)
        let sub_dir = root_path
            .child(b"nested")
            .unwrap()
            .child(b"deep")
            .unwrap()
            .child(b"folder")
            .unwrap();
        backend
            .create_dir(&sub_dir)
            .await
            .expect("create nested deep folder");
        let sub_stat = backend.stat(&sub_dir).await.expect("stat sub dir");
        assert_eq!(sub_stat.kind, EntryKind::Directory);

        let nested_file = sub_dir.child(b"doc.txt").unwrap();
        backend
            .write(
                &nested_file,
                one_chunk(Bytes::from_static(b"Nested Document Content")),
                WriteOptions::default(),
            )
            .await
            .expect("write nested doc.txt");

        // 12. List operations
        let list_root = backend.list(&root_path, None).await.expect("list root");
        let entry_names: Vec<String> = list_root
            .entries
            .iter()
            .map(|e| String::from_utf8_lossy(&e.name).into_owned())
            .collect();
        assert!(entry_names.contains(&"hello.txt".to_string()));
        assert!(entry_names.contains(&"large.bin".to_string()));
        assert!(entry_names.contains(&"nested".to_string()));

        // 13. Rename file
        let renamed_path = root_path.child(b"hello-renamed.txt").unwrap();
        backend
            .rename(&file_path, &renamed_path, false)
            .await
            .expect("rename hello.txt -> hello-renamed.txt");
        assert!(backend.stat(&file_path).await.is_err());
        let stat_renamed = backend.stat(&renamed_path).await.expect("stat renamed");
        assert_eq!(stat_renamed.size, Some(updated_payload.len() as u64));

        // 14. Rename with overwrite over existing file
        let overwrite_target = root_path.child(b"overwrite-target.txt").unwrap();
        backend
            .write(
                &overwrite_target,
                one_chunk(Bytes::from_static(b"Old Target Data")),
                WriteOptions::default(),
            )
            .await
            .unwrap();
        backend
            .rename(&renamed_path, &overwrite_target, true)
            .await
            .expect("rename with overwrite=true");
        let read_overwritten = collect(
            backend
                .read(&overwrite_target, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(read_overwritten, updated_payload);

        // 15. Delete single file
        backend
            .delete(&overwrite_target, false)
            .await
            .expect("delete file");
        assert!(backend.stat(&overwrite_target).await.is_err());

        // 16. Recursive delete of entire nested subtree
        let nested_root = root_path.child(b"nested").unwrap();
        backend
            .delete(&nested_root, true)
            .await
            .expect("recursive delete of nested dir");
        assert!(backend.stat(&nested_root).await.is_err());
        assert!(backend.stat(&nested_file).await.is_err());

        // 17. Clean up root directory
        backend
            .delete(&large_path, false)
            .await
            .expect("delete large.bin");
        backend
            .delete(&root_path, true)
            .await
            .expect("delete root dir");
        assert!(backend.stat(&root_path).await.is_err());
    }

    #[tokio::test]
    async fn test_sftp_zero_byte_file_and_known_hosts() {
        let port = 22231;
        let container_name = format!("abyss-sftp-zero-{}", Uuid::new_v4().simple());

        let _guard = match start_sftp_container(&container_name, port) {
            Some(guard) => guard,
            None => return,
        };

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let known_hosts_path = temp_dir.path().join("known_hosts");

        // 1. Connect with accept_new_host_keys=true -> records host key
        let factory = SftpFactory;
        let connection = Connection::Sftp(SftpConnection {
            host: "127.0.0.1".to_owned(),
            port,
            username: "testuser".to_owned(),
            root: "/home/testuser/upload".to_owned(),
            private_key: None,
            password_env: Some("ABYSS_TEST_DOCKER_SFTP_PASS".to_owned()),
            password_command: vec![],
            known_hosts: Some(known_hosts_path.clone()),
            accept_new_host_keys: true,
        });

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_SFTP_PASS", "testpass");
        }

        let backend: Arc<dyn StorageBackend> = factory
            .create("docker-sftp-1".to_owned(), connection)
            .await
            .expect("create SFTP backend");

        let test_root_name = format!("test-zero-{}", Uuid::new_v4().simple());
        let root = LocationCodec::parse(&format!(
            "sftp://testuser@127.0.0.1:{port}/{test_root_name}"
        ))
        .expect("parse root location");
        let Location::Remote(root_location) = root else {
            panic!("expected remote location");
        };
        let root_path = root_location.path;

        backend.create_dir(&root_path).await.expect("create dir");

        // Write zero-byte file
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
            .expect("write zero-byte file");

        let stat_zero = backend.stat(&zero_path).await.expect("stat zero-byte file");
        assert_eq!(stat_zero.size, Some(0));
        assert_eq!(stat_zero.kind, EntryKind::File);

        let read_zero = collect(
            backend
                .read(&zero_path, ReadOptions::default())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert!(read_zero.is_empty());

        backend.delete(&root_path, true).await.expect("cleanup");

        // 2. Connect with accept_new_host_keys=false using the saved known_hosts -> succeeds
        let connection2 = Connection::Sftp(SftpConnection {
            host: "127.0.0.1".to_owned(),
            port,
            username: "testuser".to_owned(),
            root: "/home/testuser/upload".to_owned(),
            private_key: None,
            password_env: Some("ABYSS_TEST_DOCKER_SFTP_PASS".to_owned()),
            password_command: vec![],
            known_hosts: Some(known_hosts_path),
            accept_new_host_keys: false,
        });

        let backend2: Arc<dyn StorageBackend> = factory
            .create("docker-sftp-2".to_owned(), connection2)
            .await
            .expect("create SFTP backend with verified known_hosts");

        let list_res = backend2
            .list(&root_path.parent().unwrap_or(root_path), None)
            .await;
        assert!(list_res.is_ok(), "known_hosts verification should succeed");
    }

    #[tokio::test]
    async fn test_sftp_authentication_failure_handling() {
        let port = 22232;
        let container_name = format!("abyss-sftp-auth-{}", Uuid::new_v4().simple());

        let _guard = match start_sftp_container(&container_name, port) {
            Some(guard) => guard,
            None => return,
        };

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let known_hosts_path = temp_dir.path().join("known_hosts");

        let factory = SftpFactory;
        let connection = Connection::Sftp(SftpConnection {
            host: "127.0.0.1".to_owned(),
            port,
            username: "testuser".to_owned(),
            root: "/home/testuser/upload".to_owned(),
            private_key: None,
            password_env: Some("ABYSS_TEST_DOCKER_SFTP_WRONG_PASS".to_owned()),
            password_command: vec![],
            known_hosts: Some(known_hosts_path),
            accept_new_host_keys: true,
        });

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_SFTP_WRONG_PASS", "invalid_password_xyz");
        }

        let backend: Arc<dyn StorageBackend> = factory
            .create("docker-sftp-fail".to_owned(), connection)
            .await
            .expect("factory create succeeds");

        let dummy_loc =
            LocationCodec::parse(&format!("sftp://testuser@127.0.0.1:{port}/any_file.txt"))
                .unwrap();
        let Location::Remote(rem) = dummy_loc else {
            panic!()
        };

        let stat_res = backend.stat(&rem.path).await;
        assert!(stat_res.is_err());
        let err = stat_res.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Authentication);
    }
}
