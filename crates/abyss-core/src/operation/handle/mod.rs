mod local;
mod storage;
mod tools;

use std::sync::atomic::Ordering;

use crate::operation::types::{ConflictChoice, OperationEvent, OperationHandle};

impl OperationHandle {
    pub fn try_recv(&self) -> Option<OperationEvent> {
        self.events.try_recv().ok()
    }

    pub fn answer_conflict(&self, choice: ConflictChoice) {
        let _ = self.conflict_response.send(choice);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.stats.set_paused(false);
        let _ = self.conflict_response.send(ConflictChoice::Cancel);
    }

    pub fn set_paused(&self, paused: bool) {
        self.stats.set_paused(paused);
    }

    pub fn set_bandwidth_limit(&self, bytes_per_second: u64) {
        self.stats.set_bandwidth_limit(bytes_per_second);
    }
}
