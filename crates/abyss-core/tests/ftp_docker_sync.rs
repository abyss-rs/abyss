mod ftp_common;

#[cfg(not(feature = "ftp"))]
#[test]
fn ftp_docker_sync_contract() {
    // FTP feature not enabled in this build
}

#[cfg(feature = "ftp")]
mod ftp_docker_sync_tests {
    use std::fs;
    use std::sync::Arc;

    use abyss_core::storage::{
        Connection, ConnectionConfig, FtpConnection, FtpMode, Location, LocationCodec,
        NamedConnection, StorageRuntime,
    };
    use abyss_core::sync::{SyncComparison, SyncStrategy, plan_locations};
    use uuid::Uuid;

    use super::ftp_common::start_ftp_container;

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

        let local_dir = temp_dir.path().join("local_sync");
        fs::create_dir(&local_dir).unwrap();
        fs::write(local_dir.join("a.txt"), b"file a").unwrap();
        fs::create_dir(local_dir.join("sub")).unwrap();
        fs::write(local_dir.join("sub").join("b.txt"), b"file b").unwrap();

        let local_loc = Location::Local(local_dir);
        let remote_loc = LocationCodec::parse("ftp://ftp-sync-conn/sync_target").unwrap();

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
        assert_eq!(plan.unchanged, 0);

        runtime
            .block_on(backend.delete(&ftp_root, true))
            .expect("cleanup");
    }
}
