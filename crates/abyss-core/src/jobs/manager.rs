use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use crate::Error;
use crate::jobs::types::{
    AccessMode, Job, JobId, JobOutcome, JobRequest, JobState, JobUpdate, LaunchMode, PathAccess,
    WaitReason,
};
use crate::operation::{ConflictChoice, OperationEvent};
use crate::progress::Speedometer;
use crate::storage::Location;

const MAX_RUNNING: usize = 3;
const HISTORY_LIMIT: usize = 50;

pub struct JobManager {
    pub(crate) jobs: VecDeque<Job>,
    pending_updates: VecDeque<JobUpdate>,
    next_id: JobId,
    bandwidth_limit: u64,
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: VecDeque::new(),
            pending_updates: VecDeque::new(),
            next_id: 1,
            bandwidth_limit: 0,
        }
    }

    pub fn submit(
        &mut self,
        request: JobRequest,
        launch: LaunchMode,
        initiating_pane: usize,
        delete_paths: Option<Vec<PathBuf>>,
    ) -> JobId {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let kind = request.kind();
        let accesses = request.accesses();
        self.jobs.push_back(Job {
            id,
            kind,
            launch,
            state: JobState::Queued(WaitReason::Capacity),
            initiating_pane,
            delete_paths,
            request: Some(request),
            accesses,
            handle: None,
            speedometer: Speedometer::new(),
            speed: None,
            final_snapshot: None,
        });
        self.schedule();
        id
    }

    pub fn poll(&mut self) -> Vec<JobUpdate> {
        let mut updates = self.pending_updates.drain(..).collect::<Vec<_>>();
        for job in &mut self.jobs {
            let Some(handle) = &job.handle else {
                continue;
            };
            let snapshot = handle.stats.snapshot();
            // Report useful file throughput. Compressed PVC transfers can put
            // far fewer bytes on the wire than they copy logically; the
            // operation dialog exposes physical I/O separately.
            job.speed = Some(job.speedometer.update(snapshot.logical_done));
            while let Some(event) = handle.try_recv() {
                match event {
                    OperationEvent::Conflict(path) => {
                        job.state = JobState::WaitingConflict(path.clone());
                        updates.push(JobUpdate::Conflict { id: job.id, path });
                    }
                    OperationEvent::Finished(result) => {
                        let outcome = match result {
                            Ok(()) => JobOutcome::Succeeded,
                            Err(Error::Cancelled) => JobOutcome::Cancelled,
                            Err(error) => JobOutcome::Failed(error.to_string()),
                        };
                        let snapshot = handle.stats.snapshot();
                        job.final_snapshot = Some(snapshot.clone());
                        job.handle = None;
                        job.state = JobState::Finished {
                            outcome: outcome.clone(),
                            at: Instant::now(),
                        };
                        updates.push(JobUpdate::Finished {
                            id: job.id,
                            kind: job.kind,
                            outcome,
                            snapshot,
                            initiating_pane: job.initiating_pane,
                            delete_paths: job.delete_paths.take(),
                        });
                        break;
                    }
                }
            }
        }
        self.schedule();
        self.trim_history();
        updates
    }

    pub fn answer_conflict(&mut self, id: JobId, choice: ConflictChoice) {
        let Some(job) = self.job_mut(id) else {
            return;
        };
        if let Some(handle) = &job.handle {
            handle.answer_conflict(choice);
            job.state = if choice == ConflictChoice::Cancel {
                JobState::Cancelling
            } else {
                JobState::Running
            };
        }
    }

    pub fn cancel(&mut self, id: JobId) {
        let Some(job) = self.job_mut(id) else {
            return;
        };
        match job.state {
            JobState::Queued(_) => {
                job.request = None;
                let snapshot = job.snapshot();
                let kind = job.kind;
                let initiating_pane = job.initiating_pane;
                let delete_paths = job.delete_paths.take();
                job.state = JobState::Finished {
                    outcome: JobOutcome::Cancelled,
                    at: Instant::now(),
                };
                self.pending_updates.push_back(JobUpdate::Finished {
                    id,
                    kind,
                    outcome: JobOutcome::Cancelled,
                    snapshot,
                    initiating_pane,
                    delete_paths,
                });
            }
            JobState::Running
            | JobState::Paused
            | JobState::WaitingConflict(_)
            | JobState::Cancelling => {
                if let Some(handle) = &job.handle {
                    handle.cancel();
                }
                job.state = JobState::Cancelling;
            }
            JobState::Finished { .. } => {}
        }
        self.schedule();
    }

    pub fn cancel_all(&mut self) {
        let ids = self
            .jobs
            .iter()
            .filter(|job| job.is_active())
            .map(|job| job.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.cancel(id);
        }
    }

    pub fn toggle_pause(&mut self, id: JobId) -> bool {
        let Some(job) = self.job_mut(id) else {
            return false;
        };
        match job.state {
            JobState::Queued(_) | JobState::Running => {
                if let Some(handle) = &job.handle {
                    handle.set_paused(true);
                }
                job.state = JobState::Paused;
                true
            }
            JobState::Paused => {
                if let Some(handle) = &job.handle {
                    handle.set_paused(false);
                    job.state = JobState::Running;
                } else {
                    job.state = JobState::Queued(WaitReason::Capacity);
                }
                self.schedule();
                true
            }
            _ => false,
        }
    }

    pub fn reorder_queued(&mut self, id: JobId, amount: isize) -> bool {
        let Some(index) = self.jobs.iter().position(|job| job.id == id) else {
            return false;
        };
        if self.jobs[index].handle.is_some()
            || !matches!(
                self.jobs[index].state,
                JobState::Queued(_) | JobState::Paused
            )
        {
            return false;
        }
        let candidate = if amount < 0 {
            (0..index).rev().find(|candidate| {
                self.jobs[*candidate].handle.is_none()
                    && matches!(
                        self.jobs[*candidate].state,
                        JobState::Queued(_) | JobState::Paused
                    )
            })
        } else {
            (index + 1..self.jobs.len()).find(|candidate| {
                self.jobs[*candidate].handle.is_none()
                    && matches!(
                        self.jobs[*candidate].state,
                        JobState::Queued(_) | JobState::Paused
                    )
            })
        };
        let Some(candidate) = candidate else {
            return false;
        };
        self.jobs.swap(index, candidate);
        self.schedule();
        true
    }

    pub fn set_bandwidth_limit(&mut self, bytes_per_second: u64) {
        self.bandwidth_limit = bytes_per_second;
        for job in &self.jobs {
            if let Some(handle) = &job.handle {
                handle.set_bandwidth_limit(bytes_per_second);
            }
        }
    }

    pub fn bandwidth_limit(&self) -> u64 {
        self.bandwidth_limit
    }

    pub fn has_active(&self) -> bool {
        self.jobs.iter().any(Job::is_active)
    }

    pub fn job(&self, id: JobId) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn job_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    pub fn visible_background(&self) -> Vec<&Job> {
        let mut visible = self
            .jobs
            .iter()
            .filter(|job| job.launch == LaunchMode::Background && job.is_running())
            .take(MAX_RUNNING)
            .collect::<Vec<_>>();
        if visible.len() < MAX_RUNNING {
            visible.extend(
                self.jobs
                    .iter()
                    .filter(|job| {
                        job.launch == LaunchMode::Background
                            && matches!(job.state, JobState::Queued(_))
                    })
                    .take(MAX_RUNNING - visible.len()),
            );
        }
        visible
    }

    pub fn history(&self) -> Vec<&Job> {
        self.jobs.iter().rev().take(HISTORY_LIMIT).collect()
    }

    pub fn running_count(&self) -> usize {
        self.jobs.iter().filter(|job| job.is_running()).count()
    }

    fn schedule(&mut self) {
        loop {
            if self.running_count() >= MAX_RUNNING {
                for job in &mut self.jobs {
                    if matches!(job.state, JobState::Queued(_)) {
                        job.state = JobState::Queued(WaitReason::Capacity);
                    }
                }
                break;
            }
            let candidate = (0..self.jobs.len()).find(|index| {
                if !matches!(self.jobs[*index].state, JobState::Queued(_)) {
                    return false;
                }
                !self.conflicts_with_running(*index) && !self.conflicts_with_earlier_queued(*index)
            });
            let Some(index) = candidate else {
                for index in 0..self.jobs.len() {
                    if matches!(self.jobs[index].state, JobState::Queued(_)) {
                        self.jobs[index].state = JobState::Queued(WaitReason::Overlap);
                    }
                }
                break;
            };
            let request = self.jobs[index]
                .request
                .take()
                .expect("queued job keeps its request");
            let handle = request.start();
            handle.set_bandwidth_limit(self.bandwidth_limit);
            self.jobs[index].handle = Some(handle);
            self.jobs[index].speedometer = Speedometer::new();
            self.jobs[index].state = JobState::Running;
        }
    }

    fn conflicts_with_running(&self, candidate: usize) -> bool {
        self.jobs
            .iter()
            .enumerate()
            .filter(|(index, job)| *index != candidate && job.is_running())
            .any(|(_, job)| accesses_conflict(&self.jobs[candidate].accesses, &job.accesses))
    }

    fn conflicts_with_earlier_queued(&self, candidate: usize) -> bool {
        self.jobs
            .iter()
            .take(candidate)
            .filter(|job| matches!(job.state, JobState::Queued(_)))
            .any(|job| accesses_conflict(&self.jobs[candidate].accesses, &job.accesses))
    }

    fn trim_history(&mut self) {
        let finished = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Finished { .. }))
            .count();
        let mut remove = finished.saturating_sub(HISTORY_LIMIT);
        while remove > 0 {
            let Some(index) = self
                .jobs
                .iter()
                .position(|job| matches!(job.state, JobState::Finished { .. }))
            else {
                break;
            };
            self.jobs.remove(index);
            remove -= 1;
        }
    }
}

pub(crate) fn accesses_conflict(left: &[PathAccess], right: &[PathAccess]) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            (left.mode == AccessMode::Write || right.mode == AccessMode::Write)
                && paths_overlap(&left.path, &right.path)
        })
    })
}

pub(crate) fn paths_overlap(left: &Location, right: &Location) -> bool {
    left.overlaps(right)
}
