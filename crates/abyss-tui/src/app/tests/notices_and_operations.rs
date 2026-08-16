use std::time::{Duration, Instant};

use crate::app::dialogs::Modal;
use crate::app::state::App;
use crate::app::status::CompletionNotice;
use crate::frontend::{NOTICE_LIFETIME, completion_message};
use crate::jobs::JobOutcome;
use crate::operation::OperationKind;
use crate::progress::ProgressSnapshot;
use crate::test_support::TempDir;

#[test]
fn completion_notices_expire_at_eight_seconds() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    let now = Instant::now();
    let text = "#1 Delete complete: 4 items".to_owned();
    app.completion_notice = Some(CompletionNotice {
        text,
        outcome: JobOutcome::Succeeded,
        until: now + NOTICE_LIFETIME,
    });

    assert!(!app.expire_completion_notice(now));
    assert!(!app.expire_completion_notice(now + NOTICE_LIFETIME - Duration::from_nanos(1)));
    assert!(app.expire_completion_notice(now + NOTICE_LIFETIME));
    assert!(app.completion_notice.is_none());
    assert_eq!(app.status, "Ready");
}

#[test]
fn a_new_status_immediately_replaces_a_completion_notice() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.set_completion_notice("#1 Delete complete".to_owned(), JobOutcome::Succeeded);

    app.set_status("Resolving item…");

    assert!(app.completion_notice.is_none());
    assert_eq!(app.status, "Resolving item…");
}

#[test]
fn every_non_ready_status_expires_at_eight_seconds() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.set_status("Created /tmp/example");
    let deadline = app.status_until.unwrap();

    assert_eq!(app.status, "Created /tmp/example");
    assert!(!app.expire_status(deadline - Duration::from_nanos(1)));
    assert!(app.expire_status(deadline));
    assert_eq!(app.status, "Ready");
    assert!(app.status_until.is_none());
}

#[test]
fn informational_and_completion_notices_share_one_eight_second_lifetime() {
    assert_eq!(NOTICE_LIFETIME, Duration::from_secs(8));

    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    let before = Instant::now();
    app.set_completion_notice("done".to_owned(), JobOutcome::Succeeded);
    let after = Instant::now();
    let until = app.completion_notice.as_ref().unwrap().until;
    assert!(until >= before + NOTICE_LIFETIME);
    assert!(until <= after + NOTICE_LIFETIME);

    app.set_status("working");
    let until = app.status_until.unwrap();
    assert!(until >= after + NOTICE_LIFETIME);
    assert!(until <= Instant::now() + NOTICE_LIFETIME);
}

#[test]
fn completion_messages_are_operation_specific_and_omit_empty_options() {
    let snapshot = ProgressSnapshot {
        logical_done: 8 * 1024,
        physical_done: 3 * 1024,
        objects_done: 4,
        ..ProgressSnapshot::default()
    };
    assert_eq!(
        completion_message(7, OperationKind::Copy, &JobOutcome::Succeeded, &snapshot),
        "#7 Copy complete: 4 items, 3.0 KiB transferred"
    );
    assert_eq!(
        completion_message(7, OperationKind::Archive, &JobOutcome::Succeeded, &snapshot),
        "#7 Archive complete: 4 items, 8.0 KiB input, 3.0 KiB output"
    );
    assert_eq!(
        completion_message(7, OperationKind::Delete, &JobOutcome::Succeeded, &snapshot),
        "#7 Delete complete: 4 items removed"
    );
    assert_eq!(
        completion_message(7, OperationKind::Verify, &JobOutcome::Succeeded, &snapshot),
        "#7 Hashes OK: 4 files verified, 8.0 KiB"
    );

    let options = ProgressSnapshot {
        cloned_bytes: 1024,
        linked_bytes: 2048,
        skipped_objects: 2,
        ..snapshot
    };
    assert!(
        completion_message(8, OperationKind::Move, &JobOutcome::Succeeded, &options)
            .ends_with("1.0 KiB cloned, 2.0 KiB linked, 2 skipped")
    );
}

#[test]
fn completion_failures_and_cancellations_keep_operation_context() {
    let snapshot = ProgressSnapshot::default();
    assert_eq!(
        completion_message(2, OperationKind::Extract, &JobOutcome::Cancelled, &snapshot),
        "#2 Extract cancelled"
    );
    assert_eq!(
        completion_message(
            3,
            OperationKind::Archive,
            &JobOutcome::Failed("disk full".to_owned()),
            &snapshot
        ),
        "#3 Archive failed: disk full"
    );
}

#[test]
fn modal_errors_include_the_detail_in_the_bottom_notice() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());

    app.show_error("Archive", "disk full");

    assert_eq!(app.status, "Archive failed: disk full");
    assert!(matches!(
        app.modal,
        Some(Modal::Message { error: true, .. })
    ));
}
