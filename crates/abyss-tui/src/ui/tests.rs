use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, MenuCategory};
use crate::operation::OperationKind;
use crate::progress::{OperationPhase, ProgressSnapshot, SpeedSnapshot};
use crate::test_support::TempDir;
use crate::ui::helpers::{
    background_job_label, display_width, fit, fit_filename, format_percent, operation_body,
    operation_speed_line,
};
use crate::ui::panes::columns;
use crate::ui::status::{notice_line, status_details};
use crate::ui::theme::{
    BACKGROUND_ETA_BREAKPOINT, BACKGROUND_PERCENT_WIDTH, BACKGROUND_SPEED_WIDTH,
};

fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for row in 0..buffer.area.height {
        for column in 0..buffer.area.width {
            text.push_str(buffer.cell((column, row)).unwrap().symbol());
        }
        text.push('\n');
    }
    text
}

fn field_column(line: &str, content: &str, field_width: usize) -> usize {
    let byte = line.find(content).unwrap();
    display_width(&line[..byte]) - field_width.saturating_sub(display_width(content))
}

#[test]
fn filename_truncation_keeps_the_extension_visible() {
    let value = fit_filename("very-long-episode-name-s01e02.mkv", 16);
    assert_eq!(display_width(&value), 16);
    assert!(value.starts_with("very-lo"));
    assert!(value.ends_with("e02.mkv"));
}

#[test]
fn list_columns_align_for_combining_and_wide_names() {
    for name in [
        "Ты — мой триумф [Flarrow Films]",
        "日本語のフォルダ",
        "family-👨‍👩‍👧‍👦-archive",
    ] {
        let row = columns(name, "1.0KiB", "2h", 40, false);
        assert_eq!(display_width(&row), 40);
        let size = row.find("1.0KiB").unwrap();
        let age = row.rfind("2h").unwrap();
        assert_eq!(display_width(&row[..size]), 26);
        assert_eq!(display_width(&row[..age]), 38);
    }
}

#[test]
fn ordinary_fit_still_truncates_folders_at_the_end() {
    assert_eq!(fit("very-long-folder", 9), "very-lon…");
}

#[test]
fn ready_status_is_not_rendered_beside_the_current_name() {
    assert_eq!(status_details(0, None), "");
    assert_eq!(status_details(3, None), "  [3 marked]");
    assert_eq!(
        status_details(0, Some("Source unavailable")),
        " — Source unavailable"
    );
}

#[test]
fn notices_use_the_full_width_and_middle_truncate_unicode() {
    let short = notice_line("Created café", 30);
    assert_eq!(display_width(&short), 30);
    assert!(short.starts_with(" Created café"));

    let long = notice_line(
        "Copy /資料/とても長い入力パス completed with important-tail.zip",
        32,
    );
    assert_eq!(display_width(&long), 32);
    assert!(long.starts_with(" Copy"));
    assert!(long.contains('…'));
    assert!(long.trim_end().ends_with("important-tail.zip"));
}

#[test]
fn minimum_terminal_renders_menu_bar_and_scrollable_bookmark_manager() {
    let temp = TempDir::new();
    let mut app = App::new(temp.path().to_owned(), temp.path().to_owned());
    app.open_menu_category(MenuCategory::Bookmarks);
    let mut terminal = Terminal::new(TestBackend::new(52, 10)).unwrap();

    terminal
        .draw(|frame| {
            super::render(frame, &app);
        })
        .unwrap();
    let first = terminal_text(&terminal);
    let top = first.lines().next().unwrap();
    assert!(top.contains("Navigate"));
    assert!(top.contains("Pane"));
    assert!(top.contains("Bookmarks"));
    assert!(top.contains("Tools"));
    assert!(top.contains("Sort:"));
    assert!(first.contains("1 Empty"));
    assert!(first.contains("Set"));

    app.app_menu.as_mut().unwrap().selected = 8;
    terminal
        .draw(|frame| {
            super::render(frame, &app);
        })
        .unwrap();
    assert!(terminal_text(&terminal).contains("9 Empty"));

    app.synchronized_scrolling = true;
    app.comparison = true;
    app.open_menu_category(MenuCategory::Pane);
    terminal
        .draw(|frame| {
            super::render(frame, &app);
        })
        .unwrap();
    let pane_menu = terminal_text(&terminal);
    assert!(pane_menu.contains("Synchronized Scrolling"));
    assert!(pane_menu.contains("Directory Comparison"));
    assert_eq!(pane_menu.matches('✓').count(), 2);
}

#[test]
fn background_progress_columns_do_not_move_with_content() {
    let width = 100;
    let short = background_job_label(
        1,
        OperationKind::Archive,
        "Compress",
        "a.txt",
        0.01,
        None,
        None,
        width,
    );
    let long = background_job_label(
        98_765,
        OperationKind::Move,
        "Finalize",
        "日本語のとても長いファイル名-family-👨‍👩‍👧‍👦-episode.mkv",
        0.888,
        Some(12 * 1024 * 1024),
        Some(Duration::from_secs(5_431)),
        width,
    );

    assert_eq!(display_width(&short), width);
    assert_eq!(display_width(&long), width);
    assert_eq!(
        field_column(&short, "1.0%", BACKGROUND_PERCENT_WIDTH),
        field_column(&long, "88.8%", BACKGROUND_PERCENT_WIDTH)
    );
    assert_eq!(
        field_column(&short, "measuring", BACKGROUND_SPEED_WIDTH),
        field_column(&long, "12.0 MiB/s", BACKGROUND_SPEED_WIDTH)
    );
    assert!(long.contains('…'));
    assert!(long.contains("episode.mkv"));
}

#[test]
fn narrow_background_progress_keeps_percent_and_speed_fixed() {
    let narrow = background_job_label(
        7,
        OperationKind::Extract,
        "Extracting",
        "extremely-long-archive-member-name.tar.zst",
        1.0,
        Some(1024),
        Some(Duration::from_secs(90)),
        52,
    );
    let before_breakpoint = background_job_label(
        7,
        OperationKind::Extract,
        "Extracting",
        "name.tar.zst",
        1.0,
        Some(1024),
        Some(Duration::from_secs(90)),
        BACKGROUND_ETA_BREAKPOINT - 1,
    );
    let at_breakpoint = background_job_label(
        7,
        OperationKind::Extract,
        "Extracting",
        "name.tar.zst",
        1.0,
        Some(1024),
        Some(Duration::from_secs(90)),
        BACKGROUND_ETA_BREAKPOINT,
    );

    assert_eq!(display_width(&narrow), 52);
    assert!(narrow.contains("100.0%"));
    assert!(narrow.contains("1.0 KiB/s"));
    assert!(!before_breakpoint.contains("ETA"));
    assert!(at_breakpoint.contains("ETA 1m30s"));
}

#[test]
fn foreground_progress_rows_keep_fixed_widths_and_labels() {
    let mut early = ProgressSnapshot {
        phase: OperationPhase::Compressing,
        logical_done: 1,
        physical_done: 0,
        objects_done: 1,
        total_bytes: 10 * 1024 * 1024,
        total_objects: 123_456,
        ..ProgressSnapshot::default()
    };
    let early_body = operation_body(
        "Compressing",
        "日本語-family-👨‍👩‍👧‍👦-very-long-video-name.mkv",
        None,
        &early,
        72,
    );
    early.phase = OperationPhase::Finalizing;
    early.logical_done = early.total_bytes;
    early.physical_done = 4 * 1024 * 1024;
    early.objects_done = early.total_objects;
    let final_body = operation_body("Finalizing", "x.mkv", None, &early, 72);

    for body in [&early_body, &final_body] {
        assert!(body.lines().all(|line| display_width(line) == 72));
    }
    let early_lines = early_body.lines().collect::<Vec<_>>();
    let final_lines = final_body.lines().collect::<Vec<_>>();
    assert_eq!(
        field_column(early_lines[3], "Ratio:", 6),
        field_column(final_lines[3], "Ratio:", 6)
    );
    assert_eq!(
        field_column(early_lines[3], "Gain:", 5),
        field_column(final_lines[3], "Gain:", 5)
    );
    assert!(early_lines[0].contains('…'));
    assert!(early_lines[0].trim_end().ends_with("video-name.mkv"));
    assert_eq!(display_width(&format_percent(0.0)), 7);
    assert_eq!(display_width(&format_percent(1.0)), 7);

    let measuring = operation_speed_line(None, 72);
    let running = operation_speed_line(
        Some(SpeedSnapshot {
            current: 1024,
            average: 2048,
            elapsed: Duration::from_secs(7_321),
        }),
        72,
    );
    assert_eq!(display_width(&measuring), 72);
    assert_eq!(display_width(&running), 72);
    assert_eq!(measuring.find("Average:"), running.find("Average:"));
    assert_eq!(measuring.find("Elapsed:"), running.find("Elapsed:"));
}
