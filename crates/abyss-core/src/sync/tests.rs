use std::fs;

use super::delta::{apply_delta, compute_delta, compute_signature};
use super::local::plan_local;
use super::plan::{SyncComparison, SyncReason, SyncStrategy};
use crate::storage::Location;

#[test]
fn metadata_and_checksum_modes_plan_only_changed_files() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir_all(destination.join("nested")).unwrap();
    fs::write(source.join("same"), b"same").unwrap();
    fs::write(destination.join("same"), b"same").unwrap();
    fs::write(source.join("nested/changed"), b"new!").unwrap();
    fs::write(destination.join("nested/changed"), b"old!").unwrap();
    fs::write(source.join("missing"), b"missing").unwrap();

    let checksum = plan_local(
        &source,
        &destination,
        SyncComparison::Checksum,
        SyncStrategy::Mirror,
    )
    .unwrap();
    assert_eq!(checksum.files.len(), 2);
    assert_eq!(checksum.unchanged, 1);
    assert!(checksum.files.iter().any(|file| {
        file.relative == "nested/changed" && file.reason == SyncReason::ChecksumChanged
    }));
    assert!(
        checksum
            .files
            .iter()
            .any(|file| { file.relative == "missing" && file.reason == SyncReason::Missing })
    );
}

#[test]
fn missing_directories_are_created_before_their_files() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(source.join("a/b")).unwrap();
    fs::write(source.join("a/b/file"), b"value").unwrap();

    let plan = plan_local(
        &source,
        &destination,
        SyncComparison::Metadata,
        SyncStrategy::Mirror,
    )
    .unwrap();
    assert_eq!(
        plan.directories,
        [
            Location::Local(destination.clone().join("a")),
            Location::Local(destination.join("a/b"))
        ]
    );
    assert_eq!(plan.files.len(), 1);
}

#[test]
fn mirror_strategy_detects_orphaned_destination_files_for_deletion() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("file1.txt"), b"hello").unwrap();
    fs::write(destination.join("file1.txt"), b"hello").unwrap();
    fs::write(destination.join("extra.txt"), b"should be pruned").unwrap();

    let plan = plan_local(
        &source,
        &destination,
        SyncComparison::Checksum,
        SyncStrategy::Mirror,
    )
    .unwrap();
    assert_eq!(plan.unchanged, 1);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].relative, "extra.txt");
    assert_eq!(plan.files[0].reason, SyncReason::Orphaned);
    assert_eq!(plan.deletions.len(), 1);
}

#[test]
fn update_only_strategy_ignores_extra_destination_files() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("file1.txt"), b"hello").unwrap();
    fs::write(destination.join("file1.txt"), b"hello").unwrap();
    fs::write(destination.join("extra.txt"), b"kept safe").unwrap();

    let plan = plan_local(
        &source,
        &destination,
        SyncComparison::Checksum,
        SyncStrategy::UpdateOnly,
    )
    .unwrap();
    assert_eq!(plan.unchanged, 1);
    assert_eq!(plan.files.len(), 0);
    assert_eq!(plan.deletions.len(), 0);
}

#[test]
fn delta_strategy_marks_modified_files_as_delta_patchable() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("file1.txt"), b"version 2 modified").unwrap();
    fs::write(destination.join("file1.txt"), b"version 1 base").unwrap();

    let plan = plan_local(
        &source,
        &destination,
        SyncComparison::DeltaSignature,
        SyncStrategy::DeltaRsync,
    )
    .unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].relative, "file1.txt");
    assert_eq!(plan.files[0].reason, SyncReason::DeltaPatchable);
}

#[test]
fn two_way_sync_strategy_syncs_changes_bidirectionally() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();

    // File created only on source -> should sync to destination
    fs::write(source.join("from_source.txt"), b"src").unwrap();
    // File created only on destination -> should sync to source
    fs::write(destination.join("from_dest.txt"), b"dst").unwrap();

    let plan = plan_local(
        &source,
        &destination,
        SyncComparison::Metadata,
        SyncStrategy::TwoWay,
    )
    .unwrap();
    assert_eq!(plan.files.len(), 2);
    let src_to_dst = plan
        .files
        .iter()
        .find(|f| f.relative == "from_source.txt")
        .unwrap();
    assert_eq!(
        src_to_dst.source,
        Location::Local(source.join("from_source.txt"))
    );
    assert_eq!(
        src_to_dst.destination,
        Location::Local(destination.join("from_source.txt"))
    );

    let dst_to_src = plan
        .files
        .iter()
        .find(|f| f.relative == "from_dest.txt")
        .unwrap();
    assert_eq!(
        dst_to_src.source,
        Location::Local(destination.join("from_dest.txt"))
    );
    assert_eq!(
        dst_to_src.destination,
        Location::Local(source.join("from_dest.txt"))
    );
}

#[test]
fn blake3_delta_diff_and_apply_round_trip() {
    let base = b"Hello, this is the original base content of a 100kb file with lots of repeated text 1234567890.";
    let target = b"Hello, this is the MODIFIED target content of a 100kb file with lots of repeated text 1234567890!";

    let signature = compute_signature(base, 16);
    let delta = compute_delta(&signature, target);
    assert!(!delta.is_empty());

    let reconstructed = apply_delta(base, &delta).expect("apply delta");
    assert_eq!(reconstructed.as_slice(), target);
}
