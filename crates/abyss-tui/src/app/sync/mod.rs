mod input;
mod session;

use crate::storage::Location;
use crate::sync::{SyncComparison, SyncPlan, SyncStrategy};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncDirection {
    LeftToRight,
    RightToLeft,
}

impl SyncDirection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LeftToRight => "Left ➔ Right",
            Self::RightToLeft => "Right ➔ Left",
        }
    }

    pub(crate) fn swapped(self) -> Self {
        match self {
            Self::LeftToRight => Self::RightToLeft,
            Self::RightToLeft => Self::LeftToRight,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncFilterMode {
    All,
    ChangesOnly,
}

impl SyncFilterMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All Files",
            Self::ChangesOnly => "Changes Only",
        }
    }

    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::All => Self::ChangesOnly,
            Self::ChangesOnly => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncMenuCategory {
    Strategy,
    Comparison,
    Direction,
    Filter,
    Actions,
}

impl SyncMenuCategory {
    pub(crate) const ALL: [Self; 5] = [
        Self::Strategy,
        Self::Comparison,
        Self::Direction,
        Self::Filter,
        Self::Actions,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Strategy => "Strategy",
            Self::Comparison => "Comparison",
            Self::Direction => "Direction",
            Self::Filter => "Filter",
            Self::Actions => "Actions",
        }
    }

    pub(crate) fn shifted(self, delta: isize) -> Self {
        let index = Self::ALL
            .iter()
            .position(|category| *category == self)
            .unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(Self::ALL.len() as isize) as usize;
        Self::ALL[next]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncMenuAction {
    StrategyMirror,
    StrategyUpdateOnly,
    StrategyDeltaRsync,
    StrategyTwoWay,
    ComparisonMetadata,
    ComparisonChecksum,
    ComparisonDeltaSignature,
    DirectionLeftToRight,
    DirectionRightToLeft,
    DirectionSwap,
    FilterAll,
    FilterChangesOnly,
    ActionCompare,
    ActionExecute,
    ActionBackground,
    ActionHelp,
    ActionExit,
}

impl SyncMenuAction {
    pub(crate) fn for_category(category: SyncMenuCategory) -> &'static [Self] {
        match category {
            SyncMenuCategory::Strategy => &[
                Self::StrategyMirror,
                Self::StrategyUpdateOnly,
                Self::StrategyDeltaRsync,
                Self::StrategyTwoWay,
            ],
            SyncMenuCategory::Comparison => &[
                Self::ComparisonMetadata,
                Self::ComparisonChecksum,
                Self::ComparisonDeltaSignature,
            ],
            SyncMenuCategory::Direction => &[
                Self::DirectionLeftToRight,
                Self::DirectionRightToLeft,
                Self::DirectionSwap,
            ],
            SyncMenuCategory::Filter => &[Self::FilterAll, Self::FilterChangesOnly],
            SyncMenuCategory::Actions => &[
                Self::ActionCompare,
                Self::ActionExecute,
                Self::ActionBackground,
                Self::ActionHelp,
                Self::ActionExit,
            ],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::StrategyMirror => "Mirror (exact sync, deletes orphans)",
            Self::StrategyUpdateOnly => "Update only (copies newer / missing)",
            Self::StrategyDeltaRsync => "Delta sync (block-level BLAKE3 SIMD)",
            Self::StrategyTwoWay => "Two-way merge (bi-directional sync)",
            Self::ComparisonMetadata => "Metadata (size and timestamp)",
            Self::ComparisonChecksum => "Checksum (full BLAKE3 hash)",
            Self::ComparisonDeltaSignature => "Delta signature (block-level BLAKE3)",
            Self::DirectionLeftToRight => "Left ➔ Right",
            Self::DirectionRightToLeft => "Right ➔ Left",
            Self::DirectionSwap => "Swap direction",
            Self::FilterAll => "All files (full preview list)",
            Self::FilterChangesOnly => "Changes only (hide unchanged)",
            Self::ActionCompare => "Re-scan & Compare",
            Self::ActionExecute => "Run Synchronization",
            Self::ActionBackground => "Run in Background",
            Self::ActionHelp => "Help & Shortcuts",
            Self::ActionExit => "Exit Sync Mode",
        }
    }

    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Self::StrategyMirror
            | Self::StrategyUpdateOnly
            | Self::StrategyDeltaRsync
            | Self::StrategyTwoWay => "8",
            Self::ComparisonMetadata
            | Self::ComparisonChecksum
            | Self::ComparisonDeltaSignature => "6",
            Self::DirectionLeftToRight | Self::DirectionRightToLeft | Self::DirectionSwap => "5",
            Self::FilterAll | Self::FilterChangesOnly => "7",
            Self::ActionCompare => "9/r",
            Self::ActionExecute => "3/↵",
            Self::ActionBackground => "b",
            Self::ActionHelp => "1/?",
            Self::ActionExit => "0/Esc",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SyncMenu {
    pub(crate) category: SyncMenuCategory,
    pub(crate) selected: usize,
}

pub(crate) struct SyncSession {
    pub(crate) source: Location,
    pub(crate) destination: Location,
    pub(crate) direction: SyncDirection,
    pub(crate) strategy: SyncStrategy,
    pub(crate) comparison: SyncComparison,
    pub(crate) filter: SyncFilterMode,
    pub(crate) plan: Option<SyncPlan>,
    pub(crate) is_planning: bool,
    pub(crate) selected_index: usize,
    pub(crate) menu: Option<SyncMenu>,
}
