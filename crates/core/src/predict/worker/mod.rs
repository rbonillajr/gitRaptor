//! The worker process of the predictor (T006, ADR-CKP-001 § 1 and § 5, Enmienda 2026-10-20;
//! ADR-GRP-009, Enmienda 2026-10-20): the daemon's own binary with the hidden subcommand
//! `predict-worker`, the only process `predict` launches. [`WorkerMerger`] is its
//! [`DryMerger`] in the daemon: fixed argv, no shell, an environment built from scratch,
//! length-prefixed frames with a cap, a wall-clock deadline per job and the child's resident
//! memory read every [`WORKER_RSS_POLL`]; past either, the child is killed and relaunched with
//! the next job.

pub mod serve;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_git::dry_merge::{DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome};

use super::merger::DryMerger;

/// The hidden subcommand of the daemon's binary that runs [`serve::serve`].
pub const WORKER_SUBCOMMAND: &str = "predict-worker";
/// Wall-clock time of one job, imposed by the daemon.
pub const PAIR_DEADLINE: Duration = Duration::from_secs(2);
/// Resident memory of the worker past which it is killed (a kill threshold, not a budget).
pub const JOB_MEMORY_CAP: u64 = 512 * 1024 * 1024;
/// How often the daemon reads the worker's resident memory.
pub const WORKER_RSS_POLL: Duration = Duration::from_millis(20);
/// Idle time after which the worker is closed (⚠️ ASSUMPTION of the spec).
pub const WORKER_IDLE_EXIT: Duration = Duration::from_secs(60);
/// Largest frame either side accepts.
pub const MAX_WORKER_FRAME: usize = 4 * 1024 * 1024;
/// `rewrites_limit` of the second job of a pair with modify/delete or rename conflicts.
pub const REWRITES_LIMIT_ESCALATED: usize = 100_000;

/// The caps the daemon imposes on the worker. Production uses [`WorkerLimits::default`]; tests
/// tighten them to make a cut deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerLimits {
    pub deadline: Duration,
    /// Deadline of the escalated second job.
    pub escalated_deadline: Duration,
    pub memory_cap: u64,
    pub rss_poll: Duration,
    pub idle_exit: Duration,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self {
            deadline: PAIR_DEADLINE,
            escalated_deadline: PAIR_DEADLINE,
            memory_cap: JOB_MEMORY_CAP,
            rss_poll: WORKER_RSS_POLL,
            idle_exit: WORKER_IDLE_EXIT,
        }
    }
}

/// What to launch: an absolute program, resolved once (`current_exe` in the daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub program: PathBuf,
    pub limits: WorkerLimits,
}

/// The daemon's [`DryMerger`]: one long-lived worker, launched with the first job.
pub struct WorkerMerger {
    _private: (),
}

impl WorkerMerger {
    pub fn new(config: WorkerConfig) -> Self {
        let _ = config;
        unimplemented!("T006")
    }

    /// The pid of the live worker, if any.
    pub fn worker_pid(&self) -> Option<u32> {
        unimplemented!("T006")
    }

    /// Workers launched so far.
    pub fn launches(&self) -> u64 {
        unimplemented!("T006")
    }

    /// Closes the worker (EOF on its stdin) if it has been idle for `idle_exit`. Returns
    /// whether it closed it.
    pub fn close_if_idle(&self, now: Instant) -> bool {
        let _ = now;
        unimplemented!("T006")
    }

    /// Closes the worker now (power saving, dormant repo, stop).
    pub fn close(&self) {
        unimplemented!("T006")
    }
}

impl DryMerger for WorkerMerger {
    fn merge(
        &self,
        repo: &Path,
        input: &DryMergeInput,
        limits: &DryMergeLimits,
        deadline: Instant,
    ) -> Result<DryMergeOutcome, DryMergeError> {
        let _ = (repo, input, limits, deadline);
        unimplemented!("T006")
    }
}

#[cfg(test)]
mod tests;
