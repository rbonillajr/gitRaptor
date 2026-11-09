//! The undo's wait for the capture that gives the base point B of an agent's change.
//!
//! The continuous capture publishes here, per repo, every Git event it was handed and every one
//! whose capture ended (sealed or given up). When an undo finds no B for an agent's change whose
//! capture is still pending, it waits on that signal with its own deadline ([`BASE_WAIT`]) and
//! then reads the points again. The wait holds neither the oplog lock nor the store writer: the
//! capture needs both to seal the point.
//!
//! Events are keyed by their engine sequence number, unique per repo, so the planner never has
//! to match the worktree path the event was signalled with.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::continuous::CaptureDeps;
use crate::daemon::{Field, Logger};

/// Longest wait of an undo for the capture of its base point. Outside the two phases' budget of
/// the overlap check, which starts once B exists, so waiting never shortens the check.
/// ⚠️ ASSUMPTION: the engine's calm (1 s) plus one capture and a few retries; measured on the
/// end-to-end raw-reset undo.
pub const BASE_WAIT: Duration = Duration::from_secs(2);

/// Longest wait for the signal of an event the undo already sees: the engine persists an event
/// a moment before it hands it to the capture.
const HANDOFF: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
struct RepoSeals {
    /// Highest event sequence number handed to the capture.
    signalled: i64,
    /// Events handed to the capture whose capture has not ended yet, with their worktree.
    outstanding: BTreeMap<i64, PathBuf>,
}

#[derive(Debug, Default)]
struct State {
    /// The capture runs and takes Git events.
    running: bool,
    repos: HashMap<String, RepoSeals>,
}

/// What the continuous capture published, per repo: the events it still has to capture.
#[derive(Debug, Default)]
pub struct CaptureSeals {
    state: Mutex<State>,
    changed: Condvar,
}

/// How a wait for bases ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// No capture was pending for any of the events: nothing was waited for.
    NotPending,
    /// Every pending capture ended (sealed or given up).
    Settled,
    /// The deadline passed with a capture still pending.
    Expired,
}

/// How long a wait for bases lasted and how it ended.
#[derive(Debug, Clone, Copy)]
pub struct Waited {
    pub waited: Duration,
    pub outcome: WaitOutcome,
}

impl Waited {
    /// Logs the wait as `tm_base_wait`: `found` is whether every base was there after it. Only the
    /// repo's id, never a path or a branch.
    pub fn log(&self, logger: &Logger, repo_id: &str, found: bool) {
        let result = match self.outcome {
            WaitOutcome::NotPending => "not-pending",
            WaitOutcome::Expired => "expired",
            WaitOutcome::Settled if found => "found",
            WaitOutcome::Settled => "abandoned",
        };
        let ms = i64::try_from(self.waited.as_millis()).unwrap_or(i64::MAX);
        logger.info(
            "tm_base_wait",
            &[
                ("repo", Field::id(repo_id)),
                ("base_wait_ms", ms.into()),
                ("result", result.into()),
            ],
        );
    }
}

impl CaptureSeals {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The capture started; with `enabled` false it takes no Git event.
    pub(crate) fn started(&self, enabled: bool) {
        self.state().running = enabled;
        self.changed.notify_all();
    }

    /// The capture stopped: nothing is pending any more.
    pub(crate) fn stopped(&self) {
        let mut st = self.state();
        st.running = false;
        st.repos.clear();
        drop(st);
        self.changed.notify_all();
    }

    /// The Git event `seq` of `worktree` was handed to the capture.
    pub(crate) fn signal(&self, repo_id: &str, worktree: &Path, seq: i64) {
        let mut st = self.state();
        if !st.running {
            return;
        }
        let repo = st.repos.entry(repo_id.to_owned()).or_default();
        repo.signalled = repo.signalled.max(seq);
        repo.outstanding.insert(seq, worktree.to_path_buf());
        drop(st);
        self.changed.notify_all();
    }

    /// The capture of `worktree` that covered its events up to `upto` ended.
    pub(crate) fn settle(&self, repo_id: &str, worktree: &Path, upto: i64) {
        let mut st = self.state();
        if let Some(repo) = st.repos.get_mut(repo_id) {
            repo.outstanding
                .retain(|seq, w| *seq > upto || w.as_path() != worktree);
        }
        drop(st);
        self.changed.notify_all();
    }

    /// The repo is no longer observed: its pending captures are given up.
    pub(crate) fn forget(&self, repo_id: &str) {
        if let Some(repo) = self.state().repos.get_mut(repo_id) {
            repo.outstanding.clear();
        }
        self.changed.notify_all();
    }

    /// Waits, up to `limit`, until the capture of every event `seqs` of `repo_id` ended. An event
    /// past the highest one signalled is waited for up to `HANDOFF` to be signalled; one that
    /// never is, or one already captured, is not pending.
    pub fn await_bases(&self, repo_id: &str, seqs: &[i64], limit: Duration) -> Waited {
        let start = Instant::now();
        let end = start + limit;
        let handoff_end = start + HANDOFF.min(limit);
        let mut seen_pending = false;
        let mut st = self.state();
        let outcome = loop {
            let now = Instant::now();
            let repo = st.repos.get(repo_id);
            let mut held = false;
            let mut unsignalled = false;
            if st.running {
                for seq in seqs {
                    let signalled = repo.map_or(0, |r| r.signalled);
                    if repo.is_some_and(|r| r.outstanding.contains_key(seq)) {
                        held = true;
                    } else if *seq > signalled && now < handoff_end {
                        unsignalled = true;
                    }
                }
            }
            seen_pending |= held;
            if !held && !unsignalled {
                break if seen_pending {
                    WaitOutcome::Settled
                } else {
                    WaitOutcome::NotPending
                };
            }
            // An event not signalled yet is waited for up to the handoff, a pending one up to
            // the deadline.
            let until = if held { end } else { handoff_end.min(end) };
            if now >= until {
                break if held {
                    WaitOutcome::Expired
                } else if seen_pending {
                    WaitOutcome::Settled
                } else {
                    WaitOutcome::NotPending
                };
            }
            st = self
                .changed
                .wait_timeout(st, until - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        Waited {
            waited: start.elapsed(),
            outcome,
        }
    }
}

/// The planner's one entry to the wait: with an engine and events `missing` (an agent's changes
/// without a base), waits up to [`BASE_WAIT`] for their captures. `None` when there is nothing
/// to wait for: no engine, or every base is known.
pub(crate) fn wait_for_bases(
    deps: Option<&CaptureDeps>,
    repo_id: &str,
    missing: &[i64],
) -> Option<Waited> {
    let deps = deps?;
    if missing.is_empty() {
        return None;
    }
    Some(deps.seals.await_bases(repo_id, missing, BASE_WAIT))
}

#[cfg(test)]
#[path = "base_wait_tests.rs"]
mod tests;
