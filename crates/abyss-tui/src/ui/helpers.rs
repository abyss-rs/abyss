use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::jobs::{Job, JobId, JobOutcome, JobState, WaitReason};
use crate::operation::OperationKind;
use crate::progress::{OperationPhase, ProgressSnapshot, SpeedSnapshot, human_bytes};
use crate::ui::theme::{
    BACKGROUND_ETA_BREAKPOINT, BACKGROUND_ETA_WIDTH, BACKGROUND_ID_WIDTH, BACKGROUND_KIND_WIDTH,
    BACKGROUND_PERCENT_WIDTH, BACKGROUND_SPEED_WIDTH, BACKGROUND_STATE_WIDTH,
};

pub(crate) fn centered(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(crate) fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height.saturating_sub(vertical.saturating_mul(2)),
    )
}

pub(crate) fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

pub(crate) fn fit(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    format!("{}…", take_prefix_columns(value, width - 1))
}

pub(crate) fn fit_filename(value: &str, width: usize) -> String {
    truncate_middle(value, width)
}

pub(crate) fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub(crate) fn take_prefix_columns(value: &str, width: usize) -> String {
    let mut used = 0;
    value
        .graphemes(true)
        .take_while(|grapheme| {
            let next = used + display_width(grapheme);
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect()
}

pub(crate) fn take_suffix_columns(value: &str, width: usize) -> String {
    let mut used = 0;
    let mut graphemes = value
        .graphemes(true)
        .rev()
        .take_while(|grapheme| {
            let next = used + display_width(grapheme);
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect::<Vec<_>>();
    graphemes.reverse();
    graphemes.concat()
}

pub(crate) fn pad_right(value: &str, width: usize) -> String {
    let value = fit(value, width);
    let padding = width.saturating_sub(display_width(&value));
    format!("{value}{}", " ".repeat(padding))
}

pub(crate) fn pad_left(value: &str, width: usize) -> String {
    let value = fit(value, width);
    let padding = width.saturating_sub(display_width(&value));
    format!("{}{value}", " ".repeat(padding))
}

pub(crate) fn compact_operation_kind(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Copy => "Copy",
        OperationKind::Move => "Move",
        OperationKind::Delete => "Delete",
        OperationKind::Trash => "Trash",
        OperationKind::Sync => "Sync",
        OperationKind::Archive => "Archive",
        OperationKind::Hash => "Hash",
        OperationKind::Verify => "Verify",
        OperationKind::Extract => "Extract",
        OperationKind::Test => "Test",
    }
}

pub(crate) fn phase_name(phase: OperationPhase) -> &'static str {
    match phase {
        OperationPhase::Scanning => "Scanning",
        OperationPhase::Copying => "Copying",
        OperationPhase::Compressing => "Compress",
        OperationPhase::Hashing => "Hashing",
        OperationPhase::WritingHashes => "Writing",
        OperationPhase::VerifyingHashes => "Verifying",
        OperationPhase::Finalizing => "Finalize",
        OperationPhase::Extracting => "Extracting",
        OperationPhase::Testing => "Testing",
        OperationPhase::Moving => "Moving",
        OperationPhase::Deleting => "Deleting",
    }
}

pub(crate) fn background_job_state(job: &Job, snapshot: &ProgressSnapshot) -> &'static str {
    match &job.state {
        JobState::Queued(_) => "Queued",
        JobState::Paused => "Paused",
        JobState::WaitingConflict(_) => "Conflict",
        JobState::Cancelling => "Cancelling",
        JobState::Finished {
            outcome: JobOutcome::Succeeded,
            ..
        } => "Complete",
        JobState::Finished {
            outcome: JobOutcome::Cancelled,
            ..
        } => "Cancelled",
        JobState::Finished {
            outcome: JobOutcome::Failed(_),
            ..
        } => "Failed",
        JobState::Running => phase_name(snapshot.phase),
    }
}

pub(crate) fn format_percent(ratio: f64) -> String {
    format!("{:>6.1}%", ratio.clamp(0.0, 1.0) * 100.0)
}

pub(crate) fn compact_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else if seconds < 360_000 {
        format!("{}h{:02}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else {
        format!("{}d{:02}h", seconds / 86_400, (seconds % 86_400) / 3_600)
    }
}

pub(crate) fn compact_count(value: u64) -> String {
    const UNITS: [&str; 6] = ["", "K", "M", "B", "T", "Q"];
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1_000.0 && unit + 1 < UNITS.len() {
        scaled /= 1_000.0;
        unit += 1;
    }
    if unit == 0 {
        value.to_string()
    } else if scaled >= 100.0 {
        format!("{scaled:.0}{}", UNITS[unit])
    } else {
        format!("{scaled:.1}{}", UNITS[unit])
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn background_job_label(
    id: JobId,
    kind: OperationKind,
    state: &str,
    filename: &str,
    ratio: f64,
    speed: Option<u64>,
    eta: Option<Duration>,
    width: usize,
) -> String {
    let show_eta = width >= BACKGROUND_ETA_BREAKPOINT;
    let separators = if show_eta { 6 } else { 5 };
    let fixed_width = BACKGROUND_ID_WIDTH
        + BACKGROUND_KIND_WIDTH
        + BACKGROUND_STATE_WIDTH
        + BACKGROUND_PERCENT_WIDTH
        + BACKGROUND_SPEED_WIDTH
        + if show_eta { BACKGROUND_ETA_WIDTH } else { 0 }
        + separators;
    let filename_width = width.saturating_sub(fixed_width);
    let name = if filename.is_empty() { "—" } else { filename };
    let name = pad_right(&fit_filename(name, filename_width), filename_width);
    let id = pad_right(&format!(" #{id}"), BACKGROUND_ID_WIDTH);
    let kind = pad_right(compact_operation_kind(kind), BACKGROUND_KIND_WIDTH);
    let state = pad_right(state, BACKGROUND_STATE_WIDTH);
    let percent = format_percent(ratio);
    let speed = speed
        .map(|value| format!("{}/s", human_bytes(value)))
        .unwrap_or_else(|| "measuring".to_owned());
    let speed = pad_left(&speed, BACKGROUND_SPEED_WIDTH);
    let label = if show_eta {
        let eta = eta
            .map(|value| format!("ETA {}", compact_duration(value)))
            .unwrap_or_else(|| "ETA —".to_owned());
        format!(
            "{id} {kind} {state} {name} {percent} {speed} {}",
            pad_left(&eta, BACKGROUND_ETA_WIDTH)
        )
    } else {
        format!("{id} {kind} {state} {name} {percent} {speed}")
    };
    pad_right(&label, width)
}

pub(crate) fn operation_body(
    phase: &str,
    filename: &str,
    queued_status: Option<&str>,
    snapshot: &ProgressSnapshot,
    width: usize,
) -> String {
    let phase_width = 10;
    let filename_width = width.saturating_sub(phase_width + 2);
    let name = if filename.is_empty() { "—" } else { filename };
    let first = format!(
        "{}  {}",
        pad_right(phase, phase_width),
        pad_right(&fit_filename(name, filename_width), filename_width)
    );
    let second = if let Some(status) = queued_status {
        status.to_owned()
    } else if snapshot.phase == OperationPhase::Scanning {
        format!(
            "Scanned: {}",
            pad_left(&compact_count(snapshot.scanned_objects), 8)
        )
    } else {
        format!(
            "Items: {} / {}   Skipped: {}",
            pad_left(&compact_count(snapshot.objects_done), 8),
            pad_left(&compact_count(snapshot.total_objects), 8),
            pad_left(&compact_count(snapshot.skipped_objects), 8)
        )
    };
    let (third, fourth) = if matches!(
        snapshot.phase,
        OperationPhase::Compressing | OperationPhase::Finalizing
    ) {
        let ratio = if snapshot.logical_done > 0 && snapshot.physical_done > 0 {
            format!(
                "{:.1}%",
                snapshot.physical_done as f64 / snapshot.logical_done as f64 * 100.0
            )
        } else {
            "—".to_owned()
        };
        let gain = if snapshot.physical_done > 0 {
            format!(
                "{:.2}×",
                snapshot.logical_done as f64 / snapshot.physical_done as f64
            )
        } else {
            "—".to_owned()
        };
        (
            format!(
                "Original: {} / {}",
                pad_left(&human_bytes(snapshot.logical_done), 10),
                pad_left(&human_bytes(snapshot.total_bytes), 10)
            ),
            format!(
                "Compressed: {}   Ratio: {}   Gain: {}",
                pad_left(&human_bytes(snapshot.physical_done), 10),
                pad_left(&ratio, 7),
                pad_left(&gain, 8)
            ),
        )
    } else {
        (
            format!(
                "Logical: {} / {}",
                pad_left(&human_bytes(snapshot.logical_done), 10),
                pad_left(&human_bytes(snapshot.total_bytes), 10)
            ),
            format!(
                "Physical: {}   Cloned: {}   Linked: {}",
                pad_left(&human_bytes(snapshot.physical_done), 10),
                pad_left(&human_bytes(snapshot.cloned_bytes), 10),
                pad_left(&human_bytes(snapshot.linked_bytes), 10)
            ),
        )
    };
    [first, second, third, fourth]
        .map(|line| pad_right(&line, width))
        .join("\n")
}

pub(crate) fn operation_speed_line(speed: Option<SpeedSnapshot>, width: usize) -> String {
    let (current, average, elapsed) = speed.map_or_else(
        || {
            (
                "measuring".to_owned(),
                "measuring".to_owned(),
                "—".to_owned(),
            )
        },
        |speed| {
            (
                format!("{}/s", human_bytes(speed.current)),
                format!("{}/s", human_bytes(speed.average)),
                compact_duration(speed.elapsed),
            )
        },
    );
    pad_right(
        &format!(
            "Speed: {}   Average: {}   Elapsed: {}",
            pad_left(&current, 13),
            pad_left(&average, 13),
            pad_left(&elapsed, 10)
        ),
        width,
    )
}

pub(crate) fn compact_job_state(job: &Job, snapshot: &ProgressSnapshot) -> String {
    match &job.state {
        JobState::Queued(WaitReason::Capacity) => "Queued: waiting for slot".to_owned(),
        JobState::Queued(WaitReason::Overlap) => "Queued: path overlap".to_owned(),
        JobState::Paused => "Paused".to_owned(),
        JobState::WaitingConflict(_) => "Waiting for conflict choice".to_owned(),
        JobState::Cancelling => "Cancelling".to_owned(),
        JobState::Finished {
            outcome: JobOutcome::Succeeded,
            ..
        } => "Complete".to_owned(),
        JobState::Finished {
            outcome: JobOutcome::Cancelled,
            ..
        } => "Cancelled".to_owned(),
        JobState::Finished {
            outcome: JobOutcome::Failed(error),
            ..
        } => format!("Failed: {error}"),
        JobState::Running => match snapshot.phase {
            OperationPhase::Scanning => "Scanning".to_owned(),
            OperationPhase::Copying => "Copying".to_owned(),
            OperationPhase::Compressing => "Compressing".to_owned(),
            OperationPhase::Hashing => "Hashing".to_owned(),
            OperationPhase::WritingHashes => "Writing hashes".to_owned(),
            OperationPhase::VerifyingHashes => "Verifying hashes".to_owned(),
            OperationPhase::Finalizing => "Finalizing".to_owned(),
            OperationPhase::Extracting => "Extracting".to_owned(),
            OperationPhase::Testing => "Testing".to_owned(),
            OperationPhase::Moving => "Moving".to_owned(),
            OperationPhase::Deleting => "Deleting".to_owned(),
        },
    }
}

pub(crate) fn job_eta(job: &Job, snapshot: &ProgressSnapshot) -> Option<Duration> {
    let speed = job.speed?;
    let bytes_per_second = speed.current.max(speed.average / 2);
    if bytes_per_second == 0 || snapshot.total_bytes <= snapshot.logical_done {
        return None;
    }
    Some(Duration::from_secs(
        snapshot.total_bytes.saturating_sub(snapshot.logical_done) / bytes_per_second,
    ))
}

pub(crate) fn truncate_middle(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width < 5 {
        return fit(value, width);
    }
    let left = (width - 1) / 2;
    let right = width - left - 1;
    let start = take_prefix_columns(value, left);
    let end = take_suffix_columns(value, right);
    format!("{start}…{end}")
}

pub(crate) fn human_bytes_compact(bytes: u64) -> String {
    if bytes < 1024 {
        bytes.to_string()
    } else {
        human_bytes(bytes).replace(' ', "")
    }
}

pub(crate) fn format_age(modified: SystemTime) -> String {
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    let seconds = age.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else if seconds < 31_536_000 {
        format!("{}d", seconds / 86_400)
    } else {
        format!("{}y", seconds / 31_536_000)
    }
}
