//! The `utility` pool of the predictor (T007, ADR-CKP-001 § 5, ADR-GRP-015 § 1 and § 2): one
//! thread of [`Pool::Predictor`](gitraptor_api::resources::Pool) that takes the queued dry
//! merges one at a time, by priority, and runs them through a [`DryMerger`]. In power saving it
//! takes nothing (D5).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use gitraptor_git::dry_merge::{DryMergeError, DryMergeOutcome};

use super::merger::DryMerger;
use super::queue::Priority;
use super::{PairKey, Triple};
use crate::power::PowerMode;

/// Jobs taken at a time (§ Valores numéricos).
pub const PREDICT_CONCURRENCY: usize = 1;

/// One dry merge to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The repo's common dir.
    pub repo: PathBuf,
    pub key: PairKey,
    pub triple: Triple,
}

/// The result of a [`Job`], sent back to the predictor.
#[derive(Debug)]
pub struct Done {
    pub key: PairKey,
    pub triple: Triple,
    pub result: Result<DryMergeOutcome, DryMergeError>,
    pub computed_at_utc_ms: i64,
}

/// The pool: its thread and its queue (one job per pair, the newest wins).
pub struct PredictPool {
    _private: (),
}

impl PredictPool {
    /// Starts the thread. Results go to `done`, in completion order.
    ///
    /// # Errors
    ///
    /// The thread could not be spawned.
    pub fn start(
        merger: Arc<dyn DryMerger>,
        power: PowerMode,
        done: Sender<Done>,
    ) -> std::io::Result<Self> {
        let _ = (merger, power, done);
        unimplemented!("T007")
    }

    /// Queues `job`, replacing a queued job of the same pair.
    pub fn submit(&self, job: Job, priority: Priority) {
        let _ = (job, priority);
        unimplemented!("T007")
    }

    /// Drops every queued job of `repo_id` (dormant or retired repo).
    pub fn forget_repo(&self, repo_id: &str) {
        let _ = repo_id;
        unimplemented!("T007")
    }

    /// Jobs waiting.
    pub fn queued(&self) -> usize {
        unimplemented!("T007")
    }

    /// The pool holds its queue because the power-saving mode is on.
    pub fn paused(&self) -> bool {
        unimplemented!("T007")
    }

    /// Stops the thread once its current job ends; the queue is dropped.
    pub fn stop(self) {
        unimplemented!("T007")
    }
}

#[cfg(test)]
mod tests;
