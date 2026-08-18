mod sftp_common;

#[cfg(not(feature = "sftp"))]
#[test]
fn sftp_docker_sync_contract() {
    // SFTP feature not enabled in this build
}

#[cfg(feature = "sftp")]
mod sftp_docker_sync_tests {
    use std::fs;
    use std::sync::Arc;

    use abyss_core::storage::{
        Connection, ConnectionConfig, Location, LocationCodec, NamedConnection, SftpConnection,
        StorageRuntime,
    };
    use abyss_core::sync::{SyncComparison, SyncStrategy, plan_locations};
    use uuid::Uuid;

    use super::sftp_common::start_sftp_container;

    #[test]
    fn test_sftp_sync_planning_with_docker() {
        let port = 22233;
        let container_name = format!("abyss-sftp-sync-{}", Uuid::new_v4().simple());

        let _guard = match start_sftp_container(&container_name, port) {
            Some(guard) => guard,
            None => return,
        };

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let known_hosts_path = temp_dir.path().join("known_hosts");
        let config_path = temp_dir.path().join("connections.toml");
        ConnectionConfig {
            version: 1,
            connections: vec![NamedConnection {
                id: "sftp-sync-conn".to_owned(),
                name: "SFTP Sync Connection".to_owned(),
                connection: Connection::Sftp(SftpConnection {
                    host: "127.0.0.1".to_owned(),
                    port,
                    username: "testuser".to_owned(),
                    root: "/home/testuser/upload".to_owned(),
                    private_key: None,
                    password_env: Some("ABYSS_TEST_DOCKER_SFTP_PASS".to_owned()),
                    password_command: vec![],
                    known_hosts: Some(known_hosts_path),
                    accept_new_host_keys: true,
                }),
            }],
        }
        .save(&config_path)
        .expect("save config");

        unsafe {
            std::env::set_var("ABYSS_TEST_DOCKER_SFTP_PASS", "testpass");
        }

        let runtime = StorageRuntime::load(&config_path).expect("load storage runtime");
        let sources = runtime.refresh_sources();
        let sftp_source = sources
            .iter()
            .find(|s| s.name == "SFTP Sync Connection")
            .expect("find sftp source");

        let backend = runtime
            .backend(&match &sftp_source.location {
                Location::Remote(r) => r.clone(),
                _ => panic!("remote location expected"),
            })
            .expect("open backend");

        let sftp_root = match &sftp_source.location {
            Location::Remote(r) => r.path.child(b"sync_target").unwrap(),
            _ => panic!(),
        };
        runtime
            .block_on(backend.create_dir(&sftp_root))
            .expect("create sftp root");

        let local_dir = temp_dir.path().join("local_sync");
        fs::create_dir(&local_dir).unwrap();
        fs::write(local_dir.join("a.txt"), b"file a").unwrap();
        fs::create_dir(local_dir.join("sub")).unwrap();
        fs::write(local_dir.join("sub").join("b.txt"), b"file b").unwrap();

        let local_loc = Location::Local(local_dir);
        let remote_loc = LocationCodec::parse("sftp://sftp-sync-conn/sync_target").unwrap();

        let plan = plan_locations(
            Arc::clone(&runtime),
            local_loc,
            remote_loc,
            SyncComparison::Metadata,
            SyncStrategy::Mirror,
        )
        .expect("plan sync");

        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.directories.len(), 1);
    }
}
