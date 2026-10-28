//! The one port the pool sees for a dry merge (D8): [`InProcessMerger`] inside the worker
//! process and in tests, `WorkerMerger` in the daemon.

use std::path::Path;
use std::time::Instant;

use gitraptor_git::dry_merge::{
    DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome, merge_in_memory,
};

/// Runs the dry merge of `input` over the repo at `repo` (its common dir) within `deadline`.
pub trait DryMerger: Send + Sync {
    /// # Errors
    ///
    /// The merge's own error, or [`DryMergeError::LimitExceeded`] when a cap the merger imposes
    /// (deadline, memory) cut it.
    fn merge(
        &self,
        repo: &Path,
        input: &DryMergeInput,
        limits: &DryMergeLimits,
        deadline: Instant,
    ) -> Result<DryMergeOutcome, DryMergeError>;
}

/// The merge in this process, with no deadline of its own.
#[derive(Debug, Default, Clone, Copy)]
pub struct InProcessMerger;

impl DryMerger for InProcessMerger {
    fn merge(
        &self,
        repo: &Path,
        input: &DryMergeInput,
        limits: &DryMergeLimits,
        _deadline: Instant,
    ) -> Result<DryMergeOutcome, DryMergeError> {
        merge_in_memory(repo, input, limits)
    }
}
