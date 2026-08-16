use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{SearchKind, SearchRequest, search_contents, search_files};
use crate::test_support::TempDir;

fn tree() -> TempDir {
    let temp = TempDir::new();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn helper() -> u32 {\n    7\n}\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "# Project\n\nhello world\n").unwrap();
    fs::write(root.join("target/main.rs"), "generated hello\n").unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    temp
}

fn request(temp: &TempDir, query: &str, kind: SearchKind, respect_ignore: bool) -> SearchRequest {
    SearchRequest {
        root: temp.path().to_owned(),
        query: query.to_owned(),
        kind,
        respect_ignore,
    }
}

fn idle() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn file_search_matches_names_case_insensitively() {
    let temp = tree();
    let hits = search_files(&request(&temp, "MAIN", SearchKind::Files, false), &idle())
        .expect("search runs");
    assert!(
        hits.iter().any(|hit| hit.path.ends_with("src/main.rs")),
        "expected src/main.rs in {hits:?}"
    );
}

#[test]
fn file_search_honours_gitignore_when_asked() {
    let temp = tree();
    let ignored = search_files(&request(&temp, "main", SearchKind::Files, true), &idle())
        .expect("search runs");
    assert!(
        !ignored
            .iter()
            .any(|hit| hit.path.starts_with(temp.path().join("target"))),
        "target/ is gitignored and should be skipped: {ignored:?}"
    );

    let everything = search_files(&request(&temp, "main", SearchKind::Files, false), &idle())
        .expect("search runs");
    assert!(
        everything.len() > ignored.len(),
        "disabling the ignore rules must widen the search"
    );
}

#[test]
fn content_search_reports_the_matching_line_and_number() {
    let temp = tree();
    let hits = search_contents(
        &request(&temp, "hello", SearchKind::Contents, true),
        &idle(),
    )
    .expect("search runs");
    let readme = hits
        .iter()
        .find(|hit| hit.path.ends_with("README.md"))
        .expect("README contains hello");
    assert_eq!(readme.line, Some(3));
    assert!(readme.preview.contains("hello world"));
}

#[test]
fn content_search_is_smart_case() {
    let temp = tree();
    // Lowercase matches regardless of case.
    let lower = search_contents(
        &request(&temp, "project", SearchKind::Contents, true),
        &idle(),
    )
    .expect("search runs");
    assert!(
        !lower.is_empty(),
        "lowercase query should match '# Project'"
    );

    // An uppercase letter makes it case sensitive.
    let upper = search_contents(
        &request(&temp, "PROJECT", SearchKind::Contents, true),
        &idle(),
    )
    .expect("search runs");
    assert!(
        upper.is_empty(),
        "uppercase query should not match '# Project'"
    );
}

#[test]
fn content_search_accepts_regular_expressions() {
    let temp = tree();
    let hits = search_contents(
        &request(&temp, r"fn \w+\(", SearchKind::Contents, true),
        &idle(),
    )
    .expect("search runs");
    assert!(
        hits.len() >= 2,
        "both source files declare a function: {hits:?}"
    );
}

#[test]
fn an_invalid_pattern_is_reported_rather_than_panicking() {
    let temp = tree();
    let error = search_contents(
        &request(&temp, "unclosed(", SearchKind::Contents, true),
        &idle(),
    )
    .expect_err("an unbalanced group is not a valid regex");
    assert!(error.contains("invalid search pattern"), "{error}");
}

#[test]
fn empty_queries_and_missing_roots_are_rejected() {
    let temp = tree();
    assert!(search_files(&request(&temp, "   ", SearchKind::Files, true), &idle()).is_err());
    assert!(search_contents(&request(&temp, "", SearchKind::Contents, true), &idle()).is_err());

    let mut missing = request(&temp, "x", SearchKind::Files, true);
    missing.root = temp.path().join("does-not-exist");
    assert!(search_files(&missing, &idle()).is_err());
}

#[test]
fn a_cancelled_search_stops_early() {
    let temp = tree();
    let cancelled = Arc::new(AtomicBool::new(true));
    let hits = search_files(
        &request(&temp, "main", SearchKind::Files, false),
        &cancelled,
    )
    .expect("cancelling is not an error");
    assert!(
        hits.is_empty(),
        "nothing should be collected once cancelled"
    );
    assert!(cancelled.load(Ordering::Relaxed));
}
