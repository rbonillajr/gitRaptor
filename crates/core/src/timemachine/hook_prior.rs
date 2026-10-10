//! The `hook-prior` snapshot a Guardrails hook gets inside `guard.evaluate` (ADR-TMC-004 § 3).
//!
//! STUB: the contract is fixed so the red contract tests compile; every function is
//! unimplemented until the story's implementation replaces this file.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::guard::Operation;

use super::continuous::CaptureDeps;
use super::manual::{ManualFloor, QuotaHit, QuotaInput};
use super::oplog::{Oplog, Requester};
use super::store::{CaptureError, SnapshotStore};
use crate::guardrails::second_line::GitProcess;

/// Hard ceiling of one hook prior, from the moment the connection serves it. It must stay well
/// below the client's call timeout, or the hook would deny with an internal error.
pub const HOOK_PRIOR_DEADLINE: Duration = Duration::from_secs(5);
/// Debug-build test hook: the deadline in milliseconds.
pub const DEADLINE_ENV: &str = "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS";
/// Per requester and worktree, in a minute.
pub const PER_MINUTE: usize = 10;
/// Per requester and worktree, in 24 h.
pub const PER_DAY: usize = 120;
/// Every requester together, per worktree, in 24 h.
pub const PER_WORKTREE_DAY: usize = 300;
/// Every requester and worktree together, per repo, in 24 h.
pub const PER_REPO_DAY: usize = 1_000;

/// What a hook prior is asked for. Built by the daemon only: nothing in it comes from what the
/// hook sent.
#[derive(Debug, Clone)]
pub struct HookPriorAsk {
    pub repo_id: String,
    /// Working directory of the hook process, read by the daemon from the process; it must pass
    /// the worktree gate (`observe::open_registered_worktree`) before anything is recorded.
    pub worktree: PathBuf,
    /// The repo's common directory, from the registry, never from the client.
    pub common_dir: PathBuf,
    pub requester: Requester,
    /// The `git` that ran the hook: one snapshot per command.
    pub git: Option<GitProcess>,
}

/// A hook prior that exists: ref in the store and `complete` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPriorTaken {
    pub snapshot_id: String,
    pub worktree: PathBuf,
    /// The same `git` command already had this snapshot.
    pub reused: bool,
}

/// Why no hook prior was recorded. None of them leaves a `complete` row.
#[derive(Debug)]
pub enum HookPriorError {
    Quota(QuotaHit),
    NoSpace,
    TimeLimit,
    Discarded,
    NoWorktree,
    Unavailable,
    Capture(CaptureError),
}

/// The deadline of this daemon.
pub fn deadline() -> Duration {
    unimplemented!("US-TMC-005: hook_prior::deadline")
}

/// Whether the hook's operation asks for a prior: a rebase, or a reference transaction that
/// deletes a branch.
pub fn wants_prior(_op: &Operation) -> bool {
    unimplemented!("US-TMC-005: hook_prior::wants_prior")
}

/// Pure: the requester's minute and day, the worktree's day, then the repo's day.
pub fn quota(_input: &QuotaInput, _now_ms: i64) -> Result<(), QuotaHit> {
    unimplemented!("US-TMC-005: hook_prior::quota")
}

/// Takes one hook prior in `store` before `deadline`.
#[allow(clippy::too_many_arguments)]
pub fn capture_in_store(
    _store: &SnapshotStore,
    _oplog: &Mutex<Oplog>,
    _ask: &HookPriorAsk,
    _engine_mark: Option<i64>,
    _include_credentials: bool,
    _floor: Option<&ManualFloor<'_>>,
    _now_ms: i64,
    _deadline: Instant,
) -> Result<HookPriorTaken, HookPriorError> {
    unimplemented!("US-TMC-005: hook_prior::capture_in_store")
}

/// The daemon's path: the engine's calm, then [`capture_in_store`], before `deadline` (the
/// connection passes `Instant::now() + deadline()`; tests pass their own, nothing sleeps).
pub fn capture(
    _deps: &CaptureDeps,
    _ask: &HookPriorAsk,
    _now_ms: i64,
    _deadline: Instant,
) -> Result<HookPriorTaken, HookPriorError> {
    unimplemented!("US-TMC-005: hook_prior::capture")
}
