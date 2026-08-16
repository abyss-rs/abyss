use std::fs;

use crate::app::dialogs::Modal;
use crate::app::state::App;
use crate::app::sync::{SyncDirection, SyncFilterMode, SyncMenuAction, SyncSession};
use crate::inspect::InspectDialog;
use crate::jobs::{JobRequest, LaunchMode};
use crate::progress::human_bytes;
use crate::storage::Location;
use crate::sync::SyncPlan;
use crate::tasks::SyncLoad;
use crate::ui::ActionButton;

impl App {
    pub(crate) fn open_sync_session(&mut self) {
        if self.sync.is_some() {
            return;
        }
        if self.panes[0].is_archive() || self.panes[1].is_archive() {
            self.set_status("Sync requires two filesystem panes");
            return;
        }
        let source = self.panes[0].location.clone();
        let destination = self.panes[1].location.clone();
        self.app_menu = None;
        self.sort_menu = None;
        self.modal = None;
        let session = SyncSession {
            source: source.clone(),
            destination: destination.clone(),
            direction: SyncDirection::LeftToRight,
            strategy: crate::sync::SyncStrategy::Mirror,
            comparison: crate::sync::SyncComparison::Metadata,
            filter: SyncFilterMode::All,
            plan: None,
            is_planning: true,
            selected_index: 0,
            menu: None,
        };
        self.sync = Some(session);
        self.sync_load = Some(SyncLoad::start(
            self.browser.storage(),
            source,
            destination,
            crate::sync::SyncComparison::Metadata,
            crate::sync::SyncStrategy::Mirror,
        ));
        self.set_status("Entered Sync Mode (3 to Run, 0/Esc to Exit)");
    }

    pub(crate) fn leave_sync_mode(&mut self) {
        self.sync = None;
        self.sync_load = None;
        self.refresh_all();
        self.set_status("Left Sync Mode");
    }

    pub(crate) fn swap_sync_direction(&mut self) {
        let label = {
            let Some(sync) = self.sync.as_mut() else {
                return;
            };
            sync.direction = sync.direction.swapped();
            let (src, dst) = match sync.direction {
                SyncDirection::LeftToRight => (
                    self.panes[0].location.clone(),
                    self.panes[1].location.clone(),
                ),
                SyncDirection::RightToLeft => (
                    self.panes[1].location.clone(),
                    self.panes[0].location.clone(),
                ),
            };
            sync.source = src.clone();
            sync.destination = dst.clone();
            sync.is_planning = true;
            let comparison = sync.comparison;
            let strategy = sync.strategy;
            self.sync_load = Some(SyncLoad::start(
                self.browser.storage(),
                src,
                dst,
                comparison,
                strategy,
            ));
            sync.direction.label()
        };
        self.set_status(format!("Direction: {label}"));
    }

    pub(crate) fn cycle_sync_comparison(&mut self) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        sync.comparison = match sync.comparison {
            crate::sync::SyncComparison::Metadata => crate::sync::SyncComparison::Checksum,
            crate::sync::SyncComparison::Checksum => crate::sync::SyncComparison::DeltaSignature,
            crate::sync::SyncComparison::DeltaSignature => crate::sync::SyncComparison::Metadata,
        };
        let label = match sync.comparison {
            crate::sync::SyncComparison::Metadata => "Metadata (Size + Time)",
            crate::sync::SyncComparison::Checksum => "Fast Hash Checksum",
            crate::sync::SyncComparison::DeltaSignature => "Delta Signature (BLAKE3 SIMD)",
        };
        self.rescan_sync_plan();
        self.set_status(format!("Comparison: {label}"));
    }

    pub(crate) fn cycle_sync_strategy(&mut self) {
        let label = {
            let Some(sync) = self.sync.as_mut() else {
                return;
            };
            sync.strategy = match sync.strategy {
                crate::sync::SyncStrategy::Mirror => crate::sync::SyncStrategy::UpdateOnly,
                crate::sync::SyncStrategy::UpdateOnly => crate::sync::SyncStrategy::DeltaRsync,
                crate::sync::SyncStrategy::DeltaRsync => crate::sync::SyncStrategy::TwoWay,
                crate::sync::SyncStrategy::TwoWay => crate::sync::SyncStrategy::Mirror,
            };
            sync.strategy.label()
        };
        self.rescan_sync_plan();
        self.set_status(format!("Strategy: {label}"));
    }

    pub(crate) fn rescan_sync_plan(&mut self) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        sync.is_planning = true;
        let source = sync.source.clone();
        let destination = sync.destination.clone();
        let comparison = sync.comparison;
        let strategy = sync.strategy;
        self.sync_load = Some(SyncLoad::start(
            self.browser.storage(),
            source,
            destination,
            comparison,
            strategy,
        ));
        self.set_status("Scanning & comparing directories…");
    }

    pub(crate) fn run_sync_session(&mut self, background: bool) {
        let Some(plan) = self.sync.as_ref().and_then(|s| s.plan.clone()) else {
            self.set_status("No sync plan ready yet (still scanning…)");
            return;
        };
        let count = plan.files.len();
        if count == 0 {
            self.set_status("Directories are already in sync");
            return;
        }
        self.execute_sync_plan_session(plan, background);
        self.leave_sync_mode();
    }

    pub(crate) fn execute_sync_plan_session(&mut self, plan: SyncPlan, background: bool) {
        for directory in &plan.directories {
            let result = match directory {
                Location::Local(path) => {
                    fs::create_dir_all(path).map_err(|error| error.to_string())
                }
                Location::Remote(remote) => self
                    .browser
                    .storage()
                    .backend(remote)
                    .and_then(|backend| {
                        if backend.capabilities().create_dir {
                            self.browser
                                .storage()
                                .block_on(backend.create_dir(&remote.path))
                        } else {
                            Ok(())
                        }
                    })
                    .map_err(|error| error.to_string()),
            };
            if let Err(error) = result {
                self.show_error("Sync", format!("create {}: {error}", directory.display()));
                return;
            }
        }
        let count = plan.files.len();
        let launch_mode = if background {
            LaunchMode::Background
        } else {
            LaunchMode::Foreground
        };
        for file in plan.files {
            self.jobs.submit(
                JobRequest::SyncFile {
                    storage: self.browser.storage(),
                    source: file.source,
                    destination: file.destination,
                },
                launch_mode,
                self.active,
                None,
            );
        }
        self.set_status(format!(
            "Started sync for {count} file(s), {} total",
            human_bytes(plan.bytes)
        ));
    }

    pub(crate) fn inspect_sync_item(&mut self) {
        let Some(sync) = self.sync.as_ref() else {
            return;
        };
        let Some(plan) = sync.plan.as_ref() else {
            return;
        };
        let files: Vec<&crate::sync::SyncFile> = match sync.filter {
            SyncFilterMode::All => plan.files.iter().collect(),
            SyncFilterMode::ChangesOnly => plan.files.iter().collect(),
        };
        if files.is_empty() {
            return;
        };
        let selected = sync.selected_index.min(files.len().saturating_sub(1));
        let file = files[selected];
        self.modal = Some(Modal::Inspect(InspectDialog::from_location(&file.source)));
    }

    pub(crate) fn perform_sync_menu_action(&mut self, action: SyncMenuAction) {
        match action {
            SyncMenuAction::StrategyMirror => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.strategy = crate::sync::SyncStrategy::Mirror;
                    self.set_status("Strategy: Mirror (Delete Orphaned Files)");
                }
            }
            SyncMenuAction::StrategyUpdateOnly => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.strategy = crate::sync::SyncStrategy::UpdateOnly;
                    self.set_status("Strategy: Update / Add Only");
                }
            }
            SyncMenuAction::StrategyDeltaRsync => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.strategy = crate::sync::SyncStrategy::DeltaRsync;
                    self.set_status("Strategy: Delta (BLAKE3 SIMD)");
                }
            }
            SyncMenuAction::StrategyTwoWay => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.strategy = crate::sync::SyncStrategy::TwoWay;
                    self.set_status("Strategy: Two-Way Merge");
                }
            }
            SyncMenuAction::ComparisonMetadata => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.comparison = crate::sync::SyncComparison::Metadata;
                }
                self.rescan_sync_plan();
                self.set_status("Comparison: Metadata (Size + Modified Time)");
            }
            SyncMenuAction::ComparisonChecksum => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.comparison = crate::sync::SyncComparison::Checksum;
                }
                self.rescan_sync_plan();
                self.set_status("Comparison: Fast Hash Checksum");
            }
            SyncMenuAction::ComparisonDeltaSignature => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.comparison = crate::sync::SyncComparison::DeltaSignature;
                }
                self.rescan_sync_plan();
                self.set_status("Comparison: Block Delta Signatures (BLAKE3 SIMD)");
            }
            SyncMenuAction::DirectionLeftToRight => {
                if let Some(sync) = self.sync.as_mut()
                    && sync.direction != SyncDirection::LeftToRight
                {
                    self.swap_sync_direction();
                }
            }
            SyncMenuAction::DirectionRightToLeft => {
                if let Some(sync) = self.sync.as_mut()
                    && sync.direction != SyncDirection::RightToLeft
                {
                    self.swap_sync_direction();
                }
            }
            SyncMenuAction::DirectionSwap => {
                self.swap_sync_direction();
            }
            SyncMenuAction::FilterAll => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.filter = SyncFilterMode::All;
                    self.set_status("Filter: Show All Files");
                }
            }
            SyncMenuAction::FilterChangesOnly => {
                if let Some(sync) = self.sync.as_mut() {
                    sync.filter = SyncFilterMode::ChangesOnly;
                    self.set_status("Filter: Show Changes Only");
                }
            }
            SyncMenuAction::ActionCompare => {
                self.rescan_sync_plan();
            }
            SyncMenuAction::ActionExecute => {
                self.run_sync_session(false);
            }
            SyncMenuAction::ActionBackground => {
                self.run_sync_session(true);
            }
            SyncMenuAction::ActionHelp => {
                self.trigger(ActionButton::Help);
            }
            SyncMenuAction::ActionExit => {
                self.leave_sync_mode();
            }
        }
    }

    pub(crate) fn execute_sync_plan(&mut self, plan: SyncPlan) {
        for directory in &plan.directories {
            let result = match directory {
                Location::Local(path) => {
                    fs::create_dir_all(path).map_err(|error| error.to_string())
                }
                Location::Remote(remote) => self
                    .browser
                    .storage()
                    .backend(remote)
                    .and_then(|backend| {
                        if backend.capabilities().create_dir {
                            self.browser
                                .storage()
                                .block_on(backend.create_dir(&remote.path))
                        } else {
                            Ok(())
                        }
                    })
                    .map_err(|error| error.to_string()),
            };
            if let Err(error) = result {
                self.show_error(
                    "Differential sync",
                    format!("create {}: {error}", directory.display()),
                );
                return;
            }
        }
        let count = plan.files.len();
        if count == 0 {
            self.set_status("Directories are already synchronized");
            self.refresh_all();
            return;
        }
        for file in plan.files {
            self.jobs.submit(
                JobRequest::SyncFile {
                    storage: self.browser.storage(),
                    source: file.source,
                    destination: file.destination,
                },
                LaunchMode::Background,
                self.active,
                None,
            );
        }
        self.set_status(format!(
            "Queued {count} differential sync file(s), {} total",
            human_bytes(plan.bytes)
        ));
    }
}
