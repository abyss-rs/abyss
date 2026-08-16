use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OperationPhase {
    #[default]
    Scanning,
    Copying,
    Compressing,
    Hashing,
    WritingHashes,
    VerifyingHashes,
    Finalizing,
    Extracting,
    Testing,
    Moving,
    Deleting,
}

#[derive(Debug, Default)]
pub struct CopyStats {
    pub logical_done: AtomicU64,
    pub physical_done: AtomicU64,
    pub current_copied: AtomicU64,
    pub current_wire: AtomicU64,
    pub current_size: AtomicU64,
    pub objects_done: AtomicU64,
    pub cloned_bytes: AtomicU64,
    pub linked_bytes: AtomicU64,
    pub skipped_objects: AtomicU64,
    pub scanned_objects: AtomicU64,
    pub total_bytes: AtomicU64,
    pub total_objects: AtomicU64,
    phase: Mutex<OperationPhase>,
    current_path: Mutex<PathBuf>,
    transfer_control: TransferControl,
}

#[derive(Debug, Default)]
struct TransferControl {
    paused: AtomicBool,
    bytes_per_second: AtomicU64,
    throttle: Mutex<ThrottleState>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct ThrottleState {
    started: Option<Instant>,
    bytes: u64,
}

impl CopyStats {
    pub fn set_paused(&self, paused: bool) {
        self.transfer_control
            .paused
            .store(paused, Ordering::Release);
        if !paused {
            self.transfer_control.wake.notify_all();
        }
    }

    pub fn set_bandwidth_limit(&self, bytes_per_second: u64) {
        self.transfer_control
            .bytes_per_second
            .store(bytes_per_second, Ordering::Release);
        let mut throttle = self
            .transfer_control
            .throttle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *throttle = ThrottleState::default();
        self.transfer_control.wake.notify_all();
    }

    pub fn bandwidth_limit(&self) -> u64 {
        self.transfer_control
            .bytes_per_second
            .load(Ordering::Acquire)
    }

    /// Cooperatively pauses and rate-limits a transfer. Returns `false` when
    /// cancellation was requested while waiting.
    pub fn wait_for_transfer(&self, cancelled: &AtomicBool, bytes: u64) -> bool {
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        let paused = self.transfer_control.paused.load(Ordering::Acquire);
        let limit = self.bandwidth_limit();
        if !paused && limit == 0 {
            return true;
        }

        let mut throttle = self
            .transfer_control
            .throttle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while self.transfer_control.paused.load(Ordering::Acquire) {
            if cancelled.load(Ordering::Relaxed) {
                return false;
            }
            let (next, _) = self
                .transfer_control
                .wake
                .wait_timeout(throttle, Duration::from_millis(100))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            throttle = next;
        }
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        if limit == 0 {
            return true;
        }
        let started = *throttle.started.get_or_insert_with(Instant::now);
        throttle.bytes = throttle.bytes.saturating_add(bytes);
        let expected = Duration::from_secs_f64(throttle.bytes as f64 / limit as f64);
        drop(throttle);
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return false;
            }
            let elapsed = started.elapsed();
            if elapsed >= expected {
                return true;
            }
            std::thread::sleep(
                expected
                    .saturating_sub(elapsed)
                    .min(Duration::from_millis(100)),
            );
        }
    }

    pub fn reset(&self) {
        self.logical_done.store(0, Ordering::Relaxed);
        self.physical_done.store(0, Ordering::Relaxed);
        self.current_copied.store(0, Ordering::Relaxed);
        self.current_wire.store(0, Ordering::Relaxed);
        self.current_size.store(0, Ordering::Relaxed);
        self.objects_done.store(0, Ordering::Relaxed);
        self.cloned_bytes.store(0, Ordering::Relaxed);
        self.linked_bytes.store(0, Ordering::Relaxed);
        self.skipped_objects.store(0, Ordering::Relaxed);
        self.scanned_objects.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        self.total_objects.store(0, Ordering::Relaxed);
        self.set_phase(OperationPhase::Scanning);
        self.set_path(Path::new(""));
        let limit = self.bandwidth_limit();
        self.set_bandwidth_limit(limit);
    }

    pub fn set_phase(&self, phase: OperationPhase) {
        *self
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = phase;
    }

    pub fn set_totals(&self, objects: u64, bytes: u64) {
        self.total_objects.store(objects, Ordering::Relaxed);
        self.total_bytes.store(bytes, Ordering::Relaxed);
    }

    pub fn observe_scan(&self, path: &Path) {
        self.scanned_objects.fetch_add(1, Ordering::Relaxed);
        self.set_path(path);
    }

    pub fn begin_file(&self, path: &Path, size: u64) {
        self.current_copied.store(0, Ordering::Relaxed);
        self.current_size.store(size, Ordering::Relaxed);
        self.set_path(path);
    }

    pub fn observe_transfer(&self, path: &Path) {
        self.set_path(path);
    }

    pub fn observe_wire(&self, bytes: u64) {
        self.current_wire.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn set_wire(&self, bytes: u64) {
        self.current_wire.store(bytes, Ordering::Relaxed);
    }

    pub fn complete_file(&self, size: u64, physical: u64, cloned: bool, linked: bool) {
        self.current_size.store(0, Ordering::Relaxed);
        self.current_copied.store(0, Ordering::Relaxed);
        self.physical_done.fetch_add(physical, Ordering::Relaxed);
        self.logical_done.fetch_add(size, Ordering::Relaxed);
        self.objects_done.fetch_add(1, Ordering::Relaxed);
        if cloned {
            self.cloned_bytes.fetch_add(size, Ordering::Relaxed);
        }
        if linked {
            self.linked_bytes.fetch_add(size, Ordering::Relaxed);
        }
    }

    pub fn begin_bulk_transfer(&self, logical: u64) {
        self.current_copied.store(0, Ordering::Relaxed);
        self.current_size.store(logical, Ordering::Relaxed);
        self.current_wire.store(0, Ordering::Relaxed);
    }

    pub fn complete_bulk_files(
        &self,
        files: u64,
        logical: u64,
        physical: u64,
        cloned: bool,
        streamed: bool,
    ) {
        subtract_saturating(&self.current_size, logical);
        if streamed {
            subtract_saturating(&self.current_copied, logical);
            subtract_saturating(&self.current_wire, physical);
        }
        self.physical_done.fetch_add(physical, Ordering::Relaxed);
        self.logical_done.fetch_add(logical, Ordering::Relaxed);
        self.objects_done.fetch_add(files, Ordering::Relaxed);
        if cloned {
            self.cloned_bytes.fetch_add(logical, Ordering::Relaxed);
        }
    }

    pub fn complete_object(&self, path: &Path) {
        self.set_path(path);
        self.objects_done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn skip_object(&self, path: &Path, logical_size: u64) {
        self.set_path(path);
        self.logical_done.fetch_add(logical_size, Ordering::Relaxed);
        self.objects_done.fetch_add(1, Ordering::Relaxed);
        self.skipped_objects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        // Read locks before calling helpers that may lock the same mutexes.
        // Struct-field temporaries live until the end of the statement, so
        // nesting `phase.lock()` inside `physical_total()` would deadlock.
        let phase = *self
            .phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current_path = self
            .current_path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        ProgressSnapshot {
            phase,
            current_path,
            logical_done: self.logical_position(),
            physical_done: self.physical_total(phase),
            objects_done: self.objects_done.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            total_objects: self.total_objects.load(Ordering::Relaxed),
            scanned_objects: self.scanned_objects.load(Ordering::Relaxed),
            cloned_bytes: self.cloned_bytes.load(Ordering::Relaxed),
            linked_bytes: self.linked_bytes.load(Ordering::Relaxed),
            skipped_objects: self.skipped_objects.load(Ordering::Relaxed),
        }
    }

    fn set_path(&self, path: &Path) {
        *self
            .current_path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = path.to_owned();
    }

    fn physical_total(&self, phase: OperationPhase) -> u64 {
        let physical = self.physical_done.load(Ordering::Relaxed);
        let wire = self.current_wire.load(Ordering::Relaxed);
        // During compression, never fall back to input bytes as "physical".
        if matches!(
            phase,
            OperationPhase::Compressing | OperationPhase::Finalizing
        ) {
            return physical.saturating_add(wire);
        }
        physical.saturating_add(if wire == 0 {
            self.current_copied.load(Ordering::Relaxed)
        } else {
            wire
        })
    }

    fn logical_position(&self) -> u64 {
        let in_file = self
            .current_copied
            .load(Ordering::Relaxed)
            .min(self.current_size.load(Ordering::Relaxed));
        self.logical_done
            .load(Ordering::Relaxed)
            .saturating_add(in_file)
            .min(self.total_bytes.load(Ordering::Relaxed))
    }
}

fn subtract_saturating(value: &AtomicU64, amount: u64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_sub(amount))
    });
}

#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub phase: OperationPhase,
    pub current_path: PathBuf,
    pub logical_done: u64,
    pub physical_done: u64,
    pub objects_done: u64,
    pub total_bytes: u64,
    pub total_objects: u64,
    pub scanned_objects: u64,
    pub cloned_bytes: u64,
    pub linked_bytes: u64,
    pub skipped_objects: u64,
}

impl ProgressSnapshot {
    pub fn ratio(&self) -> f64 {
        if self.total_bytes > 0 {
            self.logical_done as f64 / self.total_bytes as f64
        } else if self.total_objects > 0 {
            self.objects_done as f64 / self.total_objects as f64
        } else {
            0.0
        }
        .clamp(0.0, 1.0)
    }
}

pub struct Speedometer {
    started: Instant,
    samples: VecDeque<(Instant, u64)>,
}

impl Default for Speedometer {
    fn default() -> Self {
        Self::new()
    }
}

impl Speedometer {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            samples: VecDeque::new(),
        }
    }

    pub fn update(&mut self, physical_bytes: u64) -> SpeedSnapshot {
        let now = Instant::now();
        self.samples.push_back((now, physical_bytes));
        while self
            .samples
            .front()
            .is_some_and(|(time, _)| now.duration_since(*time) > Duration::from_secs(3))
        {
            self.samples.pop_front();
        }

        let current = self
            .samples
            .front()
            .map(|(time, bytes)| {
                physical_bytes.saturating_sub(*bytes) as f64
                    / now.duration_since(*time).as_secs_f64().max(0.001)
            })
            .unwrap_or(0.0);
        let elapsed = now.duration_since(self.started);
        let average = physical_bytes as f64 / elapsed.as_secs_f64().max(0.001);
        SpeedSnapshot {
            current: current as u64,
            average: average as u64,
            elapsed,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpeedSnapshot {
    pub current: u64,
    pub average: u64,
    pub elapsed: Duration,
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::{CopyStats, human_bytes};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn formats_byte_units() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn bulk_lanes_complete_without_double_counting_in_flight_bytes() {
        let stats = CopyStats::default();
        stats.set_totals(3, 300);
        stats.begin_bulk_transfer(300);
        stats.current_copied.store(300, Ordering::Relaxed);
        stats.current_wire.store(100, Ordering::Relaxed);

        stats.complete_bulk_files(1, 100, 40, false, true);
        let partial = stats.snapshot();
        assert_eq!(partial.logical_done, 300);
        assert_eq!(partial.physical_done, 100);

        stats.complete_bulk_files(2, 200, 60, false, true);
        let complete = stats.snapshot();
        assert_eq!(complete.logical_done, 300);
        assert_eq!(complete.physical_done, 100);
        assert_eq!(complete.objects_done, 3);
    }

    #[test]
    fn compressing_snapshot_uses_wire_bytes_without_deadlocking() {
        let stats = CopyStats::default();
        stats.set_phase(super::OperationPhase::Compressing);
        stats.set_totals(1, 1000);
        stats.logical_done.store(400, Ordering::Relaxed);
        stats.current_copied.store(200, Ordering::Relaxed);
        stats.current_size.store(200, Ordering::Relaxed);
        stats.current_wire.store(50, Ordering::Relaxed);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.phase, super::OperationPhase::Compressing);
        assert_eq!(snapshot.logical_done, 600);
        assert_eq!(snapshot.physical_done, 50);
    }

    #[test]
    fn paused_transfer_waits_until_resume() {
        let stats = Arc::new(CopyStats::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        stats.set_paused(true);
        let worker_stats = Arc::clone(&stats);
        let worker_cancelled = Arc::clone(&cancelled);
        let worker =
            std::thread::spawn(move || worker_stats.wait_for_transfer(&worker_cancelled, 1));
        std::thread::sleep(Duration::from_millis(30));
        assert!(!worker.is_finished());
        stats.set_paused(false);
        assert!(worker.join().unwrap());
    }

    #[test]
    fn bandwidth_limit_delays_bytes_to_the_requested_average() {
        let stats = CopyStats::default();
        let cancelled = AtomicBool::new(false);
        stats.set_bandwidth_limit(1024 * 1024);
        let started = Instant::now();
        assert!(stats.wait_for_transfer(&cancelled, 128 * 1024));
        assert!(started.elapsed() >= Duration::from_millis(100));
    }
}
