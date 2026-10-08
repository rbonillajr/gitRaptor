//! `undo` of the last operation of a worktree (US-TMC-002, BR-TMC-WF-001,
//! ADR-TMC-003 § 4, ADR-TMC-005).
//!
//! In the order of ADR-TMC-005 § 4: the worktree's stack gives the most
//! recent operation not undone yet; its guaranteed prior snapshot is the
//! target; the base permission rule runs on its frozen requester; the Git
//! preconditions run before the undo's own prior so a refused undo leaves no
//! useless point. Then the undo is a protected operation of kind `undo`
//! (ADR-TMC-004 § 1): intent, its own prior snapshot, and the applier of
//! TS-TMC-003 from `ready`. Every request that reaches the stack is in the
//! oplog, accepted or rejected, with its requester and reason.
//!
//! Raw Git events of the engine are in the stack too (US-TMC-004): their
//! target is the latest capture before them, and their owner the session the
//! engine attributed them to. Prepared for later stories: redo (US-TMC-003), overlap (US-TMC-012), confirmation (US-TMC-013) and
//! Guardrails (US-TMC-021) plug in at the marked points.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::catalog::MAX_QUEUED_PER_REPO;
use gitraptor_api::timemachine::{PriorFailure, TmRejectReason};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::{Invoker, ReaderOptions, RepoReader, SystemGit};

use super::apply::{Applier, ApplyError, ApplyPlan, ApplyReport, PlanWorktree, RefScope, Refusal};
use super::continuous::{ANCHOR_SETTLE_LIMIT, CaptureDeps, anchor};
use super::engine::{RawGitEvent, is_undoable};
use super::oplog::ExternalEvent;
use super::oplog::{
    Channel, NewOperation, OpRef, OperationKind, OperationTransition, OperationView, Oplog,
    Requester, Scope, SnapshotView, StackScope, Target,
};
use super::protected::{
    McpAllowlist, ProtectedError, ProtectedOperation, ProtectedRequest, ProtectedStep, RepoHandle,
    ScopeError, StepCtx, StepError, StepOutput, StepScope, registered_worktrees,
};
use super::repo_lock;
use super::restore::{KEPT_REF_IN_RECREATED_WORKTREE, RECREATED_WORKTREE_WARNING};
use super::store::SnapshotStore;
use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::ProcSource;
use crate::channel::requester::Who;

/// One observed repo, ready for the Time Machine's own commands.
#[derive(Clone)]
pub struct TmRepoHandle {
    /// Repo id, the requested worktree (canonical root), oplog and the
    /// snapshotter of the prior snapshot.
    pub repo: RepoHandle,
    /// `None` if the store cannot be opened.
    pub store: Option<Arc<SnapshotStore>>,
    /// The main worktree, whose Git folder holds refs and objects; `None`
    /// for a bare repo.
    pub main_root: Option<PathBuf>,
    /// `<tm>/<repo-id>`, where the write layer keeps its private folders.
    pub tm_dir: PathBuf,
    /// The profile, where no worktree may be recreated.
    pub profile_root: PathBuf,
    /// Key of the repo's write lock, shared with the executor and the
    /// applier (ADR-CKP-002 § 5): the path of the store.
    pub write_lock_key: String,
}

impl std::fmt::Debug for TmRepoHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TmRepoHandle")
            .field("repo", &self.repo)
            .finish_non_exhaustive()
    }
}

/// What the channel needs to run the Time Machine's commands.
pub trait UndoBackend: Send + Sync {
    /// The observed repo whose worktree root is `folder`.
    fn repo_of(&self, folder: &Path) -> Result<TmRepoHandle, ScopeError>;
    fn allowlist(&self) -> &dyn McpAllowlist;
}

/// The repo for a Time Machine command: over MCP from the caller's working
/// folder and only if allowlisted; otherwise from the folder the client
/// named (SEC-TMC-07, SEC-TMC-15).
pub fn tm_scope_for(
    backend: &dyn UndoBackend,
    mcp: bool,
    named: Option<&Path>,
    caller_cwd: Option<&Path>,
) -> Result<TmRepoHandle, ScopeError> {
    if mcp {
        let cwd = caller_cwd.ok_or(ScopeError::NoWorkingFolder)?;
        let repo = backend.repo_of(cwd)?;
        if !backend.allowlist().allows(&repo.repo.repo_id) {
            return Err(ScopeError::NotAllowlisted);
        }
        return Ok(repo);
    }
    backend.repo_of(named.ok_or(ScopeError::NotObserved)?)
}

/// The daemon's part of an undo besides the repo.
pub struct UndoEnv<'a> {
    pub marks: &'a Arc<ExecutorMarks>,
    pub procs: &'a dyn ProcSource,
    pub stopping: &'a AtomicBool,
    pub prior_deadline: Duration,
    /// Git resolved by the daemon; `None`: the undo is rejected.
    pub git: Option<&'a SystemGit>,
    pub invoker: &'a Invoker,
    /// The engine and the anchor capture (US-TMC-004); `None` for doubles:
    /// only oplog operations are in the stack, at `fallback_mark`.
    pub engine: Option<&'a CaptureDeps>,
    pub fallback_mark: i64,
}

/// A finished undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoDone {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    pub undone: Undone,
    pub target_snapshot_id: String,
    pub report: ApplyReport,
    /// The undo's own warnings as stable codes, also recorded in the oplog
    /// (e.g. [`super::restore::KEPT_REF_IN_RECREATED_WORKTREE`]).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoError {
    /// Nothing changed; `operation_id` is the rejected request in the oplog.
    Rejected {
        reason: TmRejectReason,
        operation_id: Option<String>,
    },
    /// The undo's prior snapshot failed: it did not run.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
    },
    /// Stopped half-way: the undo is `interrupted` and the next undo
    /// returns to its prior snapshot.
    Interrupted {
        operation_id: String,
        message: String,
    },
    /// The oplog or the write layer could not be used: nothing ran.
    Internal(String),
}

/// The base permission rule (ADR-TMC-005 § 2) for undoing work of `owner`.
/// The actor of a GitRaptor operation is its frozen requester. An agent
/// only undoes its own work, compared by session (the name is text the
/// agent supplies, for display only); an unattributed
/// requester undoes unattributed work, and an agent's only with an
/// interactive confirmation, which is US-TMC-013 (until then: rejected).
pub fn permission(
    who: &Requester,
    channel: Channel,
    owner: &Requester,
) -> Result<(), TmRejectReason> {
    match (who, owner) {
        (
            Requester::Agent { session_id, .. },
            Requester::Agent {
                session_id: owner_session,
                ..
            },
        ) if session_id == owner_session => Ok(()),
        (Requester::Agent { .. }, _) => Err(TmRejectReason::OtherActor),
        // Unattributed over MCP never gets here (TQ-7 → a); refuse anyway.
        (Requester::Unattributed, _) if channel == Channel::Mcp => Err(TmRejectReason::OtherActor),
        (Requester::Unattributed, Requester::Unattributed) => Ok(()),
        (Requester::Unattributed, Requester::Agent { .. }) => {
            Err(TmRejectReason::ConfirmationRequired)
        }
    }
}

/// The applier's refusal as a contract reason.
pub fn refusal_reason(refusal: &Refusal) -> TmRejectReason {
    match refusal {
        Refusal::InProgress { .. } => TmRejectReason::GitOperationInProgress,
        Refusal::GitBusy { .. } => TmRejectReason::GitBusy,
        Refusal::RepoBusy => TmRejectReason::RepoBusy,
        Refusal::Untrusted { .. } => TmRejectReason::RepoUntrusted,
        Refusal::WorktreeUnavailable { .. } => TmRejectReason::WorktreeUnavailable,
        Refusal::InvalidSnapshot { .. } => TmRejectReason::InvalidSnapshot,
        Refusal::HostileTree { .. } => TmRejectReason::HostileTree,
        Refusal::RefMoved { .. } => TmRejectReason::RefMoved,
        Refusal::Unsupported(_) => TmRejectReason::Unsupported,
    }
}

pub(super) fn reason_code(reason: TmRejectReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "rejected".into())
}

pub(super) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn now_ms() -> i64 {
    crate::daemon::now_ms()
}

/// What an undo takes back: an operation of the oplog or a raw Git event
/// of the engine (US-TMC-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub op: OpRef,
    /// The operation's id, or `git-event-<seq>` for a raw event.
    pub id: String,
    /// The operation's subtype, or the event's kind (`reset`, `commit`).
    pub subtype: Option<String>,
    /// Whose work it is: the operation's frozen requester, or the session
    /// the engine attributed the event to.
    pub requester: Requester,
}

/// What an undo is about to do.
struct Planned {
    undone: Undone,
    target: String,
    scope: Scope,
    worktrees: Vec<PlanWorktree>,
    refs: BTreeSet<String>,
    warnings: Vec<String>,
}

/// The engine's side of an undo's plan (US-TMC-004): the worktree's raw Git
/// events and where the current generation of the engine store starts in
/// the oplog. Empty without an engine: only oplog operations count.
#[derive(Debug, Clone, Default)]
pub struct RawSide {
    pub events: Vec<RawGitEvent>,
    pub floor: i64,
}

/// The raw events of `key` as the stack sees them: each one an operation of
/// GitRaptor caused is marked with it (ADR-TMC-003 § 4). An operation's echo
/// runs from its mark to the mark of its anchor (the capture of the state it
/// left); without an anchor, to the next snapshot of the worktree, or on.
pub(crate) fn external_events(
    oplog: &Oplog,
    raw: &RawSide,
    key: &str,
    wt_key: &str,
) -> Vec<ExternalEvent> {
    use super::oplog::SnapshotFilter;
    let ops = oplog.operations(&Default::default()).unwrap_or_default();
    let snaps = oplog
        .snapshots(&SnapshotFilter::default())
        .unwrap_or_default();
    external_events_in(&ops, &snaps, raw, key, wt_key)
}

/// [`external_events`] over what the oplog already answered: it reads nothing, so a caller that
/// asks for many worktrees reads the oplog once and holds its lock only for that.
pub(crate) fn external_events_in(
    ops: &[OperationView],
    snaps: &[SnapshotView],
    raw: &RawSide,
    key: &str,
    wt_key: &str,
) -> Vec<ExternalEvent> {
    use super::oplog::{OperationState, SnapshotLevel};
    // The mark of the capture each operation left, by operation: one pass over the snapshots.
    let mut anchors: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for s in snaps {
        if s.record.level == SnapshotLevel::Observation
            && let (Some(cause), Some(mark)) =
                (s.record.cause_operation.as_deref(), s.record.engine_mark)
        {
            let slot = anchors.entry(cause).or_insert(mark);
            *slot = (*slot).max(mark);
        }
    }
    let ran: Vec<(&OperationView, i64)> = ops
        .iter()
        .filter(|o| {
            o.record.seq >= raw.floor
                && o.record.scope.worktrees.iter().any(|w| w == key)
                && matches!(
                    o.state,
                    OperationState::Applying
                        | OperationState::Finished
                        | OperationState::Interrupted
                )
        })
        .map(|o| {
            let anchor = anchors.get(o.record.operation_id.as_str()).copied();
            let next = || {
                snaps
                    .iter()
                    .filter(|s| {
                        s.record.seq > o.record.seq
                            && s.record.worktrees.iter().any(|w| w == wt_key)
                            && s.record.cause_operation.as_deref() != Some(&o.record.operation_id)
                    })
                    .filter_map(|s| s.record.engine_mark)
                    .filter(|m| *m > o.record.engine_mark)
                    .min()
            };
            (o, anchor.or_else(next).unwrap_or(i64::MAX))
        })
        .collect();
    raw.events
        .iter()
        .map(|e| ExternalEvent {
            seq: e.seq,
            caused_by: ran
                .iter()
                .find(|(o, end)| o.record.engine_mark < e.seq && e.seq <= *end)
                .map(|(o, _)| o.record.operation_id.clone()),
        })
        .collect()
}

/// The state before a raw Git event (ADR-TMC-003 § 4): the latest snapshot
/// of the worktree, of the current generation, whose mark is before the
/// event, still a point of the timeline and intact in the store.
fn raw_target(
    oplog: &Oplog,
    store: Option<&SnapshotStore>,
    wt_key: &str,
    seq: i64,
    floor: i64,
) -> Option<String> {
    use super::oplog::SnapshotFilter;
    let mut candidates: Vec<_> = oplog
        .snapshots(&SnapshotFilter::default())
        .ok()?
        .into_iter()
        .filter(|s| {
            s.record.seq >= floor
                && s.record.worktrees.iter().any(|w| w == wt_key)
                && s.record.engine_mark.is_some_and(|m| m < seq)
                && s.state.is_available()
                && !s.tampered
        })
        .collect();
    candidates.sort_by_key(|s| (s.record.engine_mark, s.record.seq));
    let store = store?;
    candidates
        .into_iter()
        .rev()
        .find(|s| store.verify(&s.record.snapshot_id).is_ok())
        .map(|s| s.record.snapshot_id)
}

/// Steps 4–6 of ADR-TMC-005 § 4 (set, target, base rule). A refusal
/// carries the scope to record it under.
fn plan(
    oplog: &Oplog,
    store: Option<&SnapshotStore>,
    worktree: &Path,
    who: &Who,
    channel: Channel,
    raw: &RawSide,
) -> Result<Planned, (TmRejectReason, Scope, Vec<OpRef>)> {
    let key = worktree.to_string_lossy().into_owned();
    let own_scope = Scope {
        worktrees: vec![key.clone()],
        refs: Vec::new(),
    };
    let reject = |reason, scope: &Scope, target: Vec<OpRef>| (reason, scope.clone(), target);
    let wt_key = super::protected::worktree_key(
        &registered_worktrees(worktree).unwrap_or_default(),
        worktree,
        0,
    );
    let external = external_events(oplog, raw, &key, &wt_key);
    let stack = oplog
        .undo_stack_in(&StackScope::Worktree(key), &external, raw.floor)
        .map_err(|_| reject(TmRejectReason::TargetUnavailable, &own_scope, vec![]))?;
    // Keys of the worktrees the undone operation recreated, if it is a
    // restore: their branches are kept rather than refused (NFR-01).
    let mut recreated: Vec<String> = Vec::new();
    let (undone, scope, target) = match stack.last_operation() {
        None => return Err(reject(TmRejectReason::NothingToUndo, &own_scope, vec![])),
        Some(op @ OpRef::GitEvent(seq)) => {
            let event = raw.events.iter().find(|e| e.seq == *seq);
            let Some(event) = event.filter(|e| is_undoable(e.kind)) else {
                // Pushes, reconciliations, branches and worktrees created or
                // deleted: not undone yet, and never skipped.
                return Err(reject(
                    TmRejectReason::RawGitNotCovered,
                    &own_scope,
                    vec![op.clone()],
                ));
            };
            let scope = Scope {
                worktrees: own_scope.worktrees.clone(),
                refs: event
                    .branch
                    .iter()
                    .map(|b| format!("refs/heads/{b}"))
                    .collect(),
            };
            let target = raw_target(oplog, store, &wt_key, *seq, raw.floor).ok_or_else(|| {
                reject(TmRejectReason::TargetUnavailable, &scope, vec![op.clone()])
            })?;
            let undone = Undone {
                op: op.clone(),
                id: format!("git-event-{seq}"),
                subtype: Some(event.kind.as_str().to_owned()),
                requester: event.actor.clone(),
            };
            (undone, scope, target)
        }
        Some(OpRef::Oplog(id)) => {
            let Some(view) = oplog.operation(id).ok().flatten() else {
                return Err(reject(
                    TmRejectReason::TargetUnavailable,
                    &own_scope,
                    vec![],
                ));
            };
            let scope = view.record.scope.clone();
            if view.record.kind == OperationKind::Restore {
                recreated = view
                    .record
                    .warnings
                    .iter()
                    .filter_map(|w| {
                        w.strip_prefix(RECREATED_WORKTREE_WARNING)?
                            .strip_prefix(':')
                    })
                    .map(str::to_owned)
                    .collect();
            }
            let op = OpRef::Oplog(view.record.operation_id.clone());
            // The target: the operation's guaranteed prior snapshot.
            let target = view.prior_snapshot.clone().ok_or_else(|| {
                reject(TmRejectReason::TargetUnavailable, &scope, vec![op.clone()])
            })?;
            let undone = Undone {
                op,
                id: view.record.operation_id.clone(),
                subtype: view.record.subtype.clone(),
                requester: view.record.requester.clone(),
            };
            (undone, scope, target)
        }
    };
    let target_ref = vec![undone.op.clone()];
    let refuse = |reason| reject(reason, &scope, target_ref.clone());

    // Still a point of the timeline and intact in the store.
    let available = oplog
        .snapshot(&target)
        .ok()
        .flatten()
        .is_some_and(|s| s.state.is_available() && !s.tampered);
    let store = store.ok_or_else(|| refuse(TmRejectReason::TargetUnavailable))?;
    if !available || store.verify(&target).is_err() {
        return Err(refuse(TmRejectReason::TargetUnavailable));
    }
    let meta = store
        .meta(&target)
        .map_err(|_| refuse(TmRejectReason::TargetUnavailable))?;

    // Base permission rule (ADR-TMC-005 § 2).
    permission(&who.requester, channel, &undone.requester).map_err(refuse)?;
    // Next, in this order: confirmation (US-TMC-013), Guardrails
    // (US-TMC-021) and overlap (US-TMC-012).

    // Each worktree of the scope by its canonical root, as the daemon
    // recorded it, and still a worktree of this repo (the oplog is
    // untrusted input when read back, SEC-TMC-09); its key and branch from
    // the target's meta.
    let registered =
        registered_worktrees(worktree).map_err(|_| refuse(TmRejectReason::WorktreeUnavailable))?;
    let mut worktrees = Vec::new();
    let mut refs: BTreeSet<String> = scope.refs.iter().cloned().collect();
    for root in &scope.worktrees {
        if !registered.iter().any(|(p, _)| p == Path::new(root)) {
            // Removed, moved or foreign: not recreated yet (US-TMC-009).
            return Err(refuse(TmRejectReason::WorktreeUnavailable));
        }
        let in_meta = meta
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == Path::new(root))
            .ok_or_else(|| refuse(TmRejectReason::TargetUnavailable))?;
        if let Some(branch) = &in_meta.head_branch {
            refs.insert(format!("refs/heads/{branch}"));
        }
        if let Some(branch) = head_branch(Path::new(root)) {
            refs.insert(format!("refs/heads/{branch}"));
        }
        worktrees.push(PlanWorktree {
            key: in_meta.key.clone(),
            root: PathBuf::from(root),
            recreate_id: None,
        });
    }
    // A branch checked out in a worktree outside the scope never moves: it
    // would change that worktree's history under its files. Undoing a
    // restore keeps, instead, a branch only a worktree it recreated has out:
    // that worktree stays, so the undo always completes.
    let mut kept: BTreeSet<String> = BTreeSet::new();
    let mut in_use: BTreeSet<String> = BTreeSet::new();
    for (i, (root, _)) in registered.iter().enumerate() {
        let outside = !scope.worktrees.iter().any(|w| Path::new(w) == root);
        if outside
            && let Some(branch) = head_branch(root)
            && refs.contains(&format!("refs/heads/{branch}"))
        {
            let full = format!("refs/heads/{branch}");
            let key = super::protected::worktree_key(&registered, root, i);
            if recreated.contains(&key) {
                kept.insert(full);
            } else {
                in_use.insert(full);
            }
        }
    }
    if !in_use.is_empty() {
        return Err(refuse(TmRejectReason::RefInUse));
    }
    for full in &kept {
        refs.remove(full);
    }
    let warnings = if kept.is_empty() {
        Vec::new()
    } else {
        vec![KEPT_REF_IN_RECREATED_WORKTREE.to_owned()]
    };
    // The undo records the refs it may move, so undoing it (or redoing)
    // keeps the same scope.
    let scope = Scope {
        worktrees: scope.worktrees,
        refs: refs.iter().cloned().collect(),
    };
    Ok(Planned {
        undone,
        target,
        scope,
        worktrees,
        refs,
        warnings,
    })
}

/// The branch `HEAD` names in the worktree at `root`, if symbolic.
pub(super) fn head_branch(root: &Path) -> Option<String> {
    RepoReader::open(root, &ReaderOptions::default())
        .ok()?
        .head()
        .ok()?
        .branch
}

/// Records a rejected request (ADR-TMC-005 § 4): `intent → rejected`.
fn record_rejection(
    oplog: &Mutex<Oplog>,
    scope: Scope,
    target: Vec<OpRef>,
    who: &Who,
    channel: Channel,
    engine_mark: i64,
    reason: TmRejectReason,
) -> UndoError {
    let mut log = lock(oplog);
    let new = NewOperation {
        kind: OperationKind::Undo,
        subtype: None,
        scope,
        requester: who.requester.clone(),
        channel,
        confirmed: false,
        target: Target::Undo(target),
        warnings: Vec::new(),
        engine_mark,
    };
    let operation_id = log.record_operation(&new, now_ms()).ok().inspect(|id| {
        let _ = log.advance_operation(
            id,
            OperationTransition::Rejected {
                reason: &reason_code(reason),
            },
            now_ms(),
        );
    });
    UndoError::Rejected {
        reason,
        operation_id,
    }
}

/// The applier as the step of the undo's protected operation.
pub(super) struct UndoStep<'a> {
    pub(super) store: &'a SnapshotStore,
    pub(super) write: &'a WriteContext,
    /// The repo, held by the undo since it chose its target.
    pub(super) guard: &'a repo_lock::RepoGuard,
    pub(super) main_root: PathBuf,
    pub(super) profile_root: PathBuf,
    pub(super) plan: ApplyPlan,
    pub(super) declared: StepScope,
    pub(super) result: Option<Result<ApplyReport, ApplyError>>,
}

impl ProtectedStep for UndoStep<'_> {
    fn subtype(&self) -> &str {
        "undo"
    }

    fn scope(&self) -> StepScope {
        self.declared.clone()
    }

    fn self_annotated(&self) -> bool {
        true
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.plan.prior_snapshot = ctx.prior_snapshot_id().to_owned();
        let applier = Applier::new(
            self.store,
            self.write,
            ctx.oplog(),
            self.main_root.clone(),
            self.profile_root.clone(),
        );
        let result = applier.apply_holding(ctx.operation_id(), &self.plan, self.guard);
        let out = match &result {
            Ok(_) => Ok(StepOutput::default()),
            Err(e) => Err(StepError::new(e.to_string())),
        };
        self.result = Some(result);
        out
    }
}

/// Undoes the most recent operation of `repo.repo.worktree` that is not
/// undone yet, for `who` through `channel`.
pub fn undo_last(
    repo: &TmRepoHandle,
    who: &Who,
    channel: Channel,
    env: &UndoEnv<'_>,
) -> Result<UndoDone, UndoError> {
    let oplog = &repo.repo.oplog;
    let worktree = repo.repo.worktree.as_path();
    let repo_id = repo.repo.repo_id.as_str();
    let mut engine_mark = env
        .engine
        .map_or(env.fallback_mark, |d| d.engine.mark(repo_id));
    // The repo is held from choosing the target to the close, with the key
    // the executor queues on (ADR-CKP-002 § 5): the last operation cannot
    // change under the undo, and a busy repo costs no prior snapshot.
    let give_up = || env.stopping.load(std::sync::atomic::Ordering::SeqCst);
    let guard =
        match repo_lock::lock_queued(&repo.write_lock_key, MAX_QUEUED_PER_REPO, &give_up, |_| {}) {
            Ok(guard) => guard,
            Err(_) => {
                let scope = Scope {
                    worktrees: vec![worktree.to_string_lossy().into_owned()],
                    refs: Vec::new(),
                };
                return Err(record_rejection(
                    oplog,
                    scope,
                    Vec::new(),
                    who,
                    channel,
                    engine_mark,
                    TmRejectReason::RepoBusy,
                ));
            }
        };

    // The engine first (US-TMC-004): every `git` that already ended in the
    // worktree is persisted, so it is in the stack and the mark covers it.
    // An undo never plans on a stale mark: not calm in time, the repo is
    // busy.
    let mut raw = RawSide::default();
    if let Some(deps) = env.engine {
        let wt = [worktree.to_path_buf()];
        match deps.engine.settle(repo_id, &wt, ANCHOR_SETTLE_LIMIT) {
            Some(mark) => engine_mark = mark,
            None => {
                let scope = Scope {
                    worktrees: vec![worktree.to_string_lossy().into_owned()],
                    refs: Vec::new(),
                };
                return Err(record_rejection(
                    oplog,
                    scope,
                    Vec::new(),
                    who,
                    channel,
                    engine_mark,
                    TmRejectReason::RepoBusy,
                ));
            }
        }
        raw = RawSide {
            events: deps
                .engine
                .raw_events(repo_id, worktree)
                .unwrap_or_default(),
            floor: deps.engine.generation_floor(repo_id),
        };
    }
    let rejected = |scope, target, reason| {
        record_rejection(oplog, scope, target, who, channel, engine_mark, reason)
    };

    let planned = {
        let log = lock(oplog);
        plan(&log, repo.store.as_deref(), worktree, who, channel, &raw)
    };
    let planned = match planned {
        Ok(p) => p,
        Err((reason, scope, target)) => return Err(rejected(scope, target, reason)),
    };
    let target_ref = vec![planned.undone.op.clone()];
    let (Some(store), Some(main_root)) = (repo.store.as_deref(), repo.main_root.clone()) else {
        return Err(rejected(
            planned.scope,
            target_ref,
            TmRejectReason::Unsupported,
        ));
    };
    let Some(git) = env.git else {
        return Err(rejected(
            planned.scope,
            target_ref,
            TmRejectReason::GitUnavailable,
        ));
    };
    let write = WriteContext::new(git.clone(), env.invoker.clone(), &repo.tm_dir)
        .map_err(|e| UndoError::Internal(format!("write layer: {e}")))?;

    let apply_plan = ApplyPlan {
        target_snapshot: planned.target.clone(),
        prior_snapshot: String::new(),
        worktrees: planned.worktrees.clone(),
        refs: RefScope::Only(planned.refs.clone()),
    };
    // Git preconditions before the undo's prior (ADR-TMC-005 § 4); the
    // applier runs them again under its locks.
    {
        let applier = Applier::new(
            store,
            &write,
            oplog,
            main_root.clone(),
            repo.profile_root.clone(),
        );
        if let Some(first) = applier.check_preconditions(&apply_plan).first() {
            return Err(rejected(planned.scope, target_ref, refusal_reason(first)));
        }
    }

    let worktree_paths: Vec<PathBuf> = planned.scope.worktrees.iter().map(PathBuf::from).collect();
    let declared = StepScope {
        worktrees: worktree_paths.iter().skip(1).cloned().collect(),
        refs: planned.refs.iter().cloned().collect(),
    };
    let req = ProtectedRequest {
        kind: OperationKind::Undo,
        scope: planned.scope.clone(),
        worktree_paths,
        who: who.clone(),
        channel,
        confirmed: false,
        target: Target::Undo(target_ref),
        warnings: planned.warnings.clone(),
        engine_mark,
    };
    let mut step = UndoStep {
        store,
        write: &write,
        guard: &guard,
        main_root,
        profile_root: repo.profile_root.clone(),
        plan: apply_plan,
        declared,
        result: None,
    };
    let protected = ProtectedOperation {
        oplog,
        snapshotter: Arc::clone(&repo.repo.snapshotter),
        marks: env.marks,
        procs: env.procs,
        stopping: env.stopping,
        deadline: env.prior_deadline,
    };
    let ran = protected.run(&req, &mut step);
    // The state the undo left, while the repo is still held: its echo in the
    // engine is never taken for raw Git (US-TMC-004).
    if let Some(deps) = env.engine {
        let id = match &ran {
            Ok(o) => Some(o.operation_id.as_str()),
            Err(ProtectedError::Step { operation_id, .. }) => Some(operation_id.as_str()),
            Err(_) => None,
        };
        if let Some(id) = id {
            let worktrees: Vec<PathBuf> = req.worktree_paths.clone();
            anchor(deps, repo_id, &worktrees, id);
        }
    }
    match ran {
        Ok(outcome) => Ok(UndoDone {
            operation_id: outcome.operation_id,
            prior_snapshot_id: outcome.prior.snapshot_id,
            undone: planned.undone,
            target_snapshot_id: planned.target,
            report: match step.result {
                Some(Ok(report)) => report,
                _ => ApplyReport::default(),
            },
            warnings: planned.warnings,
        }),
        Err(ProtectedError::Prior {
            reason,
            operation_id,
            ..
        }) => Err(UndoError::Prior {
            reason,
            operation_id,
        }),
        Err(ProtectedError::Oplog(e)) => Err(UndoError::Internal(format!("oplog: {e}"))),
        Err(ProtectedError::Step {
            operation_id,
            message,
        }) => match step.result {
            Some(Err(ApplyError::Rejected(refusals))) => Err(UndoError::Rejected {
                reason: refusals
                    .first()
                    .map_or(TmRejectReason::Unsupported, refusal_reason),
                operation_id: Some(operation_id),
            }),
            _ => Err(UndoError::Interrupted {
                operation_id,
                message,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timemachine::oplog::RequesterOrigin;

    fn agent(name: &str, session: &str) -> Requester {
        Requester::Agent {
            name: name.into(),
            origin: RequesterOrigin::Detected,
            session_id: session.into(),
        }
    }

    /// ADR-TMC-005 § 2, row by row.
    #[test]
    fn the_base_permission_rule() {
        let claude1 = agent("claude-1", "10:1");
        let un = Requester::Unattributed;
        assert_eq!(permission(&claude1, Channel::Mcp, &claude1), Ok(()));
        assert_eq!(permission(&claude1, Channel::Cli, &claude1), Ok(()));
        assert_eq!(
            permission(&claude1, Channel::Cli, &agent("claude-2", "11:1")),
            Err(TmRejectReason::OtherActor)
        );
        // Same name, another session: another agent.
        assert_eq!(
            permission(&claude1, Channel::Cli, &agent("claude-1", "12:1")),
            Err(TmRejectReason::OtherActor)
        );
        // The session decides, not the name the agent supplies.
        assert_eq!(
            permission(&claude1, Channel::Cli, &agent("renamed", "10:1")),
            Ok(())
        );
        assert_eq!(
            permission(&claude1, Channel::Cli, &un),
            Err(TmRejectReason::OtherActor)
        );
        assert_eq!(permission(&un, Channel::Cli, &un), Ok(()));
        assert_eq!(permission(&un, Channel::Tui, &un), Ok(()));
        assert_eq!(
            permission(&un, Channel::Cli, &claude1),
            Err(TmRejectReason::ConfirmationRequired)
        );
        assert_eq!(
            permission(&un, Channel::Mcp, &un),
            Err(TmRejectReason::OtherActor)
        );
    }

    #[test]
    fn every_refusal_has_a_reason() {
        let wt = PathBuf::from("/r");
        for (refusal, reason) in [
            (
                Refusal::InProgress {
                    worktree: wt.clone(),
                    marker: "MERGE_HEAD",
                },
                TmRejectReason::GitOperationInProgress,
            ),
            (
                Refusal::GitBusy { lock: wt.clone() },
                TmRejectReason::GitBusy,
            ),
            (Refusal::RepoBusy, TmRejectReason::RepoBusy),
            (
                Refusal::RefMoved { name: "x".into() },
                TmRejectReason::RefMoved,
            ),
        ] {
            assert_eq!(refusal_reason(&refusal), reason);
        }
        assert_eq!(
            reason_code(TmRejectReason::NothingToUndo),
            "nothing-to-undo"
        );
    }
}
