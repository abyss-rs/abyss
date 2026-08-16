use crate::storage::Location;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SyncComparison {
    #[default]
    Metadata,
    Checksum,
    DeltaSignature,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SyncStrategy {
    #[default]
    Mirror,
    UpdateOnly,
    DeltaRsync,
    TwoWay,
}

impl SyncStrategy {
    pub const ALL: [Self; 4] = [
        Self::Mirror,
        Self::UpdateOnly,
        Self::DeltaRsync,
        Self::TwoWay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mirror => "Mirror",
            Self::UpdateOnly => "Update Only",
            Self::DeltaRsync => "Delta (BLAKE3 SIMD)",
            Self::TwoWay => "Two-Way Merge",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SyncPlan {
    pub source: Location,
    pub destination: Location,
    pub comparison: SyncComparison,
    pub strategy: SyncStrategy,
    pub directories: Vec<Location>,
    pub files: Vec<SyncFile>,
    pub deletions: Vec<Location>,
    pub unchanged: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SyncFile {
    pub source: Location,
    pub destination: Location,
    pub relative: String,
    pub reason: SyncReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncReason {
    Missing,
    TypeChanged,
    MetadataChanged,
    ChecksumChanged,
    DeltaPatchable,
    Orphaned,
}

impl SyncReason {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Missing => "+ ADD",
            Self::TypeChanged => "~ TYPE",
            Self::MetadataChanged => "~ UPDATE",
            Self::ChecksumChanged => "~ DIFF",
            Self::DeltaPatchable => "Δ DELTA",
            Self::Orphaned => "- DELETE",
        }
    }
}
