//! TS-TMC-005 signature stub: the undo's wait for the capture that gives its base point B.
//! Only the signatures the criterion tests use; the expert replaces this file whole.

use std::time::Duration;

/// Longest wait of an undo for the capture of its base point (TS-TMC-005, ⚠️ ASSUMPTION).
pub const BASE_WAIT: Duration = Duration::from_secs(2);

/// What the continuous capture published, per repo (stub).
#[derive(Debug, Default)]
pub struct CaptureSeals {}

/// How long a wait for bases lasted (stub).
#[derive(Debug, Clone, Copy)]
pub struct Waited {
    pub waited: Duration,
}

impl CaptureSeals {
    /// Waits, up to `limit`, until every `seqs` of `repo_id` is settled (stub).
    pub fn await_bases(&self, _repo_id: &str, _seqs: &[i64], _limit: Duration) -> Waited {
        Waited {
            waited: Duration::ZERO,
        }
    }
}

#[cfg(test)]
#[path = "base_wait_tests.rs"]
mod tests;
