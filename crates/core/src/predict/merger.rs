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

    /// Releases what the merger holds between jobs (the worker process): the pool calls it when
    /// it pauses for power saving or stops. Nothing by default.
    fn close(&self) {}

    /// Releases it when it has been idle long enough at `now`; whether it did. The pool calls it
    /// while its queue is empty. Nothing by default.
    fn close_if_idle(&self, _now: Instant) -> bool {
        false
    }
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
