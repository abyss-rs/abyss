use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::Error;
use crate::copy::{ConflictDecision, ConflictResolver};
use crate::progress::CopyStats;
use crate::remote_operation::transfer;
use crate::storage::{
    Connection, ConnectionConfig, KubernetesConnection, Location, LocationCodec, NamedConnection,
    StorageRuntime,
};

#[cfg(feature = "kubernetes")]
struct Overwrite;

#[cfg(feature = "kubernetes")]
impl ConflictResolver for Overwrite {
    fn resolve(&self, _destination: &Path) -> Result<ConflictDecision, Error> {
        Ok(ConflictDecision::Overwrite)
    }
}

#[cfg(feature = "kubernetes")]
#[test]
fn live_kubernetes_bulk_transfer_path() {
    let Ok(uri) = std::env::var("ABYSS_REMOTE_BULK_URI") else {
        return;
    };
    let Location::Remote(root) = LocationCodec::parse(&uri).expect("parse bulk test URI") else {
        panic!("ABYSS_REMOTE_BULK_URI must be remote");
    };
    assert_eq!(root.scheme, "kube");
    let namespace = root
        .path
        .components()
        .first()
        .and_then(|value| value.to_str())
        .expect("bulk test URI needs a namespace")
        .to_owned();
    let config_directory = tempfile::tempdir().expect("temporary config");
    let config_path = config_directory.path().join("connections.toml");
    ConnectionConfig {
        version: 1,
        connections: vec![NamedConnection {
            id: root.connection.clone(),
            name: "Kubernetes bulk transfer test".to_owned(),
            connection: Connection::Kubernetes(KubernetesConnection {
                kubeconfig: Vec::new(),
                context: root.connection.clone(),
                namespaces: vec![namespace],
                helper_image: std::env::var("ABYSS_KUBE_HELPER_IMAGE")
                    .unwrap_or_else(|_| "abyss-kube-helper:test".to_owned()),
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
    .save(&config_path)
    .expect("save bulk test config");
    let storage = StorageRuntime::load(&config_path).expect("load bulk test runtime");

    let (source, generated_source) = if let Some(source) =
        std::env::var_os("ABYSS_REMOTE_BULK_SOURCE")
    {
        let source = PathBuf::from(source)
            .canonicalize()
            .expect("canonicalize benchmark source");
        let alias_parent = tempfile::tempdir().expect("benchmark alias parent");
        let alias = alias_parent
            .path()
            .join(format!("abyss-real-bulk-{}", uuid::Uuid::new_v4().simple()));
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, &alias).expect("create benchmark source alias");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(source, &alias).expect("create benchmark source alias");
        (alias, Some(alias_parent))
    } else {
        let upload_parent = tempfile::tempdir().expect("upload parent");
        let source = upload_parent
            .path()
            .join(format!("abyss-bulk-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&source).expect("create upload root");
        for index in 0..1_000 {
            std::fs::write(
                source.join(format!("file-{index:04}.txt")),
                format!("small-file-{index:04}-").repeat(32),
            )
            .expect("write source file");
        }
        (source, Some(upload_parent))
    };
    let expected_files = count_files(&source);
    let cancelled = Arc::new(AtomicBool::new(false));
    let upload_stats = Arc::new(CopyStats::default());
    let started = std::time::Instant::now();
    transfer(
        Arc::clone(&storage),
        vec![Location::Local(source.clone())],
        Location::Remote(root.clone()),
        false,
        Arc::clone(&cancelled),
        Arc::clone(&upload_stats),
        &Overwrite,
    )
    .expect("bulk upload");
    let upload_elapsed = started.elapsed();
    let upload_snapshot = upload_stats.snapshot();
    eprintln!(
        "uploaded {} files and {} logical bytes ({} wire bytes) in {:.2?} ({:.1} MiB/s logical)",
        expected_files.0,
        expected_files.1,
        upload_snapshot.physical_done,
        upload_elapsed,
        expected_files.1 as f64 / upload_elapsed.as_secs_f64() / (1024.0 * 1024.0),
    );

    let uploaded = Location::Remote(root.clone())
        .child(source.file_name().expect("source name").as_encoded_bytes())
        .expect("uploaded path");
    if std::env::var_os("ABYSS_REMOTE_BULK_UPLOAD_ONLY").is_none() {
        let download_parent = tempfile::tempdir().expect("download parent");
        let started = std::time::Instant::now();
        transfer(
            Arc::clone(&storage),
            vec![uploaded.clone()],
            Location::Local(download_parent.path().to_owned()),
            false,
            cancelled,
            Arc::new(CopyStats::default()),
            &Overwrite,
        )
        .expect("bulk download");
        let downloaded = download_parent
            .path()
            .join(source.file_name().expect("source name"));
        assert_eq!(count_files(&downloaded), expected_files);
        eprintln!(
            "downloaded {} files and {} bytes in {:.2?}",
            expected_files.0,
            expected_files.1,
            started.elapsed()
        );
    }
    let Location::Remote(uploaded) = uploaded else {
        unreachable!()
    };
    storage
        .block_on(
            storage
                .backend(&uploaded)
                .expect("bulk backend")
                .delete(&uploaded.path, true),
        )
        .expect("remove uploaded test tree");
    storage.shutdown().expect("shutdown bulk test runtime");
    drop(generated_source);
}

#[cfg(feature = "kubernetes")]
fn count_files(root: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0_u64;
    let mut stack = vec![root.to_owned()];
    while let Some(directory) = stack.pop() {
        for item in std::fs::read_dir(directory).expect("read benchmark directory") {
            let item = item.expect("read benchmark entry");
            let metadata = item.metadata().expect("read benchmark metadata");
            if metadata.is_dir() {
                stack.push(item.path());
            } else if metadata.is_file() && !item.file_name().as_encoded_bytes().starts_with(b"._")
            {
                files += 1;
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    (files, bytes)
}
