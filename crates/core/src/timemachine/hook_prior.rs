//! The `hook-prior` snapshot a Guardrails hook gets inside `guard.evaluate` (ADR-TMC-004 § 3).
//!
//! The daemon takes it before the destructive operation's effects, answers `complete` or
//! `failed` within a deadline and never lets a late capture become a point. Nothing in a request
//! localizes anything: the worktree is the one the daemon read from the hook process, and the
//! repo's directory comes from the registry. Like the manual snapshot, its quota is counted in
//! the oplog under a recording lock, every attempt that reached capture counts, and nothing is
//! ever deleted to make room.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gitraptor_api::guard::Operation;
use gitraptor_api::methods::QuotaWindow;
use gitraptor_git::ReadError;

use super::continuous::CaptureDeps;
use super::engine::GitState;
use super::manual::{
    DAY_MS, MINUTE_MS, ManualFloor, QuotaHit, QuotaInput, VolumeProbe, cached_reserve, folder_id,
    resolve_worktree,
};
use super::oplog::{HookPriorMeta, Oplog, Requester, SnapshotLevel};
use super::protected::{registered_worktrees, worktree_key};
use super::store::{CaptureError, CaptureRequest, SnapshotStore, ValidityGuard, WorktreeScope};
use crate::guardrails::second_line::GitProcess;
use crate::profile::settings::include_credential_files;

/// Hard ceiling of one hook prior, from the moment the connection serves it. It must stay well
/// below the client's call timeout, or the hook would deny with an internal error.
pub const HOOK_PRIOR_DEADLINE: Duration = Duration::from_secs(5);
/// Debug-build test hook: the deadline in milliseconds.
pub const DEADLINE_ENV: &str = "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS";
/// Per requester and worktree, in a minute.
pub const PER_MINUTE: usize = 10;
/// Per requester and worktree, in 24 h.
pub const PER_DAY: usize = 120;
/// Every agent together, per worktree, in 24 h.
pub const PER_WORKTREE_DAY: usize = 300;
/// Every agent and worktree together, per repo, in 24 h.
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

/// What a `git` command already got, kept by the store: a failure too, so a command is never
/// retried (the worst a command costs is one deadline).
pub(crate) type Booked = Result<(String, PathBuf), BookedFailure>;

/// [`HookPriorError`] as the book keeps it: a capture failure is kept by what it says.
#[derive(Debug, Clone)]
pub(crate) enum BookedFailure {
    Quota(QuotaHit),
    NoSpace,
    TimeLimit,
    Discarded,
    NoWorktree,
    Unavailable,
    Capture(String),
}

impl From<&HookPriorError> for BookedFailure {
    fn from(e: &HookPriorError) -> Self {
        match e {
            HookPriorError::Quota(hit) => Self::Quota(*hit),
            HookPriorError::NoSpace => Self::NoSpace,
            HookPriorError::TimeLimit => Self::TimeLimit,
            HookPriorError::Discarded => Self::Discarded,
            HookPriorError::NoWorktree => Self::NoWorktree,
            HookPriorError::Unavailable => Self::Unavailable,
            HookPriorError::Capture(e) => Self::Capture(e.to_string()),
        }
    }
}

impl From<BookedFailure> for HookPriorError {
    fn from(f: BookedFailure) -> Self {
        match f {
            BookedFailure::Quota(hit) => Self::Quota(hit),
            BookedFailure::NoSpace => Self::NoSpace,
            BookedFailure::TimeLimit => Self::TimeLimit,
            BookedFailure::Discarded => Self::Discarded,
            BookedFailure::NoWorktree => Self::NoWorktree,
            BookedFailure::Unavailable => Self::Unavailable,
            BookedFailure::Capture(m) => Self::Capture(CaptureError::InvalidInput(m)),
        }
    }
}

/// The deadline of this daemon.
pub fn deadline() -> Duration {
    if cfg!(debug_assertions)
        && let Some(ms) = std::env::var(DEADLINE_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    HOOK_PRIOR_DEADLINE
}

/// Whether the hook's operation asks for a prior: a rebase, or a reference transaction that
/// deletes a branch. Only those hooks run before the working tree is touched; the same
/// `reference-transaction` of a creation or a move also arrives after a checkout or a reset
/// rewrote it, and a point there would mix two states.
pub fn wants_prior(op: &Operation) -> bool {
    match op {
        Operation::Rebase { .. } => true,
        Operation::RefTransaction { updates, .. } => updates
            .iter()
            .any(|u| u.new.is_zero() && u.refname.starts_with("refs/heads/")),
        _ => false,
    }
}

/// Pure: the requester's minute and day, the worktree's day, then the repo's day. The first
/// full window refuses, with when its oldest attempt leaves it.
pub fn quota(input: &QuotaInput, now_ms: i64) -> Result<(), QuotaHit> {
    let windows = [
        (
            QuotaWindow::Minute,
            &input.requester_ms,
            MINUTE_MS,
            PER_MINUTE,
        ),
        (QuotaWindow::Day, &input.requester_ms, DAY_MS, PER_DAY),
        (
            QuotaWindow::WorktreeDay,
            &input.worktree_ms,
            DAY_MS,
            PER_WORKTREE_DAY,
        ),
        (QuotaWindow::RepoDay, &input.repo_ms, DAY_MS, PER_REPO_DAY),
    ];
    for (window, stamps, window_ms, limit) in windows {
        let floor = now_ms.saturating_sub(window_ms);
        let (count, oldest) = stamps
            .iter()
            .copied()
            .filter(|s| *s > floor)
            .fold((0usize, i64::MAX), |(n, min), s| (n + 1, min.min(s)));
        if count >= limit {
            return Err(QuotaHit {
                window,
                release_at_ms: oldest.saturating_add(window_ms),
            });
        }
    }
    Ok(())
}

/// The worktree as the daemon reads it, through the one gate every Time Machine capture uses.
struct Verified {
    root: PathBuf,
    key: String,
    id: Option<(u64, u64)>,
}

/// Resolves `ask.worktree` and opens it only as a worktree the repo registers and whose `.git`
/// it owns (`observe::open_registered_worktree`). A folder that is not, or whose `.git` points
/// at another repo, is refused before anything is recorded or read behind it.
fn verify(ask: &HookPriorAsk) -> Result<Verified, HookPriorError> {
    let (root, key) = resolve_worktree(&ask.worktree).map_err(|_| HookPriorError::NoWorktree)?;
    match crate::observe::open_registered_worktree(&ask.common_dir, &root) {
        Ok(_) => {}
        Err(e @ ReadError::Untrusted(_)) => return Err(HookPriorError::Capture(e.into())),
        Err(_) => return Err(HookPriorError::NoWorktree),
    }
    let id = folder_id(&root);
    Ok(Verified { root, key, id })
}

fn reuse(booked: Booked) -> Result<HookPriorTaken, HookPriorError> {
    booked
        .map(|(snapshot_id, worktree)| HookPriorTaken {
            snapshot_id,
            worktree,
            reused: true,
        })
        .map_err(Into::into)
}

/// Takes one hook prior in `store` before `deadline`.
///
/// 1. The command that already has its prior (or its failure) gets it back, `reused`.
/// 2. The worktree is resolved and verified ([`HookPriorError::NoWorktree`], or
///    `Capture(Read(Untrusted))`), before any row.
/// 3. The recording lock is waited for until `deadline` ([`HookPriorError::TimeLimit`]).
/// 4. Under it: the quota (the requester's windows; the worktree and repo ceilings only for
///    agents), the free-space floor, and the `pending` row, before the worktree is read.
/// 5. The capture stops, and nothing is deleted, if `deadline` passes, the volume goes below the
///    floor or the Git state changes: the attempt counts and there is no point.
/// 6. The result is kept for the `git` command and returned.
///
/// A `rebase` in progress or a lock of the index is not refused: the hook runs with `git`
/// stopped, and the validity guard drops a capture that sees the state change. `engine_mark` is
/// the engine's mark the read state reaches; `now_ms` the request's instant (every row of the
/// attempt carries it). Nothing is written in the repo.
#[allow(clippy::too_many_arguments)]
pub fn capture_in_store(
    store: &SnapshotStore,
    oplog: &Mutex<Oplog>,
    ask: &HookPriorAsk,
    engine_mark: Option<i64>,
    include_credentials: bool,
    floor: Option<&ManualFloor<'_>>,
    now_ms: i64,
    deadline: Instant,
) -> Result<HookPriorTaken, HookPriorError> {
    if let Some(booked) = ask.git.and_then(|g| store.hook_prior().booked(g)) {
        return reuse(booked);
    }
    // A refused worktree is not kept: it is cheap to refuse again and nothing was recorded.
    let verified = verify(ask)?;
    let result = record_one(
        store,
        verified,
        oplog,
        ask,
        engine_mark,
        include_credentials,
        floor,
        now_ms,
        deadline,
    );
    if let Some(git) = ask.git {
        let booked: Booked = match &result {
            Ok(taken) => Ok((taken.snapshot_id.clone(), taken.worktree.clone())),
            Err(e) => Err(e.into()),
        };
        store.hook_prior().remember(git, booked);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn record_one(
    store: &SnapshotStore,
    verified: Verified,
    oplog: &Mutex<Oplog>,
    ask: &HookPriorAsk,
    engine_mark: Option<i64>,
    include_credentials: bool,
    floor: Option<&ManualFloor<'_>>,
    now_ms: i64,
    deadline: Instant,
) -> Result<HookPriorTaken, HookPriorError> {
    let Verified { root, key, id } = verified;
    let registered = registered_worktrees(&ask.common_dir).unwrap_or_default();
    let store_key = worktree_key(&registered, &root, 0);

    let Some(_recording) = store.hook_prior().lock_recording(deadline) else {
        return Err(HookPriorError::TimeLimit);
    };
    // A late attempt is not even recorded.
    if Instant::now() >= deadline {
        return Err(HookPriorError::TimeLimit);
    }
    // The root the gate verified must still be the folder at this path.
    if folder_id(&root) != id {
        return Err(HookPriorError::Discarded);
    }
    {
        let session = ask.requester.session_id();
        let log = oplog.lock().unwrap_or_else(|p| p.into_inner());
        let mut input = log
            .hook_prior_quota_input(session, &key, now_ms)
            .map_err(|e| HookPriorError::Capture(e.into()))?;
        if session.is_none() {
            // Q-GRD-37: the ceilings of the worktree and the repo are the agents'. An
            // unattributed requester only answers to its own bucket.
            input.worktree_ms.clear();
            input.repo_ms.clear();
        }
        quota(&input, now_ms).map_err(HookPriorError::Quota)?;
    }
    if floor.is_some_and(|f| f.crossed(store.path())) {
        return Err(HookPriorError::NoSpace);
    }

    let at_start = GitState::read(&root);
    let guarded = root.clone();
    let req = CaptureRequest {
        level: SnapshotLevel::HookPrior,
        common_dir: ask.common_dir.clone(),
        worktrees: vec![WorktreeScope {
            key: store_key,
            path: root.clone(),
            hint: None,
        }],
        engine_mark,
        cause_operation: None,
        cause_event_seq: None,
        include_credentials,
        // Evaluated once, right before the validity point: whatever arrives late is discarded,
        // so a `complete` row never follows an answer of `failed`.
        still_valid: Some(ValidityGuard(std::sync::Arc::new(move || {
            Instant::now() < deadline && GitState::read(&guarded) == at_start
        }))),
        give_way: None,
    };
    let meta = HookPriorMeta {
        requester: ask.requester.clone(),
        worktree_key: key,
        requested_ms: now_ms,
    };
    let crossed = AtomicBool::new(false);
    let abort = || {
        if Instant::now() >= deadline {
            return true;
        }
        floor.is_some_and(|f| {
            let below = f.crossed(store.path());
            if below {
                crossed.store(true, Ordering::Release);
            }
            below
        })
    };
    match store.capture_hook_prior_until(oplog, &req, &meta, &abort) {
        Ok(out) => Ok(HookPriorTaken {
            snapshot_id: out.snapshot_id,
            worktree: root,
            reused: false,
        }),
        Err(CaptureError::Yielded | CaptureError::Discarded) => {
            Err(if crossed.load(Ordering::Acquire) {
                HookPriorError::NoSpace
            } else if Instant::now() >= deadline {
                HookPriorError::TimeLimit
            } else {
                HookPriorError::Discarded
            })
        }
        Err(e) => Err(HookPriorError::Capture(e)),
    }
}

/// The daemon's path: the engine's calm, then [`capture_in_store`], before `deadline` (the
/// connection passes `Instant::now() + deadline()`; tests pass their own, nothing sleeps). The
/// credential option and the floor come from the profile, never from the request.
pub fn capture(
    deps: &CaptureDeps,
    ask: &HookPriorAsk,
    now_ms: i64,
    deadline: Instant,
) -> Result<HookPriorTaken, HookPriorError> {
    let (oplog, store) = deps
        .repos
        .repo(&ask.repo_id)
        .ok_or(HookPriorError::Unavailable)?;
    let store = store.ok_or(HookPriorError::Unavailable)?;
    if let Some(booked) = ask.git.and_then(|g| store.hook_prior().booked(g)) {
        return reuse(booked);
    }
    // The gate before anything reads the folder (the wait for calm, the estimate of its size).
    let Verified { root, key, .. } = verify(ask)?;
    let left = deadline.saturating_duration_since(Instant::now());
    let Some(mark) = deps
        .engine
        .settle(&ask.repo_id, std::slice::from_ref(&ask.worktree), left)
    else {
        return Err(book(&store, ask, HookPriorError::TimeLimit));
    };
    let probe = VolumeProbe;
    let floor = deps.free_space_floor.map(|floor| ManualFloor {
        floor,
        reserve_bytes: cached_reserve(&store, &key, &root, deadline),
        probe: &probe,
    });
    capture_in_store(
        &store,
        &oplog,
        ask,
        Some(mark),
        include_credential_files(&deps.profile),
        floor.as_ref(),
        now_ms,
        deadline,
    )
}

/// Keeps a failure for the `git` command, so its next hook does not try again.
fn book(store: &SnapshotStore, ask: &HookPriorAsk, error: HookPriorError) -> HookPriorError {
    if let Some(git) = ask.git {
        store.hook_prior().remember(git, Err((&error).into()));
    }
    error
}
