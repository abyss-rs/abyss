pub mod delta;
pub mod execute;
mod local;
mod plan;
mod remote;

#[cfg(test)]
mod tests;

pub use self::execute::execute_sync;
pub use self::local::plan_local;
pub use self::plan::{SyncComparison, SyncFile, SyncPlan, SyncReason, SyncStrategy};
pub use self::remote::plan_locations;
pub use delta::{DeltaError, Rollsum, Signature, apply_delta, compute_delta, compute_signature};
