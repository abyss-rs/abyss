#[cfg(any(feature = "s3", feature = "kubernetes", feature = "remote"))]
use super::{ConnectionConfig, NamedConnection};

#[cfg(feature = "s3")]
#[test]
fn configuration_round_trips_without_secrets() {
    use crate::storage::{Connection, S3Connection, S3Preset};
    use crate::test_support::TempDir;

    let temp = TempDir::new();
    let path = temp.path().join("connections.toml");
    let expected = ConnectionConfig {
        version: 1,
        connections: vec![NamedConnection {
            id: "r2".to_owned(),
            name: "Cloudflare".to_owned(),
            connection: Connection::S3(S3Connection {
                preset: S3Preset::CloudflareR2,
                endpoint: None,
                region: None,
                profile: Some("r2".to_owned()),
                account_id: Some("account".to_owned()),
                force_path_style: None,
                buckets: vec!["media".to_owned()],
                disable_payload_signing: false,
                disable_checksums: false,
                disable_multipart: false,
            }),
        }],
    };
    expected.save(&path).unwrap();
    assert_eq!(ConnectionConfig::load(&path).unwrap(), expected);
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains("secret"));
}

#[cfg(feature = "kubernetes")]
#[test]
fn kubernetes_helper_images_are_ordered_and_backward_compatible() {
    use crate::storage::{KubernetesConnection, KubernetesHelperImage, KubernetesImagePullPolicy};

    let mut connection = KubernetesConnection {
        kubeconfig: Vec::new(),
        context: "docker-desktop".to_owned(),
        namespaces: Vec::new(),
        helper_image: String::new(),
        helper_images: Vec::new(),
        image_pull_secrets: vec!["private-registry".to_owned()],
        service_account: None,
        run_as_user: None,
        run_as_group: None,
        fs_group: None,
        migration_workers: 4,
    };
    let defaults = connection.resolved_helper_images();
    assert_eq!(defaults.len(), 3);
    assert_eq!(defaults[0].pull_policy, KubernetesImagePullPolicy::Never);
    assert_eq!(defaults[1].image, "abyss-kube-helper:test");
    assert!(
        defaults[2]
            .image
            .starts_with("ghcr.io/vyrti/abyss-kube-helper:")
    );

    connection.helper_image = "legacy.example/helper:v1".to_owned();
    let legacy = connection.resolved_helper_images();
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].image, "legacy.example/helper:v1");
    assert_eq!(
        legacy[0].pull_policy,
        KubernetesImagePullPolicy::IfNotPresent
    );

    connection.helper_images = vec![
        KubernetesHelperImage {
            image: "local/helper:test".to_owned(),
            pull_policy: KubernetesImagePullPolicy::Never,
        },
        KubernetesHelperImage {
            image: "registry.example/helper:v1".to_owned(),
            pull_policy: KubernetesImagePullPolicy::Always,
        },
    ];
    assert_eq!(
        connection.resolved_helper_images(),
        connection.helper_images
    );
}

#[cfg(feature = "kubernetes")]
#[test]
fn kubernetes_helper_image_candidates_deserialize_from_toml() {
    use crate::storage::{Connection, KubernetesImagePullPolicy};

    let config: ConnectionConfig = toml::from_str(
        r#"
version = 1

[[connections]]
id = "local-cluster"
name = "Local cluster"
provider = "kubernetes"
context = "docker-desktop"
image_pull_secrets = ["private-registry"]
helper_images = [
  { image = "abyss-kube-helper:test", pull_policy = "never" },
  { image = "registry.example/abyss-kube-helper:v1", pull_policy = "always" },
]
"#,
    )
    .expect("deserialize helper image candidates");
    let Connection::Kubernetes(connection) = &config.connections[0].connection else {
        panic!("expected Kubernetes connection");
    };
    assert_eq!(connection.image_pull_secrets, ["private-registry"]);
    assert_eq!(connection.resolved_migration_workers(), 4);
    assert_eq!(connection.helper_images.len(), 2);
    assert_eq!(
        connection.helper_images[0].pull_policy,
        KubernetesImagePullPolicy::Never
    );
    assert_eq!(
        connection.helper_images[1].pull_policy,
        KubernetesImagePullPolicy::Always
    );
}

#[cfg(feature = "remote")]
#[test]
fn all_connection_types_round_trip_through_toml() {
    use crate::storage::{
        AzureConnection, AzureCredentialSource, AzureMode, Connection, FtpConnection, FtpMode,
        GcsConnection, KubernetesConnection, S3Connection, S3Preset, SftpConnection,
    };

    let config = ConnectionConfig {
        version: 1,
        connections: vec![
            NamedConnection {
                id: "s3-test".to_owned(),
                name: "S3".to_owned(),
                connection: Connection::S3(S3Connection {
                    preset: S3Preset::Aws,
                    endpoint: None,
                    region: Some("us-east-1".to_owned()),
                    profile: Some("default".to_owned()),
                    account_id: None,
                    force_path_style: None,
                    buckets: vec!["b1".to_owned()],
                    disable_payload_signing: false,
                    disable_checksums: false,
                    disable_multipart: false,
                }),
            },
            NamedConnection {
                id: "az-test".to_owned(),
                name: "Azure".to_owned(),
                connection: Connection::Azure(AzureConnection {
                    mode: AzureMode::Blob,
                    account: "myacc".to_owned(),
                    endpoint: None,
                    credential: AzureCredentialSource::DeveloperTools,
                }),
            },
            NamedConnection {
                id: "gcs-test".to_owned(),
                name: "GCS".to_owned(),
                connection: Connection::Gcs(GcsConnection {
                    project: "myproj".to_owned(),
                    endpoint: None,
                    credential_path: None,
                }),
            },
            NamedConnection {
                id: "kube-test".to_owned(),
                name: "Kube".to_owned(),
                connection: Connection::Kubernetes(KubernetesConnection {
                    kubeconfig: vec![],
                    context: "ctx".to_owned(),
                    namespaces: vec!["ns".to_owned()],
                    helper_image: String::new(),
                    helper_images: vec![],
                    image_pull_secrets: vec![],
                    service_account: None,
                    run_as_user: None,
                    run_as_group: None,
                    fs_group: None,
                    migration_workers: 4,
                }),
            },
            NamedConnection {
                id: "sftp-test".to_owned(),
                name: "SFTP".to_owned(),
                connection: Connection::Sftp(SftpConnection {
                    host: "127.0.0.1".to_owned(),
                    port: 2222,
                    username: "user".to_owned(),
                    root: String::new(),
                    private_key: None,
                    password_env: Some("PASS".to_owned()),
                    password_command: vec![],
                    known_hosts: None,
                    accept_new_host_keys: true,
                }),
            },
            NamedConnection {
                id: "ftp-test".to_owned(),
                name: "FTP".to_owned(),
                connection: Connection::Ftp(FtpConnection {
                    host: "127.0.0.1".to_owned(),
                    port: Some(2121),
                    username: "user".to_owned(),
                    password_env: Some("PASS".to_owned()),
                    root: String::new(),
                    mode: FtpMode::Plain,
                }),
            },
        ],
    };
    let serialized = toml::to_string_pretty(&config).expect("serialize connection config");
    let deserialized: ConnectionConfig =
        toml::from_str(&serialized).expect("deserialize connection config");
    assert_eq!(config, deserialized);
}
