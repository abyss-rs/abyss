use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use super::JobManager;
use super::manager::{accesses_conflict, paths_overlap};
use super::types::{
    AccessMode, JobRequest, JobState, JobUpdate, LaunchMode, PathAccess, WaitReason,
};
use crate::operation::{ConflictChoice, OperationKind};
use crate::storage::Location;
use crate::test_support::TempDir;

fn delete(path: PathBuf) -> JobRequest {
    JobRequest::Delete {
        sources: vec![path],
    }
}

#[test]
fn overlap_is_component_aware() {
    assert!(paths_overlap(
        &Location::Local(PathBuf::from("/a/folder")),
        &Location::Local(PathBuf::from("/a/folder/child"))
    ));
    assert!(!paths_overlap(
        &Location::Local(PathBuf::from("/a/folder")),
        &Location::Local(PathBuf::from("/a/folder-two"))
    ));
    let remote_parent = crate::storage::LocationCodec::parse("s3://archive/shows").unwrap();
    let remote_child = crate::storage::LocationCodec::parse("s3://archive/shows/season-1").unwrap();
    let other_connection =
        crate::storage::LocationCodec::parse("s3://backup/shows/season-1").unwrap();
    assert!(paths_overlap(&remote_parent, &remote_child));
    assert!(!paths_overlap(&remote_parent, &other_connection));
}

#[test]
fn concurrent_reads_are_safe_but_writes_are_not() {
    let read = [PathAccess {
        path: Location::Local("/source".into()),
        mode: AccessMode::Read,
    }];
    let other_read = [PathAccess {
        path: Location::Local("/source/file".into()),
        mode: AccessMode::Read,
    }];
    let write = [PathAccess {
        path: Location::Local("/source/file".into()),
        mode: AccessMode::Write,
    }];
    assert!(!accesses_conflict(&read, &other_read));
    assert!(accesses_conflict(&read, &write));
}

#[test]
fn starts_only_three_jobs_and_queues_the_fourth() {
    let temp = TempDir::new();
    let mut jobs = JobManager::new();
    let first = jobs.submit(
        delete(temp.path().join("one")),
        LaunchMode::Background,
        0,
        None,
    );
    let second = jobs.submit(
        delete(temp.path().join("two")),
        LaunchMode::Background,
        0,
        None,
    );
    let third = jobs.submit(
        delete(temp.path().join("three")),
        LaunchMode::Background,
        0,
        None,
    );
    let fourth = jobs.submit(
        delete(temp.path().join("four")),
        LaunchMode::Background,
        0,
        None,
    );

    assert_eq!(jobs.running_count(), 3);
    assert!(matches!(jobs.job(first).unwrap().state, JobState::Running));
    assert!(matches!(jobs.job(second).unwrap().state, JobState::Running));
    assert!(matches!(jobs.job(third).unwrap().state, JobState::Running));
    assert!(matches!(
        jobs.job(fourth).unwrap().state,
        JobState::Queued(WaitReason::Capacity)
    ));
    jobs.cancel(fourth);
    assert!(
        jobs.poll()
            .into_iter()
            .any(|update| matches!(update, super::JobUpdate::Finished { id, .. } if id == fourth))
    );
    jobs.cancel_all();
}

#[test]
fn queued_jobs_pause_resume_and_reorder_without_starting() {
    let temp = TempDir::new();
    let mut jobs = JobManager::new();
    let mut ids = Vec::new();
    for name in ["one", "two", "three", "four", "five"] {
        ids.push(jobs.submit(
            delete(temp.path().join(name)),
            LaunchMode::Background,
            0,
            None,
        ));
    }
    let fourth = ids[3];
    let fifth = ids[4];
    assert!(matches!(
        jobs.job(fourth).unwrap().state,
        JobState::Queued(_)
    ));
    assert!(jobs.reorder_queued(fifth, -1));
    let order = jobs.jobs.iter().map(|job| job.id).collect::<Vec<_>>();
    assert!(
        order.iter().position(|id| *id == fifth).unwrap()
            < order.iter().position(|id| *id == fourth).unwrap()
    );

    assert!(jobs.toggle_pause(fifth));
    assert!(matches!(jobs.job(fifth).unwrap().state, JobState::Paused));
    assert!(jobs.toggle_pause(fifth));
    assert!(matches!(
        jobs.job(fifth).unwrap().state,
        JobState::Queued(_)
    ));
    jobs.cancel_all();
}

#[test]
fn running_job_pause_keeps_its_capacity_slot() {
    let temp = TempDir::new();
    let mut jobs = JobManager::new();
    let id = jobs.submit(
        delete(temp.path().join("one")),
        LaunchMode::Background,
        0,
        None,
    );
    assert!(jobs.toggle_pause(id));
    assert!(matches!(jobs.job(id).unwrap().state, JobState::Paused));
    assert_eq!(jobs.running_count(), 1);
    assert!(jobs.toggle_pause(id));
    assert!(matches!(jobs.job(id).unwrap().state, JobState::Running));
    jobs.cancel_all();
}

#[test]
fn overlap_waits_without_blocking_an_unrelated_job() {
    let temp = TempDir::new();
    let mut jobs = JobManager::new();
    let first = jobs.submit(
        delete(temp.path().join("media/show")),
        LaunchMode::Background,
        0,
        None,
    );
    let blocked = jobs.submit(
        delete(temp.path().join("media/show/season-one")),
        LaunchMode::Background,
        0,
        None,
    );
    let unrelated = jobs.submit(
        delete(temp.path().join("backup/other")),
        LaunchMode::Background,
        0,
        None,
    );

    assert!(matches!(jobs.job(first).unwrap().state, JobState::Running));
    assert!(matches!(
        jobs.job(blocked).unwrap().state,
        JobState::Queued(WaitReason::Overlap)
    ));
    assert!(matches!(
        jobs.job(unrelated).unwrap().state,
        JobState::Running
    ));
    let visible = jobs
        .visible_background()
        .into_iter()
        .map(|job| job.id)
        .collect::<Vec<_>>();
    assert_eq!(visible, [first, unrelated, blocked]);
    jobs.cancel_all();
}

#[test]
fn routes_independent_conflicts_to_their_own_jobs() {
    let temp = TempDir::new();
    let mut jobs = JobManager::new();
    let mut ids = Vec::new();
    for name in ["one", "two"] {
        let source_dir = temp.path().join(format!("source-{name}"));
        let destination = temp.path().join(format!("destination-{name}"));
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_dir.join("episode.mkv");
        fs::write(&source, name).unwrap();
        fs::write(destination.join("episode.mkv"), "existing").unwrap();
        ids.push(jobs.submit(
            JobRequest::Copy {
                sources: vec![source],
                destination,
                kind: OperationKind::Copy,
            },
            LaunchMode::Background,
            0,
            None,
        ));
    }

    let mut conflicts = Vec::new();
    for _ in 0..200 {
        for update in jobs.poll() {
            if let JobUpdate::Conflict { id, .. } = update {
                conflicts.push(id);
            }
        }
        if conflicts.len() == 2 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    conflicts.sort_unstable();
    ids.sort_unstable();
    assert_eq!(conflicts, ids);
    for id in &ids {
        jobs.answer_conflict(*id, ConflictChoice::Skip);
    }
    for _ in 0..200 {
        jobs.poll();
        if !jobs.has_active() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!jobs.has_active());
    jobs.cancel_all();
}
