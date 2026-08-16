#[allow(unused_imports)]
use super::{DiscoveryEnvironment, discover_sources};
#[allow(unused_imports)]
use crate::storage::{Connection, ConnectionConfig, NamedConnection};
#[cfg(feature = "s3")]
use crate::storage::{S3Connection, S3Preset};
#[cfg(any(
    feature = "s3",
    feature = "kubernetes",
    feature = "azure",
    feature = "gcs"
))]
use crate::test_support::TempDir;

#[cfg(feature = "s3")]
fn empty_s3(profile: Option<&str>) -> Connection {
    Connection::S3(S3Connection {
        preset: S3Preset::Aws,
        endpoint: None,
        region: None,
        profile: profile.map(str::to_owned),
        account_id: None,
        force_path_style: None,
        buckets: Vec::new(),
        disable_payload_signing: false,
        disable_checksums: false,
        disable_multipart: false,
    })
}

#[cfg(feature = "s3")]
#[test]
fn profiles_endpoints_and_configured_precedence_are_discovered_without_secrets() {
    let temp = TempDir::new();
    let aws = temp.path().join("config");
    std::fs::write(
        &aws,
        "[default]\nregion=eu-west-1\naws_secret_access_key=never-show\n\
         [profile minio]\nendpoint_url=http://127.0.0.1:9000\naws_access_key_id=also-secret\n",
    )
    .unwrap();
    let environment = DiscoveryEnvironment::for_test(
        [("AWS_CONFIG_FILE", aws.display().to_string())],
        Some(temp.path().to_owned()),
    );
    let config = ConnectionConfig {
        version: 1,
        connections: vec![NamedConnection {
            id: "saved-minio".to_owned(),
            name: "Configured MinIO".to_owned(),
            connection: empty_s3(Some("minio")),
        }],
    };
    let sources = discover_sources(&config, &environment);
    assert_eq!(sources[0].id, "local");
    assert!(sources.iter().any(|source| source.id == "saved-minio"));
    assert!(
        !sources
            .iter()
            .any(|source| { source.context == "minio" && source.id != "saved-minio" })
    );
    let debug = format!("{sources:?}");
    assert!(!debug.contains("never-show"));
    assert!(!debug.contains("also-secret"));
}

#[cfg(feature = "s3")]
#[test]
fn discovered_ids_do_not_collide_with_persistent_ids() {
    let environment = DiscoveryEnvironment::for_test([("AWS_PROFILE", "team")], None);
    let config = ConnectionConfig {
        version: 1,
        connections: vec![NamedConnection {
            id: "discovered-aws-team".to_owned(),
            name: "Reserved".to_owned(),
            connection: empty_s3(Some("other")),
        }],
    };
    let sources = discover_sources(&config, &environment);
    let ids = sources
        .iter()
        .map(|source| &source.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), sources.len());
}

#[cfg(feature = "kubernetes")]
#[test]
fn merged_kubeconfigs_contribute_every_context_in_stable_order() {
    let temp = TempDir::new();
    let first = temp.path().join("one.yaml");
    let second = temp.path().join("two.yaml");
    std::fs::write(
        &first,
        "apiVersion: v1\nkind: Config\ncontexts:\n  - name: west\n    context:\n      cluster: west\n      user: west\n      namespace: media\n",
    )
    .unwrap();
    std::fs::write(
        &second,
        "apiVersion: v1\nkind: Config\ncontexts:\n  - name: east\n    context:\n      cluster: east\n      user: east\n",
    )
    .unwrap();
    let kubeconfig = std::env::join_paths([&first, &second])
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let environment = DiscoveryEnvironment::for_test([("KUBECONFIG", kubeconfig)], None);
    let first_result = discover_sources(&ConnectionConfig::default(), &environment);
    let second_result = discover_sources(&ConnectionConfig::default(), &environment);
    assert_eq!(first_result, second_result);
    assert!(first_result.iter().any(|source| {
        source.provider == "Kubernetes" && source.context == "east" && source.endpoint.is_empty()
    }));
    assert!(first_result.iter().any(|source| {
        source.provider == "Kubernetes"
            && source.context == "west"
            && source.endpoint == "media"
            && matches!(
                source
                    .connection
                    .as_ref()
                    .map(|connection| &connection.connection),
                Some(Connection::Kubernetes(connection))
                    if connection.namespaces.is_empty()
            )
    }));
}

#[cfg(all(feature = "azure", feature = "gcs"))]
#[test]
fn azure_and_gcs_require_metadata_and_inherited_credentials() {
    let temp = TempDir::new();
    let adc = temp.path().join("adc.json");
    std::fs::write(&adc, "{}").unwrap();
    let environment = DiscoveryEnvironment::for_test(
        vec![
            ("AZURE_STORAGE_ACCOUNT".to_owned(), "account".to_owned()),
            ("AZURE_TENANT_ID".to_owned(), "tenant".to_owned()),
            ("AZURE_CLIENT_ID".to_owned(), "client".to_owned()),
            ("AZURE_CLIENT_SECRET".to_owned(), "do-not-copy".to_owned()),
            ("GOOGLE_CLOUD_PROJECT".to_owned(), "project".to_owned()),
            (
                "GOOGLE_APPLICATION_CREDENTIALS".to_owned(),
                adc.display().to_string(),
            ),
        ],
        Some(temp.path().to_owned()),
    );
    let sources = discover_sources(&ConnectionConfig::default(), &environment);
    assert!(
        sources
            .iter()
            .any(|source| { source.provider == "Azure Blob" && source.context == "account" })
    );
    assert!(sources.iter().any(|source| {
        source.provider == "Google Cloud Storage" && source.context == "project"
    }));
    assert!(!format!("{sources:?}").contains("do-not-copy"));

    let metadata_only = DiscoveryEnvironment::for_test(
        [
            ("AZURE_STORAGE_ACCOUNT", "account"),
            ("GOOGLE_CLOUD_PROJECT", "project"),
        ],
        None,
    );
    let sources = discover_sources(&ConnectionConfig::default(), &metadata_only);
    assert!(!sources.iter().any(|source| {
        matches!(
            source.provider.as_str(),
            "Azure Blob" | "Google Cloud Storage"
        )
    }));
}

#[cfg(feature = "s3")]
#[test]
fn selected_profile_environment_endpoint_becomes_s3_compatible() {
    let environment = DiscoveryEnvironment::for_test(
        [
            ("AWS_PROFILE", "lab"),
            ("AWS_ENDPOINT_URL_S3", "http://minio.test:9000"),
            ("AWS_REGION", "us-east-1"),
        ],
        None,
    );
    let sources = discover_sources(&ConnectionConfig::default(), &environment);
    let source = sources
        .iter()
        .find(|source| source.context == "lab")
        .unwrap();
    assert_eq!(source.provider, "S3-compatible");
    assert_eq!(source.endpoint, "http://minio.test:9000");
}

#[cfg(feature = "gcs")]
#[test]
fn windows_appdata_adc_location_is_discovered() {
    let temp = TempDir::new();
    let gcloud = temp.path().join("gcloud");
    std::fs::create_dir(&gcloud).unwrap();
    std::fs::write(gcloud.join("application_default_credentials.json"), "{}").unwrap();
    let environment = DiscoveryEnvironment::for_test(
        [
            ("APPDATA", temp.path().to_string_lossy().as_ref()),
            ("GOOGLE_CLOUD_PROJECT", "windows-project"),
        ],
        None,
    );

    let sources = discover_sources(&ConnectionConfig::default(), &environment);

    assert!(sources.iter().any(|source| {
        source.provider == "Google Cloud Storage" && source.context == "windows-project"
    }));
}

#[cfg(all(feature = "sftp", feature = "ftp"))]
#[test]
fn configured_ftp_sftp_produce_correct_sources() {
    use crate::storage::{FtpConnection, FtpMode, Location, SftpConnection};

    let config = ConnectionConfig {
        version: 1,
        connections: vec![
            NamedConnection {
                id: "c-sftp".to_owned(),
                name: "SFTP Host".to_owned(),
                connection: Connection::Sftp(SftpConnection {
                    host: "ssh.example.com".to_owned(),
                    port: 2222,
                    username: "user2".to_owned(),
                    root: String::new(),
                    private_key: None,
                    password_env: None,
                    password_command: vec![],
                    known_hosts: None,
                    accept_new_host_keys: false,
                }),
            },
            NamedConnection {
                id: "c-ftp".to_owned(),
                name: "FTP Server".to_owned(),
                connection: Connection::Ftp(FtpConnection {
                    host: "ftp.example.com".to_owned(),
                    port: Some(21),
                    username: "user3".to_owned(),
                    password_env: None,
                    root: String::new(),
                    mode: FtpMode::Plain,
                }),
            },
        ],
    };
    let sources = discover_sources(&config, &DiscoveryEnvironment::default());

    let find = |id: &str| sources.iter().find(|s| s.id == id).expect(id);

    let sftp = find("c-sftp");
    assert_eq!(sftp.provider, "SFTP");
    let Location::Remote(loc) = &sftp.location else {
        panic!()
    };
    assert_eq!(loc.scheme, "sftp");

    let ftp = find("c-ftp");
    assert_eq!(ftp.provider, "FTP");
    let Location::Remote(loc) = &ftp.location else {
        panic!()
    };
    assert_eq!(loc.scheme, "ftp");
}
