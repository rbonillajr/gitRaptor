//! Redo (ADR-TMC-003 § 4): takes back the last undo of a worktree, returning it to the point that
//! undo took right before itself. It is the undo with another selector and another target: the
//! same repo lock, engine settle, confirmation gate, overlap check, Git preconditions, prior point,
//! applier and anchor, recorded as a protected operation of kind `redo` over `Target::Redo(U)`.
//! Anything that cannot be redone refuses the whole request before the prior (NFR-01).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gitraptor_api::catalog::MAX_QUEUED_PER_REPO;
use gitraptor_api::timemachine::{PriorFailure, TmRejectReason};
use gitraptor_git::tm_write::WriteContext;

use super::apply::{Applier, ApplyError, ApplyPlan, ApplyReport, PlanWorktree, RefScope};
use super::confirm::{Confirmation, PlanFacts, PlanKind};
use super::continuous::{ANCHOR_SETTLE_LIMIT, anchor, await_moved_branches, branch_tips};
use super::oplog::{
    Channel, NewOperation, OpRef, OperationKind, OperationTransition, OperationView, Oplog,
    Requester, Scope, StackScope, Target,
};
use super::overlap::{self, OverlapInput, OverlapReport};
use super::protected::{
    ProtectedError, ProtectedOperation, ProtectedRequest, StepScope, registered_worktrees,
};
use super::repo_lock;
use super::store::{Meta, SnapshotStore};
use super::undo::{
    OVERLAP_BUDGET, RawSide, Tail, TmRepoHandle, UndoEnv, UndoStep, Undone, external_events,
    head_branch, lock, materialize, now_ms, raw_of_all, reason_code, refusal_reason, settle_all,
};
use crate::channel::requester::Who;

/// The subtype a redo is recorded with.
const REDO_SUBTYPE: &str = "redo";

/// A redo that ran to the end.
#[derive(Debug)]
pub struct RedoDone {
    /// The redo itself, as recorded.
    pub operation_id: String,
    /// The point taken right before the redo.
    pub prior_snapshot_id: String,
    /// The undo this redo took back.
    pub redone_undo_id: String,
    /// The undo's prior: the state the worktree returned to.
    pub target_snapshot_id: String,
    pub report: ApplyReport,
    /// The redo's own warnings as stable codes.
    pub warnings: Vec<String>,
}

/// Why a redo did not run to the end.
#[derive(Debug)]
pub enum RedoError {
    /// Nothing changed; `operation_id` is the rejected request in the oplog.
    Rejected {
        reason: TmRejectReason,
        operation_id: Option<String>,
    },
    /// Another actor changed, after the undo, something the redo would overwrite (or that could
    /// not be checked: `report.unverified`). Nothing changed.
    Overlap {
        operation_id: Option<String>,
        // Boxed: the report would make every `Result<_, RedoError>` large.
        report: Box<OverlapReport>,
        /// The undo the redo was asked to take back, to describe the overlap.
        undone: Box<Undone>,
    },
    /// The redo's prior snapshot failed: it did not run.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
    },
    /// Stopped half-way: the redo is `interrupted` and the next undo returns to its prior.
    Interrupted {
        operation_id: String,
        message: String,
    },
    /// The oplog or the write layer could not be used: nothing ran.
    Internal(String),
}

/// Whose work a redo takes back: the requester of the undo `undo` and the owner of everything that
/// undo took back (the frozen requester of an operation, the engine's actor of a raw Git event).
/// An operation or event that can no longer be read counts as unattributed: never as nobody's.
pub(crate) fn redo_owners(undo: &OperationView, raw: &RawSide, oplog: &Oplog) -> Vec<Requester> {
    let mut owners = vec![undo.record.requester.clone()];
    let taken_back: &[OpRef] = match &undo.record.target {
        Target::Undo(set) => set,
        _ => &[],
    };
    for op in taken_back {
        let owner = match op {
            OpRef::Oplog(id) => oplog
                .operation(id)
                .ok()
                .flatten()
                .map(|view| view.record.requester),
            OpRef::GitEvent(seq) => raw
                .events
                .iter()
                .find(|e| e.seq == *seq)
                .map(|e| e.actor.clone()),
        }
        .unwrap_or(Requester::Unattributed);
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    owners
}

/// Records a rejected redo request (ADR-TMC-005 § 4): `intent → rejected`, over the undo it was
/// asked to redo when there is one.
fn record_rejection(
    oplog: &Mutex<Oplog>,
    scope: Scope,
    undo: Option<&str>,
    who: &Who,
    channel: Channel,
    engine_mark: i64,
    reason: TmRejectReason,
) -> RedoError {
    let mut log = lock(oplog);
    let new = NewOperation {
        kind: OperationKind::Redo,
        subtype: None,
        scope,
        requester: who.requester.clone(),
        channel,
        confirmed: false,
        target: undo.map_or(Target::None, |u| Target::Redo(u.to_owned())),
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
    RedoError::Rejected {
        reason,
        operation_id,
    }
}

/// What a redo is about to do.
struct RedoPlan {
    /// The undo it takes back.
    undo: Undone,
    /// The undo's prior: where the worktrees return to.
    target: String,
    scope: Scope,
    worktrees: Vec<PlanWorktree>,
    refs: BTreeSet<String>,
    confirmed: bool,
    /// Phase 1 of the overlap check: where the tail starts.
    overlap: OverlapReport,
}

/// Why the plan refused: the reason, the scope and undo to record it under, and the overlap
/// report when that is the reason.
type RedoRefusal = (
    TmRejectReason,
    Scope,
    Option<String>,
    Option<(Box<OverlapReport>, Box<Undone>)>,
);

/// The inputs of [`plan`] besides the oplog.
struct PlanInput<'a> {
    store: Option<&'a SnapshotStore>,
    common_dir: &'a Path,
    worktree: &'a Path,
    who: &'a Who,
    channel: Channel,
    confirm: &'a Confirmation<'a>,
    raw: &'a RawSide,
    raw_all: &'a std::collections::HashMap<PathBuf, RawSide>,
    events_complete: bool,
    deadline: Instant,
}

/// Selection, target, owners, confirmation, phase 1 of the overlap and `ref-in-use`, in the
/// order of ADR-TMC-005 § 4, before any prior.
fn plan(oplog_lock: &Mutex<Oplog>, input: &PlanInput<'_>) -> Result<RedoPlan, RedoRefusal> {
    // The oplog is held while the stack and the target are read, and released before the
    // confirmation gate: that walks the caller's process tree. The repo's write lock (held by
    // the caller) keeps the plan from changing under it.
    let log = lock(oplog_lock);
    let oplog: &Oplog = &log;
    let key = input.worktree.to_string_lossy().into_owned();
    let own_scope = Scope {
        worktrees: vec![key.clone()],
        refs: Vec::new(),
    };
    let wt_key = super::protected::worktree_key(
        &registered_worktrees(input.common_dir).unwrap_or_default(),
        input.worktree,
        0,
    );
    let external = external_events(oplog, input.raw, &key, &wt_key);
    let stack = oplog
        .undo_stack_in(&StackScope::Worktree(key.clone()), &external, input.raw.floor)
        .map_err(|_| (TmRejectReason::TargetUnavailable, own_scope.clone(), None, None))?;
    let Some(undo_id) = stack.next_redo().map(str::to_owned) else {
        let reason = if stack.redo_invalidated().is_some() {
            TmRejectReason::RedoInvalidated
        } else {
            TmRejectReason::NothingToRedo
        };
        return Err((reason, own_scope, None, None));
    };
    let Some(view) = oplog.operation(&undo_id).ok().flatten() else {
        return Err((
            TmRejectReason::TargetUnavailable,
            own_scope,
            Some(undo_id),
            None,
        ));
    };
    let scope = view.record.scope.clone();
    let refuse = |reason| (reason, scope.clone(), Some(undo_id.clone()), None);
    // The target: the point the undo took right before itself, still a point of the timeline
    // and intact in the store.
    let target = view
        .prior_snapshot
        .clone()
        .ok_or_else(|| refuse(TmRejectReason::TargetUnavailable))?;
    let available = oplog
        .snapshot(&target)
        .ok()
        .flatten()
        .is_some_and(|s| s.state.is_available() && !s.tampered);
    let store = input
        .store
        .ok_or_else(|| refuse(TmRejectReason::TargetUnavailable))?;
    if !available || store.verify(&target).is_err() {
        return Err(refuse(TmRejectReason::TargetUnavailable));
    }
    let meta: Meta = store
        .meta(&target)
        .map_err(|_| refuse(TmRejectReason::TargetUnavailable))?;
    let owners = redo_owners(&view, input.raw, oplog);
    let undo = Undone {
        op: OpRef::Oplog(undo_id.clone()),
        id: undo_id.clone(),
        subtype: view.record.subtype.clone(),
        requester: view.record.requester.clone(),
        at_ms: Some(view.record.recorded_ms),
    };
    drop(log);

    // Base permission rule over every owner (ADR-TMC-005 § 2), then the interactive
    // confirmation of what it asks to confirm (§ 3). The redone undo enters the hash as
    // `undone_id`; `PlanKind::Redo` keeps it apart from an undo with the same facts.
    let requested = key;
    let facts = PlanFacts {
        kind: PlanKind::Redo,
        worktree: &requested,
        requester: &input.who.requester,
        channel: input.channel,
        target_snapshot: &target,
        undone_id: Some(&undo_id),
        undone_subtype: undo.subtype.as_deref(),
        scope: &scope,
        owners: &owners,
    };
    let confirmed = input
        .confirm
        .gate_over(
            super::restore::restore_permission(&input.who.requester, input.channel, &owners),
            &facts,
            &[],
        )
        .map_err(refuse)?;
    // Next: Guardrails (US-TMC-021).

    let registered = registered_worktrees(input.common_dir)
        .map_err(|_| refuse(TmRejectReason::WorktreeUnavailable))?;
    let (mut worktrees, mut refs) =
        materialize(input.common_dir, &meta, &scope).map_err(refuse)?;
    // Phase 1 of the overlap check (ADR-TMC-005 § 5) from the undo: its requester is the
    // reference, its anchor the base. Before `ref-in-use`: a kept branch never moves.
    let reference = vec![undo.requester.clone()];
    let undone = vec![undo.op.clone()];
    let report = {
        let log = lock(oplog_lock);
        let ops = log.operations(&Default::default());
        let snaps = log.snapshots(&super::oplog::SnapshotFilter::default());
        drop(log);
        match (ops, snaps) {
            (Ok(ops), Ok(snaps)) => overlap::check_with(
                store,
                &OverlapInput {
                    reference: &reference,
                    undone: &undone,
                    target: &target,
                    worktrees: &worktrees,
                    refs: &refs,
                    ops: &ops,
                    snaps: &snaps,
                    raw: input.raw_all,
                    deadline: input.deadline,
                },
                input.events_complete,
            ),
            _ => OverlapReport {
                unverified: true,
                ..OverlapReport::default()
            },
        }
    };
    if report.stops() {
        let reason = if report.unverified {
            TmRejectReason::OverlapUnverified
        } else {
            TmRejectReason::Overlap
        };
        return Err((
            reason,
            scope.clone(),
            Some(undo_id.clone()),
            Some((Box::new(report), Box::new(undo))),
        ));
    }
    for full in &report.kept_refs {
        refs.remove(full);
    }
    for w in &mut worktrees {
        if let Some(paths) = report.kept.get(&w.key) {
            w.keep.extend(paths.iter().cloned());
        }
    }
    // A branch checked out in a worktree outside the scope never moves: it would change that
    // worktree's history under its files.
    let in_use = registered.iter().any(|(root, _)| {
        !scope.worktrees.iter().any(|w| Path::new(w) == root)
            && head_branch(input.common_dir, root)
                .is_some_and(|branch| refs.contains(&format!("refs/heads/{branch}")))
    });
    if in_use {
        return Err(refuse(TmRejectReason::RefInUse));
    }
    // The redo records the refs it may move, so undoing it keeps the same scope.
    let scope = Scope {
        worktrees: scope.worktrees.clone(),
        refs: refs.iter().cloned().collect(),
    };
    Ok(RedoPlan {
        undo,
        target,
        scope,
        worktrees,
        refs,
        confirmed,
        overlap: report,
    })
}

/// Redoes the last undo of `repo.repo.worktree`, for `who` through `channel`, as one protected
/// `redo` operation with its own prior and `Target::Redo` of that undo.
///
/// # Errors
///
/// [`RedoError::Rejected`] with `nothing-to-redo` when no undo is pending, `redo-invalidated` when
/// a later operation cleared it, or any reason of the undo's checks; [`RedoError::Overlap`] when
/// another actor's later work would be overwritten; [`RedoError::Prior`] when the prior point
/// failed; [`RedoError::Interrupted`] when the applier stopped half-way; [`RedoError::Internal`]
/// when the write layer cannot be used. Nothing changed unless it is `Interrupted`.
pub fn redo_last(
    repo: &TmRepoHandle,
    who: &Who,
    channel: Channel,
    confirm: &Confirmation<'_>,
    env: &UndoEnv<'_>,
) -> Result<RedoDone, RedoError> {
    let oplog = &repo.repo.oplog;
    let worktree = repo.repo.worktree.as_path();
    let repo_id = repo.repo.repo_id.as_str();
    let mut engine_mark = env
        .engine
        .map_or(env.fallback_mark, |d| d.engine.mark(repo_id));
    let own_scope = || Scope {
        worktrees: vec![worktree.to_string_lossy().into_owned()],
        refs: Vec::new(),
    };
    // The repo is held from reading the stack to the close (DS-US-TMC-002 D9): the undo to redo
    // cannot change under the redo, and a busy repo costs no prior snapshot.
    let give_up = || env.stopping.load(std::sync::atomic::Ordering::SeqCst);
    let Ok(guard) =
        repo_lock::lock_queued(&repo.write_lock_key, MAX_QUEUED_PER_REPO, &give_up, |_| {})
    else {
        return Err(record_rejection(
            oplog,
            own_scope(),
            None,
            who,
            channel,
            engine_mark,
            TmRejectReason::RepoBusy,
        ));
    };

    // The engine first (US-TMC-004): every `git` that already ended is persisted, so it is in the
    // stack (it may invalidate the redo) and the mark covers it.
    let mut raw = RawSide::default();
    let mut others_settled = true;
    if let Some(deps) = env.engine {
        match deps
            .engine
            .settle(repo_id, &[worktree.to_path_buf()], ANCHOR_SETTLE_LIMIT)
        {
            Some(mark) => {
                engine_mark = mark;
                match settle_all(deps, repo_id, &repo.repo.common_dir, worktree) {
                    Some(mark) => engine_mark = mark,
                    None => others_settled = false,
                }
            }
            None => {
                return Err(record_rejection(
                    oplog,
                    own_scope(),
                    None,
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
    let rejected = |scope, undo: Option<&str>, reason| {
        record_rejection(oplog, scope, undo, who, channel, engine_mark, reason)
    };
    let (raw_all, events_complete) =
        raw_of_all(env.engine, repo_id, &repo.repo.common_dir, worktree);
    let events_complete = events_complete && others_settled;
    let overlap_started = Instant::now();

    let planned = plan(
        oplog,
        &PlanInput {
            store: repo.store.as_deref(),
            common_dir: &repo.repo.common_dir,
            worktree,
            who,
            channel,
            confirm,
            raw: &raw,
            raw_all: &raw_all,
            events_complete,
            deadline: overlap_started + OVERLAP_BUDGET,
        },
    );
    let planned = match planned {
        Ok(p) => p,
        Err((reason, scope, undo, None)) => return Err(rejected(scope, undo.as_deref(), reason)),
        Err((reason, scope, undo, Some((report, undone)))) => {
            let operation_id = match rejected(scope, undo.as_deref(), reason) {
                RedoError::Rejected { operation_id, .. } => operation_id,
                _ => None,
            };
            return Err(RedoError::Overlap {
                operation_id,
                report,
                undone,
            });
        }
    };
    let undo_id = planned.undo.id.clone();
    let tail_budget = OVERLAP_BUDGET.saturating_sub(overlap_started.elapsed());
    let (Some(store), Some(main_root)) = (repo.store.as_deref(), repo.main_root.clone()) else {
        return Err(rejected(
            planned.scope,
            Some(&undo_id),
            TmRejectReason::Unsupported,
        ));
    };
    let Some(git) = env.git else {
        return Err(rejected(
            planned.scope,
            Some(&undo_id),
            TmRejectReason::GitUnavailable,
        ));
    };
    let write = WriteContext::new(git.clone(), env.invoker.clone(), &repo.tm_dir)
        .map_err(|e| RedoError::Internal(format!("write layer: {e}")))?;

    let apply_plan = ApplyPlan {
        target_snapshot: planned.target.clone(),
        prior_snapshot: String::new(),
        worktrees: planned.worktrees.clone(),
        refs: RefScope::Only(planned.refs.clone()),
        selective: None,
    };
    // Git preconditions before the redo's prior (ADR-TMC-005 § 4); the applier runs them again
    // under its locks.
    {
        let applier = Applier::new(
            store,
            &write,
            oplog,
            main_root.clone(),
            repo.repo.common_dir.clone(),
            repo.profile_root.clone(),
        );
        if let Some(first) = applier.check_preconditions(&apply_plan).first() {
            return Err(rejected(
                planned.scope,
                Some(&undo_id),
                refusal_reason(first),
            ));
        }
    }

    let worktree_paths: Vec<PathBuf> = planned.scope.worktrees.iter().map(PathBuf::from).collect();
    let declared = StepScope {
        worktrees: worktree_paths.iter().skip(1).cloned().collect(),
        refs: planned.refs.iter().cloned().collect(),
    };
    let req = ProtectedRequest {
        kind: OperationKind::Redo,
        scope: planned.scope.clone(),
        worktree_paths,
        who: who.clone(),
        channel,
        confirmed: planned.confirmed,
        target: Target::Redo(undo_id.clone()),
        warnings: Vec::new(),
        engine_mark,
        binding: None,
    };
    let mut step = UndoStep {
        store,
        write: &write,
        guard: &guard,
        main_root,
        common_dir: repo.repo.common_dir.clone(),
        profile_root: repo.profile_root.clone(),
        plan: apply_plan,
        declared,
        result: None,
        tail: Some(Tail {
            reference: vec![planned.undo.requester.clone()],
            undone: vec![planned.undo.op.clone()],
            phase1: planned.overlap.clone(),
            engine: env.engine,
            repo_id: repo_id.to_owned(),
            worktree: worktree.to_path_buf(),
            budget: tail_budget,
            intent_mark: engine_mark,
            selected: None,
        }),
        overlap: None,
        refused: None,
        subtype: REDO_SUBTYPE,
    };
    let protected = ProtectedOperation {
        oplog,
        snapshotter: Arc::clone(&repo.repo.snapshotter),
        marks: env.marks,
        procs: env.procs,
        stopping: env.stopping,
        deadline: env.prior_deadline,
        prior_valid: None,
    };
    let tips_before = env
        .engine
        .map(|_| branch_tips(&repo.repo.common_dir))
        .unwrap_or_default();
    let ran = protected.run(&req, &mut step);
    // The state the redo left, while the repo is still held: its echo in the engine is never
    // taken for raw Git, so it neither clears the next redo nor overlaps the next undo.
    if let Some(deps) = env.engine {
        let id = match &ran {
            Ok(o) => Some(o.operation_id.as_str()),
            Err(ProtectedError::Step { operation_id, .. }) => Some(operation_id.as_str()),
            Err(_) => None,
        };
        if let Some(id) = id {
            let settled =
                await_moved_branches(deps, repo_id, &req.worktree_paths, &tips_before, engine_mark);
            anchor(deps, repo_id, &req.worktree_paths, id, settled);
        }
    }
    match ran {
        Ok(outcome) => Ok(RedoDone {
            operation_id: outcome.operation_id,
            prior_snapshot_id: outcome.prior.snapshot_id,
            redone_undo_id: undo_id,
            target_snapshot_id: planned.target,
            report: match step.result {
                Some(Ok(report)) => report,
                _ => ApplyReport::default(),
            },
            warnings: Vec::new(),
        }),
        Err(ProtectedError::Prior {
            reason,
            operation_id,
            ..
        }) => Err(RedoError::Prior {
            reason,
            operation_id,
        }),
        Err(ProtectedError::Oplog(e)) => Err(RedoError::Internal(format!("oplog: {e}"))),
        Err(ProtectedError::Step { operation_id, .. }) if step.refused.is_some() => {
            Err(RedoError::Rejected {
                reason: step.refused.unwrap_or(TmRejectReason::Unsupported),
                operation_id: Some(operation_id),
            })
        }
        Err(ProtectedError::Step { operation_id, .. })
            if step.overlap.as_ref().is_some_and(OverlapReport::stops) =>
        {
            Err(RedoError::Overlap {
                operation_id: Some(operation_id),
                report: Box::new(step.overlap.take().unwrap_or_default()),
                undone: Box::new(planned.undo),
            })
        }
        Err(ProtectedError::Step {
            operation_id,
            message,
            ..
        }) => match step.result {
            Some(Err(ApplyError::Rejected(refusals))) => Err(RedoError::Rejected {
                reason: refusals
                    .first()
                    .map_or(TmRejectReason::Unsupported, refusal_reason),
                operation_id: Some(operation_id),
            }),
            _ => Err(RedoError::Interrupted {
                operation_id,
                message,
            }),
        },
    }
}

#[cfg(test)]
#[path = "redo_tests.rs"]
mod tests;
