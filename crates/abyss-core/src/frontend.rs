//! Shared presentation-neutral conventions used by every Abyss frontend.

use std::time::Duration;

use crate::jobs::{JobId, JobOutcome};
use crate::operation::OperationKind;
use crate::progress::{ProgressSnapshot, human_bytes};

/// Lifetime of a transient status or completed-job notice.
pub const NOTICE_LIFETIME: Duration = Duration::from_secs(8);

/// Formats a completed job consistently for terminal and graphical frontends.
pub fn completion_message(
    id: JobId,
    kind: OperationKind,
    outcome: &JobOutcome,
    snapshot: &ProgressSnapshot,
) -> String {
    match outcome {
        JobOutcome::Cancelled => format!("#{id} {} cancelled", kind.title()),
        JobOutcome::Failed(error) => format!("#{id} {} failed: {error}", kind.title()),
        JobOutcome::Succeeded => {
            let mut message = match kind {
                OperationKind::Copy
                | OperationKind::Move
                | OperationKind::Sync
                | OperationKind::Extract => format!(
                    "#{id} {} complete: {} items, {} transferred",
                    kind.title(),
                    snapshot.objects_done,
                    human_bytes(snapshot.physical_done)
                ),
                OperationKind::Archive => format!(
                    "#{id} Archive complete: {} items, {} input, {} output",
                    snapshot.objects_done,
                    human_bytes(snapshot.logical_done),
                    human_bytes(snapshot.physical_done)
                ),
                OperationKind::Delete | OperationKind::Trash => format!(
                    "#{id} {} complete: {} items removed",
                    kind.title(),
                    snapshot.objects_done
                ),
                OperationKind::Hash => format!(
                    "#{id} Hash database created: {} files, {}",
                    snapshot.objects_done,
                    human_bytes(snapshot.logical_done)
                ),
                OperationKind::Verify => format!(
                    "#{id} Hashes OK: {} files verified, {}",
                    snapshot.objects_done,
                    human_bytes(snapshot.logical_done)
                ),
                OperationKind::Test => format!(
                    "#{id} Archive OK: {} members, {}",
                    snapshot.objects_done,
                    human_bytes(snapshot.logical_done)
                ),
            };
            if matches!(
                kind,
                OperationKind::Copy
                    | OperationKind::Move
                    | OperationKind::Sync
                    | OperationKind::Extract
            ) {
                if snapshot.cloned_bytes > 0 {
                    message.push_str(&format!(", {} cloned", human_bytes(snapshot.cloned_bytes)));
                }
                if snapshot.linked_bytes > 0 {
                    message.push_str(&format!(", {} linked", human_bytes(snapshot.linked_bytes)));
                }
                if snapshot.skipped_objects > 0 {
                    message.push_str(&format!(", {} skipped", snapshot.skipped_objects));
                }
            }
            message
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_messages_are_operation_specific() {
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
            completion_message(7, OperationKind::Delete, &JobOutcome::Succeeded, &snapshot),
            "#7 Delete complete: 4 items removed"
        );
    }
}
