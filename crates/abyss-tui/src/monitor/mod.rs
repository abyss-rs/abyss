//! In-process system monitor.
//!
//! Backed by `sysinfo`, so Abyss shows what `btop` would without needing it
//! installed — and without leaving the file manager.

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use sysinfo::{Disks, MemoryRefreshKind, ProcessRefreshKind, RefreshKind, System};

/// Sampling interval. CPU percentages need a gap between samples to mean
/// anything, and anything faster just burns power.
const REFRESH: Duration = Duration::from_millis(1_000);

/// How many processes the table shows.
pub(crate) const TOP_PROCESSES: usize = 12;

#[derive(Clone, Debug)]
pub(crate) struct ProcessRow {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) cpu: f32,
    pub(crate) memory: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct DiskRow {
    pub(crate) name: String,
    pub(crate) used: u64,
    pub(crate) total: u64,
}

/// A live view of the machine, refreshed on a timer.
pub(crate) struct Monitor {
    system: System,
    disks: Disks,
    last_refresh: Instant,
    pub(crate) cpu_per_core: Vec<f32>,
    pub(crate) cpu_total: f32,
    pub(crate) memory_used: u64,
    pub(crate) memory_total: u64,
    pub(crate) swap_used: u64,
    pub(crate) swap_total: u64,
    pub(crate) processes: Vec<ProcessRow>,
    pub(crate) disk_rows: Vec<DiskRow>,
}

impl Monitor {
    pub(crate) fn new() -> Self {
        let mut monitor = Self {
            system: System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage())
                    .with_memory(MemoryRefreshKind::everything())
                    .with_processes(ProcessRefreshKind::nothing().with_cpu().with_memory()),
            ),
            disks: Disks::new_with_refreshed_list(),
            // Backdate so the first tick samples immediately.
            last_refresh: Instant::now() - REFRESH,
            cpu_per_core: Vec::new(),
            cpu_total: 0.0,
            memory_used: 0,
            memory_total: 0,
            swap_used: 0,
            swap_total: 0,
            processes: Vec::new(),
            disk_rows: Vec::new(),
        };
        monitor.tick();
        monitor
    }

    /// Resample if enough time has passed.
    ///
    /// Returns `true` when the figures changed and the frame needs redrawing.
    pub(crate) fn tick(&mut self) -> bool {
        if self.last_refresh.elapsed() < REFRESH {
            return false;
        }
        self.last_refresh = Instant::now();

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.disks.refresh(true);

        self.cpu_per_core = self
            .system
            .cpus()
            .iter()
            .map(sysinfo::Cpu::cpu_usage)
            .collect();
        self.cpu_total = self.system.global_cpu_usage();
        self.memory_used = self.system.used_memory();
        self.memory_total = self.system.total_memory();
        self.swap_used = self.system.used_swap();
        self.swap_total = self.system.total_swap();

        let mut processes: Vec<ProcessRow> = self
            .system
            .processes()
            .values()
            .map(|process| ProcessRow {
                pid: process.pid().as_u32(),
                name: process.name().to_string_lossy().into_owned(),
                cpu: process.cpu_usage(),
                memory: process.memory(),
            })
            .collect();
        // Busiest first, as every process viewer does.
        processes.sort_by(|left, right| {
            right
                .cpu
                .total_cmp(&left.cpu)
                .then_with(|| right.memory.cmp(&left.memory))
        });
        processes.truncate(TOP_PROCESSES);
        self.processes = processes;

        self.disk_rows = self
            .disks
            .list()
            .iter()
            .map(|disk| {
                let total = disk.total_space();
                DiskRow {
                    name: disk.mount_point().to_string_lossy().into_owned(),
                    used: total.saturating_sub(disk.available_space()),
                    total,
                }
            })
            .collect();
        true
    }
}

/// Fraction used, guarding against a total of zero.
pub(crate) fn ratio(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (used as f64 / total as f64).clamp(0.0, 1.0)
}
