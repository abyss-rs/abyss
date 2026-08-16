use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Pod};
use kube::core::{ApiResource, GroupVersionKind};

use super::compression::{
    LZ4_BLOCK, decode_brotli_stream, decode_deflate_stream, decode_lz4_stream,
    encode_brotli_stream, encode_deflate_stream, encode_lz4_stream,
};
use super::descriptor::NO_PVC_MESSAGE;
use super::ops::{
    VolumeUsage, claim_namespaces, format_usage, namespace_page, volume_snapshot_object,
};
use super::session::{
    HelperSession, migration_worker_limit, pod_image_startup_failure, pod_startup_failure,
    select_helper_session,
};
use crate::storage::{ByteStream, ErrorKind, WireProgress};

#[test]
fn namespace_listing_contains_only_namespaces_with_claims() {
    let claims = ["media", "backups", "media"]
        .into_iter()
        .map(|namespace| {
            serde_json::from_value::<PersistentVolumeClaim>(serde_json::json!({
                "apiVersion": "v1",
                "kind": "PersistentVolumeClaim",
                "metadata": {
                    "name": format!("{namespace}-claim"),
                    "namespace": namespace
                },
                "spec": {
                    "accessModes": ["ReadWriteOnce"],
                    "resources": {"requests": {"storage": "1Gi"}}
                }
            }))
            .expect("deserialize claim")
        })
        .collect::<Vec<_>>();
    let page = namespace_page(claim_namespaces(claims)).expect("build namespace page");
    let names = page
        .entries
        .into_iter()
        .map(|entry| String::from_utf8(entry.name).expect("namespace is UTF-8"))
        .collect::<Vec<_>>();
    assert_eq!(names, ["backups", "media"]);
}

#[test]
fn empty_claim_list_reports_the_cluster_message() {
    let error = namespace_page(BTreeSet::new()).expect_err("empty cluster must fail");
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(error.message, NO_PVC_MESSAGE);
    assert_eq!(error.to_string(), "no pvc found in this cluster");
}

#[test]
fn usage_status_includes_capacity_and_inode_percentages() {
    let status = format_usage(&VolumeUsage {
        capacity_bytes: 1_000,
        free_bytes: 100,
        total_inodes: 200,
        free_inodes: 50,
    });
    assert_eq!(status, "abyss-usage|90|900|1000|75|150|200");
}

#[test]
fn volume_snapshot_manifest_targets_the_selected_claim() {
    let resource = ApiResource::from_gvk(&GroupVersionKind::gvk(
        "snapshot.storage.k8s.io",
        "v1",
        "VolumeSnapshot",
    ));
    let snapshot = volume_snapshot_object("media-snapshot", "media-pvc", &resource);
    assert_eq!(
        snapshot
            .data
            .pointer("/spec/source/persistentVolumeClaimName")
            .and_then(serde_json::Value::as_str),
        Some("media-pvc")
    );
    assert_eq!(snapshot.metadata.name.as_deref(), Some("media-snapshot"));
}

#[test]
fn reports_image_pull_failure_without_waiting_for_timeout() {
    let pod: Pod = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "helper"},
        "status": {
            "phase": "Pending",
            "containerStatuses": [{
                "name": "helper",
                "image": "missing",
                "imageID": "",
                "ready": false,
                "restartCount": 0,
                "started": false,
                "state": {
                    "waiting": {
                        "reason": "ImagePullBackOff",
                        "message": "pull access denied"
                    }
                }
            }]
        }
    }))
    .expect("deserialize pod");
    let message = pod_image_startup_failure(&pod).expect("detect image pull failure");
    assert!(message.contains("ImagePullBackOff"));
    assert!(message.contains("pull access denied"));
}

#[test]
fn missing_node_local_image_is_a_retryable_candidate_failure() {
    let pod: Pod = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "helper"},
        "status": {
            "phase": "Pending",
            "containerStatuses": [{
                "name": "helper",
                "image": "abyss-kube-helper:test",
                "imageID": "",
                "ready": false,
                "restartCount": 0,
                "started": false,
                "state": {
                    "waiting": {
                        "reason": "ErrImageNeverPull",
                        "message": "image is not present with pull policy of Never"
                    }
                }
            }]
        }
    }))
    .expect("deserialize pod");
    let message = pod_image_startup_failure(&pod).expect("detect missing local image");
    assert!(message.contains("ErrImageNeverPull"));
}

#[test]
fn reports_unschedulable_pvc_pod_without_waiting_for_timeout() {
    let pod: Pod = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "helper"},
        "status": {
            "phase": "Pending",
            "conditions": [{
                "type": "PodScheduled",
                "status": "False",
                "reason": "Unschedulable",
                "message": "persistentvolumeclaim is not bound"
            }]
        }
    }))
    .expect("deserialize pod");
    let message = pod_startup_failure(&pod).expect("detect scheduling failure");
    assert!(message.contains("cannot be scheduled"));
    assert!(message.contains("persistentvolumeclaim is not bound"));
}

#[test]
fn migration_worker_limit_respects_access_mode_and_safety_cap() {
    assert_eq!(migration_worker_limit(4, &["ReadWriteMany".to_owned()]), 4);
    assert_eq!(migration_worker_limit(99, &["ReadWriteMany".to_owned()]), 8);
    assert_eq!(migration_worker_limit(0, &["ReadWriteOnce".to_owned()]), 1);
    assert_eq!(
        migration_worker_limit(4, &["ReadWriteOncePod".to_owned()]),
        1
    );
}

#[test]
fn helper_pool_selection_is_stable_round_robin() {
    let pool = ["one", "two", "three"]
        .into_iter()
        .map(|pod| HelperSession {
            namespace: "default".to_owned(),
            pod: pod.to_owned(),
            created: Instant::now(),
        })
        .collect::<Vec<_>>();
    let next = AtomicU64::new(0);
    assert_eq!(select_helper_session(&pool, &next).pod, "one");
    assert_eq!(select_helper_session(&pool, &next).pod, "two");
    assert_eq!(select_helper_session(&pool, &next).pod, "three");
    assert_eq!(select_helper_session(&pool, &next).pod, "one");
}

#[tokio::test]
async fn async_lz4_transport_round_trips_chunk_boundaries() {
    let expected = bytes::Bytes::from(
        (0..(LZ4_BLOCK * 3 + 117))
            .map(|index| ((index * 31 + index / 11) % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let source: ByteStream = Box::pin(futures_util::stream::iter(vec![
        Ok(expected.slice(..17)),
        Ok(expected.slice(17..LZ4_BLOCK + 23)),
        Ok(expected.slice(LZ4_BLOCK + 23..)),
    ]));
    let mut decoded = decode_lz4_stream(encode_lz4_stream(source, None), None);
    let mut actual = Vec::new();
    while let Some(chunk) = decoded.next().await {
        actual.extend_from_slice(&chunk.expect("decode chunk"));
    }
    assert_eq!(actual, expected.as_ref());
}

#[tokio::test]
async fn async_lz4_transport_rejects_truncated_blocks() {
    let source: ByteStream = Box::pin(futures_util::stream::once(async {
        Ok(bytes::Bytes::from_static(&[0, 0, 0, 8, 0, 0, 0, 4, 1, 2]))
    }));
    let mut decoded = decode_lz4_stream(source, None);
    assert!(decoded.next().await.expect("one result").is_err());
}

#[tokio::test]
async fn async_lz4_transport_aggregates_tiny_files() {
    let logical = b"same-small-file-body\n".repeat(10_000);
    let chunks = logical
        .chunks(21)
        .map(|chunk| Ok(bytes::Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    let wire = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let observed = Arc::clone(&wire);
    let progress: WireProgress = Arc::new(move |bytes| {
        observed.fetch_add(bytes, Ordering::Relaxed);
    });
    let source: ByteStream = Box::pin(futures_util::stream::iter(chunks));
    let mut encoded = encode_lz4_stream(source, Some(progress));
    while encoded.next().await.transpose().expect("encode").is_some() {}
    assert!(
        wire.load(Ordering::Relaxed) < logical.len() as u64 / 10,
        "tiny chunks were not aggregated into an effective compression window"
    );
}

#[tokio::test]
async fn async_pure_rust_codecs_round_trip() {
    let expected = bytes::Bytes::from(b"provider-binary-section\0".repeat(32_768));
    for codec in ["brotli", "deflate"] {
        let source: ByteStream = Box::pin(futures_util::stream::once({
            let expected = expected.clone();
            async move { Ok(expected) }
        }));
        let mut decoded = match codec {
            "brotli" => decode_brotli_stream(encode_brotli_stream(source, None), None),
            "deflate" => decode_deflate_stream(encode_deflate_stream(source, None), None),
            _ => unreachable!(),
        };
        let mut actual = Vec::new();
        while let Some(chunk) = decoded.next().await {
            actual.extend_from_slice(&chunk.expect("decode chunk"));
        }
        assert_eq!(actual, expected.as_ref(), "{codec} round trip");
    }
}
