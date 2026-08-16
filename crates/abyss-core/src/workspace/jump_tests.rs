use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use super::jump::{best_visit, score};
use super::state::{StoredLocation, VisitRecord};
use crate::storage::Location;
use crate::test_support::TempDir;
use crate::workspace::WorkspaceState;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn record(path: &std::path::Path, visits: u32, age_seconds: u64) -> VisitRecord {
    VisitRecord {
        location: StoredLocation::Local(path.to_owned()),
        visits,
        last_visit: now().saturating_sub(age_seconds),
    }
}

#[test]
fn recent_visits_outrank_older_ones_at_equal_frequency() {
    let path = std::path::Path::new("/tmp");
    let fresh = score(&record(path, 5, 60));
    let yesterday = score(&record(path, 5, 90_000));
    let ancient = score(&record(path, 5, 30 * 86_400));
    assert!(fresh > yesterday, "{fresh} should beat {yesterday}");
    assert!(yesterday > ancient, "{yesterday} should beat {ancient}");
}

#[test]
fn frequent_visits_outrank_rare_ones_at_equal_age() {
    let path = std::path::Path::new("/tmp");
    assert!(score(&record(path, 20, 60)) > score(&record(path, 2, 60)));
}

#[test]
fn a_stale_favourite_can_still_beat_a_one_off_recent_visit() {
    let path = std::path::Path::new("/tmp");
    // 100 visits a month ago (×0.25 = 25) beats one visit a minute ago (×4 = 4).
    assert!(score(&record(path, 100, 30 * 86_400)) > score(&record(path, 1, 60)));
}

#[test]
fn the_best_match_is_the_highest_scoring_existing_directory() {
    let temp = TempDir::new();
    let rare = temp.path().join("project-rare");
    let common = temp.path().join("project-common");
    fs::create_dir_all(&rare).unwrap();
    fs::create_dir_all(&common).unwrap();

    let visits = vec![record(&rare, 1, 120), record(&common, 50, 120)];
    assert_eq!(best_visit(&visits, "project"), Some(common));
}

#[test]
fn directories_that_no_longer_exist_are_skipped() {
    let temp = TempDir::new();
    let present = temp.path().join("present");
    fs::create_dir_all(&present).unwrap();
    let missing = temp.path().join("missing");

    // The missing one scores far higher but cannot be jumped to.
    let visits = vec![record(&missing, 500, 60), record(&present, 1, 60)];
    assert_eq!(
        best_visit(&visits, ""),
        None,
        "an empty query matches nothing"
    );
    assert_eq!(best_visit(&visits, "present"), Some(present));
    assert_eq!(best_visit(&visits, "missing"), None);
}

#[test]
fn matching_is_case_insensitive_and_substring_based() {
    let temp = TempDir::new();
    let path = temp.path().join("MyProject");
    fs::create_dir_all(&path).unwrap();
    let visits = vec![record(&path, 3, 60)];
    assert_eq!(best_visit(&visits, "myproj"), Some(path.clone()));
    assert_eq!(best_visit(&visits, "PROJECT"), Some(path));
}

#[test]
fn browsing_records_visits_and_counts_repeats() {
    let temp = TempDir::new();
    let mut state = WorkspaceState::default();
    let location = Location::Local(temp.path().to_owned());

    state.record_history(&location);
    assert_eq!(state.visits.len(), 1);
    assert_eq!(state.visits[0].visits, 1);

    state.record_history(&location);
    assert_eq!(
        state.visits.len(),
        1,
        "the same directory is not duplicated"
    );
    assert_eq!(state.visits[0].visits, 2, "revisiting increments the count");
}

#[test]
fn visits_survive_a_workspace_round_trip() {
    let temp = TempDir::new();
    let mut state = WorkspaceState::default();
    state.record_history(&Location::Local(temp.path().to_owned()));

    let encoded = toml::to_string(&state).expect("serialize");
    let decoded: WorkspaceState = toml::from_str(&encoded).expect("deserialize");
    assert_eq!(decoded.visits.len(), 1);
    assert_eq!(decoded.visits[0].visits, 1);
}

#[test]
fn a_workspace_file_without_visits_still_loads() {
    // Files written before smart jump kept history are still valid.
    let older = r#"
version = 1
bandwidth_limit = 0
archive_buffer_capacity = 134217728
bookmarks = []
history = []
"#;
    let decoded: WorkspaceState = toml::from_str(older).expect("older files must still load");
    assert!(decoded.visits.is_empty());
}
