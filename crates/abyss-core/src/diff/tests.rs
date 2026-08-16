use std::fs;

use super::{DiffTag, diff_files, diff_text};
use crate::test_support::TempDir;

#[test]
fn identical_input_produces_no_changes() {
    let result = diff_text("one\ntwo\nthree\n", "one\ntwo\nthree\n");
    assert!(result.identical);
    assert!(result.lines.is_empty());
    assert_eq!(result.stats.inserted, 0);
    assert_eq!(result.stats.deleted, 0);
}

#[test]
fn a_changed_line_shows_as_one_delete_and_one_insert() {
    let result = diff_text("one\ntwo\nthree\n", "one\nTWO\nthree\n");
    assert!(!result.identical);
    assert_eq!(result.stats.deleted, 1);
    assert_eq!(result.stats.inserted, 1);

    let deleted = result
        .lines
        .iter()
        .find(|line| line.tag == DiffTag::Delete)
        .expect("a deleted line");
    assert_eq!(deleted.text, "two");
    assert_eq!(
        deleted.left,
        Some(2),
        "deletions carry the left line number"
    );
    assert!(deleted.right.is_none());

    let inserted = result
        .lines
        .iter()
        .find(|line| line.tag == DiffTag::Insert)
        .expect("an inserted line");
    assert_eq!(inserted.text, "TWO");
    assert_eq!(inserted.right, Some(2));
    assert!(inserted.left.is_none());
}

#[test]
fn unchanged_lines_are_kept_as_context() {
    let result = diff_text("one\ntwo\nthree\n", "one\nTWO\nthree\n");
    let context: Vec<_> = result
        .lines
        .iter()
        .filter(|line| line.tag == DiffTag::Context)
        .map(|line| line.text.as_str())
        .collect();
    assert!(context.contains(&"one"), "context lines: {context:?}");
    assert!(context.contains(&"three"), "context lines: {context:?}");
}

#[test]
fn distant_changes_are_separated_rather_than_run_together() {
    // Two edits far apart, with more than the context window between them.
    let mut left: Vec<String> = (0..40).map(|index| format!("line {index}")).collect();
    let mut right = left.clone();
    right[1] = "changed near the top".to_owned();
    right[38] = "changed near the bottom".to_owned();
    left.push(String::new());
    right.push(String::new());

    let result = diff_text(&left.join("\n"), &right.join("\n"));
    assert!(
        result
            .lines
            .iter()
            .any(|line| line.tag == DiffTag::Separator),
        "a gap of unchanged lines should be marked, not silently dropped"
    );
    // The untouched middle must not be included in full.
    assert!(
        result.lines.len() < left.len(),
        "only changed regions and their context belong in the output"
    );
}

#[test]
fn pure_additions_and_removals_are_counted_correctly() {
    let added = diff_text("one\n", "one\ntwo\n");
    assert_eq!(added.stats.inserted, 1);
    assert_eq!(added.stats.deleted, 0);

    let removed = diff_text("one\ntwo\n", "one\n");
    assert_eq!(removed.stats.inserted, 0);
    assert_eq!(removed.stats.deleted, 1);
}

#[test]
fn files_are_read_from_disk_and_compared() {
    let temp = TempDir::new();
    let left = temp.path().join("left.txt");
    let right = temp.path().join("right.txt");
    fs::write(&left, "alpha\nbeta\n").unwrap();
    fs::write(&right, "alpha\ngamma\n").unwrap();

    let result = diff_files(&left, &right).expect("both files are readable text");
    assert_eq!(result.stats.inserted, 1);
    assert_eq!(result.stats.deleted, 1);
}

#[test]
fn binaries_and_directories_are_refused_with_a_reason() {
    let temp = TempDir::new();
    let text = temp.path().join("text.txt");
    let binary = temp.path().join("binary.bin");
    fs::write(&text, "hello\n").unwrap();
    fs::write(&binary, [0xff, 0xfe, 0x00, 0x01]).unwrap();

    let error = diff_files(&text, &binary).expect_err("invalid UTF-8 is not diffable");
    assert!(error.contains("not a text file"), "{error}");

    let error = diff_files(&text, temp.path()).expect_err("a directory is not diffable");
    assert!(error.contains("is a directory"), "{error}");
}

#[test]
fn a_missing_file_reports_its_path() {
    let temp = TempDir::new();
    let present = temp.path().join("present.txt");
    fs::write(&present, "hello\n").unwrap();
    let error =
        diff_files(&present, &temp.path().join("absent.txt")).expect_err("missing file errors");
    assert!(error.contains("absent.txt"), "{error}");
}
