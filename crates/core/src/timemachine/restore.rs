//! `restore` of a worktree to a point of the timeline (US-TMC-009,
//! BR-TMC-WF-003, ADR-TMC-004 § 1, ADR-TMC-005).
//!
//! A protected operation of kind `restore` on the path of `undo_last`: the
//! repo lock, the engine settled, the plan, the base permission rule over
//! every actor whose work the restore takes back, the Git preconditions, the
//! intent, its own prior snapshot and the applier with a bounded ref scope.
//! It stays in the worktree's stack, so `undo` takes it back.
//!
//! Worktrees the point had and that are gone are recreated without
//! checkout. Each one is recorded in the restore's `warnings` as
//! `recreated-worktree:<key>` ([`recreated_worktree_warning`]), with the
//! key the point's meta gives it, so an undo of the restore never refuses
//! a branch only that worktree has out: it keeps it and says so with
//! [`KEPT_REF_IN_RECREATED_WORKTREE`].
//!
//! What a restore reaches: the requested worktree, plus the worktrees and
//! branches that the work done from it after the point touched (the scopes
//! of those operations and the raw Git events of the plan's worktrees),
//! limited to what differs between the point and now. A branch the point
//! does not have is never deleted (it is listed as kept); a branch of the
//! point that is gone and that nothing of the plan touched does not come
//! back (it is listed as not returned). Worktree roots come from the repo's
//! validated state; the oplog's scopes and the point's meta are untrusted
//! and only intersect it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gitraptor_api::catalog::MAX_QUEUED_PER_REPO;
use gitraptor_api::timemachine::{PriorFailure, TmRejectReason};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::{ReaderOptions, RepoReader};

use super::apply::{Applier, ApplyError, ApplyPlan, ApplyReport, PlanWorktree, RefScope};
use super::continuous::{ANCHOR_SETTLE_LIMIT, anchor, await_moved_branches};
use super::engine::RawGitEvent;
use super::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, OperationView,
    Oplog, Requester, Scope, SnapshotFilter, SnapshotView, Target,
};
use super::protected::{
    ProtectedError, ProtectedOperation, ProtectedRequest, StepScope, registered_worktrees,
    worktree_key,
};
use super::repo_lock;
use super::store::{Meta, SnapshotStore};
use super::undo::{
    RawSide, TmRepoHandle, UndoEnv, UndoStep, external_events_in, head_branch, lock, now_ms,
    permission, reason_code, refusal_reason,
};
use crate::channel::requester::Who;

/// Warning prefix a restore records for each worktree it recreated,
/// followed by `:` and the worktree's key in the point's meta.
pub const RECREATED_WORKTREE_WARNING: &str = "recreated-worktree";

/// Warning code an undo of a restore records when it leaves a branch as it
/// is because only a worktree that restore recreated has it checked out.
pub const KEPT_REF_IN_RECREATED_WORKTREE: &str = "kept-ref-in-recreated-worktree";

/// The warning a restore records for the recreated worktree `key`.
pub fn recreated_worktree_warning(key: &str) -> String {
    format!("{RECREATED_WORKTREE_WARNING}:{key}")
}

/// A finished restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreDone {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    pub target_snapshot_id: String,
    /// Existing worktrees brought to the point, the requested one first.
    pub worktrees: Vec<PathBuf>,
    /// Worktrees of the point that were gone and were recreated.
    pub recreated: Vec<PathBuf>,
    /// Full ref names moved to the point's values.
    pub refs: Vec<String>,
    /// Local branches that exist now and not at the point: left as they are.
    pub kept_branches: Vec<String>,
    /// Branches of the point that are gone and were not brought back,
    /// since nothing done from the plan's worktrees touched them.
    pub not_returned_branches: Vec<String>,
    pub report: ApplyReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// No snapshot with that id in this repo's oplog. Not recorded.
    NotFound,
    /// Nothing changed; `operation_id` is the rejected request in the oplog.
    Rejected {
        reason: TmRejectReason,
        operation_id: Option<String>,
    },
    /// The restore's prior snapshot failed: it did not run.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
    },
    /// Stopped half-way: the restore is `interrupted` and `undo` returns to
    /// its prior snapshot.
    Interrupted {
        operation_id: String,
        message: String,
    },
    /// The oplog or the write layer could not be used: nothing ran.
    Internal(String),
}

/// The base permission rule (ADR-TMC-005 § 2) over every actor whose work
/// the restore takes back. An unattributed requester over MCP is always
/// `OtherActor`, owners or not; otherwise `Ok` with no owners. Any owner that yields
/// `OtherActor` makes the result `OtherActor`; otherwise any
/// `ConfirmationRequired` makes it `ConfirmationRequired`. Each owner is
/// checked with [`super::undo::permission`].
pub fn restore_permission(
    who: &Requester,
    channel: Channel,
    owners: &[Requester],
) -> Result<(), TmRejectReason> {
    // Unattributed over MCP never gets here (TQ-7 → a); refuse anyway.
    if matches!(who, Requester::Unattributed) && channel == Channel::Mcp {
        return Err(TmRejectReason::OtherActor);
    }
    let mut confirm = false;
    for owner in owners {
        match permission(who, channel, owner) {
            Ok(()) => {}
            Err(TmRejectReason::ConfirmationRequired) => confirm = true,
            Err(other) => return Err(other),
        }
    }
    if confirm {
        Err(TmRejectReason::ConfirmationRequired)
    } else {
        Ok(())
    }
}

/// Records a rejected restore (ADR-TMC-005 § 4): `intent → rejected`.
fn record_restore_rejection(
    oplog: &Mutex<Oplog>,
    scope: Scope,
    target_id: &str,
    who: &Who,
    channel: Channel,
    engine_mark: i64,
    reason: TmRejectReason,
) -> RestoreError {
    let mut log = lock(oplog);
    let new = NewOperation {
        kind: OperationKind::Restore,
        subtype: None,
        scope,
        requester: who.requester.clone(),
        channel,
        confirmed: false,
        target: Target::Snapshot(target_id.to_owned()),
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
    RestoreError::Rejected {
        reason,
        operation_id,
    }
}

fn root_key(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

/// The point's meta if it is still a point of the timeline, intact in the
/// store, and it holds `worktree` (ADR-TMC-003 § 3).
fn valid_point(
    store: Option<&SnapshotStore>,
    point: &SnapshotView,
    worktree: &Path,
) -> Option<Meta> {
    if !point.state.is_available() || point.tampered {
        return None;
    }
    let store = store?;
    store.verify(&point.record.snapshot_id).ok()?;
    let meta = store.meta(&point.record.snapshot_id).ok()?;
    meta.worktrees
        .iter()
        .any(|w| Path::new(&w.path) == worktree)
        .then_some(meta)
}

/// The operations done after the point: finished or cut, with a prior
/// snapshot taken at the point or later (the one whose prior is the point
/// included). The second list holds the tampered ones that may be after the
/// point (recorded after it, or with a prior at it or later): their owner
/// cannot be told, so they are never folded silently.
fn operations_after<'a>(
    ops: &'a [OperationView],
    snaps: &[SnapshotView],
    point: &SnapshotView,
) -> (Vec<&'a OperationView>, Vec<&'a OperationView>) {
    let seq_of: HashMap<&str, i64> = snaps
        .iter()
        .map(|s| (s.record.snapshot_id.as_str(), s.record.seq))
        .collect();
    let prior_after = |o: &OperationView| {
        o.prior_snapshot
            .as_deref()
            .and_then(|p| seq_of.get(p))
            .is_some_and(|seq| *seq >= point.record.seq)
    };
    let (tampered, intact): (Vec<&OperationView>, Vec<&OperationView>) =
        ops.iter().partition(|o| o.tampered);
    let after = intact
        .into_iter()
        .filter(|o| {
            matches!(
                o.state,
                OperationState::Finished | OperationState::Interrupted
            ) && prior_after(o)
        })
        .collect();
    let tampered = tampered
        .into_iter()
        .filter(|o| o.record.seq > point.record.seq || prior_after(o))
        .collect();
    (after, tampered)
}

/// A raw Git event of a registered worktree, after the point.
struct RawAfter {
    root: PathBuf,
    event: RawGitEvent,
    /// An echo of an operation of the oplog: its owner is that operation's.
    echo: bool,
}

fn is_dir(root: &Path) -> bool {
    std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir())
}

/// Restores `repo.repo.worktree` to snapshot `snapshot_id` for `who`
/// through `channel`.
///
/// # Errors
///
/// [`RestoreError::NotFound`] for an id this repo's oplog does not have;
/// otherwise as [`RestoreError`] describes.
pub fn restore_to(
    repo: &TmRepoHandle,
    snapshot_id: &str,
    who: &Who,
    channel: Channel,
    env: &UndoEnv<'_>,
) -> Result<RestoreDone, RestoreError> {
    let oplog = &repo.repo.oplog;
    let worktree = repo.repo.worktree.as_path();
    let repo_id = repo.repo.repo_id.as_str();
    // A point of another repo is the same as an unknown one: nothing is
    // recorded for it.
    if lock(oplog).snapshot(snapshot_id).ok().flatten().is_none() {
        return Err(RestoreError::NotFound);
    }
    let mut engine_mark = env
        .engine
        .map_or(env.fallback_mark, |d| d.engine.mark(repo_id));
    let own_scope = Scope {
        worktrees: vec![root_key(worktree)],
        refs: Vec::new(),
    };
    let reject = |scope: Scope, mark: i64, reason| {
        record_restore_rejection(oplog, scope, snapshot_id, who, channel, mark, reason)
    };

    // The repo is held from planning to the close, with the key the executor
    // queues on (ADR-CKP-002 § 5): a busy repo costs no prior snapshot.
    let give_up = || env.stopping.load(std::sync::atomic::Ordering::SeqCst);
    let Ok(guard) =
        repo_lock::lock_queued(&repo.write_lock_key, MAX_QUEUED_PER_REPO, &give_up, |_| {})
    else {
        return Err(reject(own_scope, engine_mark, TmRejectReason::RepoBusy));
    };

    // The engine first: every `git` that already ended in the worktree is
    // persisted. Never planned on a stale mark: not calm in time, busy.
    let mut floor = 0;
    if let Some(deps) = env.engine {
        match deps
            .engine
            .settle(repo_id, &[worktree.to_path_buf()], ANCHOR_SETTLE_LIMIT)
        {
            Some(mark) => engine_mark = mark,
            None => return Err(reject(own_scope, engine_mark, TmRejectReason::RepoBusy)),
        }
        floor = deps.engine.generation_floor(repo_id);
    }

    // One read of the oplog; its lock is released right after.
    let (ops, snaps) = {
        let log = lock(oplog);
        (
            log.operations(&Default::default()),
            log.snapshots(&SnapshotFilter::default()),
        )
    };
    let (Ok(ops), Ok(snaps)) = (ops, snaps) else {
        return Err(RestoreError::Internal("oplog unavailable".into()));
    };

    // A valid point (ADR-TMC-003 § 3): complete, intact, holding this worktree.
    let point = snaps
        .iter()
        .find(|s| s.record.snapshot_id == snapshot_id)
        .cloned();
    let Some((point, meta)) = point.and_then(|p| {
        let meta = valid_point(repo.store.as_deref(), &p, worktree)?;
        Some((p, meta))
    }) else {
        return Err(reject(
            own_scope,
            engine_mark,
            TmRejectReason::TargetUnavailable,
        ));
    };
    let (after, tampered_after) = operations_after(&ops, &snaps, &point);

    // The worktrees of the plan, by their roots in the repo's validated
    // state (SEC-TMC-09): the oplog's scopes and the meta only intersect it.
    let Ok(registered) = registered_worktrees(worktree) else {
        return Err(reject(
            own_scope,
            engine_mark,
            TmRejectReason::WorktreeUnavailable,
        ));
    };
    let is_registered = |root: &Path| registered.iter().any(|(p, _)| p == root);
    let in_meta = |root: &Path| meta.worktrees.iter().find(|w| Path::new(&w.path) == root);
    let own_key = root_key(worktree);
    let mut existing: Vec<PlanWorktree> = Vec::new();
    if let Some(mw) = in_meta(worktree) {
        existing.push(PlanWorktree {
            key: mw.key.clone(),
            root: worktree.to_path_buf(),
            recreate_id: None,
        });
    }
    for op in after
        .iter()
        .filter(|o| o.record.scope.worktrees.contains(&own_key))
    {
        for root in &op.record.scope.worktrees {
            let root = Path::new(root);
            if existing.iter().any(|w| w.root == root) || !is_registered(root) {
                continue;
            }
            // A worktree the point does not have stays out; a branch it
            // holds is then guarded by the ref-in-use check below.
            if let Some(mw) = in_meta(root) {
                existing.push(PlanWorktree {
                    key: mw.key.clone(),
                    root: root.to_path_buf(),
                    recreate_id: None,
                });
            }
        }
    }
    let plan_scope = |existing: &[PlanWorktree], refs: &BTreeSet<String>| Scope {
        worktrees: existing.iter().map(|w| root_key(&w.root)).collect(),
        refs: refs.iter().cloned().collect(),
    };
    // Worktrees of the point that are gone: recreated without checkout, with
    // the id the point registered them under. The main worktree has none.
    let mut recreate: Vec<PlanWorktree> = Vec::new();
    for mw in &meta.worktrees {
        let root = PathBuf::from(&mw.path);
        if is_registered(&root) || std::fs::symlink_metadata(&root).is_ok() {
            continue;
        }
        let id = meta
            .registered
            .iter()
            .find(|r| Path::new(&r.path) == root)
            .and_then(|r| r.id.clone());
        let Some(id) = id else {
            return Err(reject(
                plan_scope(&existing, &BTreeSet::new()),
                engine_mark,
                TmRejectReason::WorktreeUnavailable,
            ));
        };
        recreate.push(PlanWorktree {
            key: mw.key.clone(),
            root,
            recreate_id: Some(id),
        });
    }
    // A worktree Git still registers but whose folder is gone cannot be
    // brought to the point: refused before the engine waits on it.
    if existing.iter().any(|w| !is_dir(&w.root)) {
        return Err(reject(
            plan_scope(&existing, &BTreeSet::new()),
            engine_mark,
            TmRejectReason::WorktreeUnavailable,
        ));
    }
    let roots: Vec<PathBuf> = existing.iter().map(|w| w.root.clone()).collect();

    // The engine again, over every worktree of the plan, before their raw
    // events are read. Without an engine (a daemon whose engine store is not
    // open, or tests) there are no raw events: only the oplog counts.
    let mut raw_after: Vec<RawAfter> = Vec::new();
    if let Some(deps) = env.engine {
        match deps.engine.settle(repo_id, &roots, ANCHOR_SETTLE_LIMIT) {
            Some(mark) => engine_mark = mark,
            None => {
                return Err(reject(
                    plan_scope(&existing, &BTreeSet::new()),
                    engine_mark,
                    TmRejectReason::RepoBusy,
                ));
            }
        }
        // After the point: past its mark when the mark is of this
        // generation; otherwise every event counts (fails closed).
        let past_point = |seq: i64| match point.record.engine_mark {
            Some(mark) if point.record.seq >= floor => seq > mark,
            _ => true,
        };
        for (root, _) in registered.iter().filter(|(r, _)| is_dir(r)) {
            // Events the engine cannot answer for are not "no events": whose
            // work they are is unknown, so the restore waits for it.
            let Some(events) = deps.engine.raw_events(repo_id, root) else {
                return Err(reject(
                    plan_scope(&existing, &BTreeSet::new()),
                    engine_mark,
                    TmRejectReason::RepoBusy,
                ));
            };
            let raw = RawSide { events, floor };
            let wt_key = worktree_key(&registered, root, 0);
            let external = external_events_in(&ops, &snaps, &raw, &root_key(root), &wt_key);
            for (event, ext) in raw.events.into_iter().zip(external) {
                if past_point(event.seq) {
                    raw_after.push(RawAfter {
                        root: root.clone(),
                        event,
                        echo: ext.caused_by.is_some(),
                    });
                }
            }
        }
    }
    let plan_roots: Vec<&Path> = existing
        .iter()
        .chain(&recreate)
        .map(|w| w.root.as_path())
        .collect();
    let touches_plan = |root: &str| plan_roots.iter().any(|p| *p == Path::new(root));

    // The candidate branches: the point's and the current branch of each
    // worktree of the plan, the refs of the work done on them and the
    // branches their raw events name.
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for w in existing.iter().chain(&recreate) {
        if let Some(branch) = in_meta(&w.root).and_then(|mw| mw.head_branch.as_ref()) {
            candidates.insert(format!("refs/heads/{branch}"));
        }
    }
    for w in &existing {
        if let Some(branch) = head_branch(&w.root) {
            candidates.insert(format!("refs/heads/{branch}"));
        }
    }
    for op in &after {
        if op.record.scope.worktrees.iter().any(|r| touches_plan(r)) {
            candidates.extend(op.record.scope.refs.iter().cloned());
        }
    }
    for raw in &raw_after {
        if plan_roots.contains(&raw.root.as_path())
            && let Some(branch) = &raw.event.branch
        {
            candidates.insert(format!("refs/heads/{branch}"));
        }
    }
    let now: BTreeMap<String, String> = match RepoReader::open(worktree, &ReaderOptions::default())
        .and_then(|r| r.local_branches())
    {
        Ok(list) => list.into_iter().map(|b| (b.name, b.commit)).collect(),
        Err(_) => {
            return Err(reject(
                plan_scope(&existing, &BTreeSet::new()),
                engine_mark,
                TmRejectReason::WorktreeUnavailable,
            ));
        }
    };
    // Only branches the point has, and only those that differ: one the
    // point does not have is never deleted, and `refs/stash` never moves.
    let refs: BTreeSet<String> = candidates
        .into_iter()
        .filter(|full| {
            full.strip_prefix("refs/heads/").is_some_and(|name| {
                meta.branches
                    .get(name)
                    .is_some_and(|at_point| now.get(name) != Some(at_point))
            })
        })
        .collect();
    let scope = plan_scope(&existing, &refs);
    // A branch checked out in a worktree outside the plan never moves: it
    // would change that worktree's history under its files.
    for (root, _) in &registered {
        if !plan_roots.contains(&root.as_path())
            && let Some(branch) = head_branch(root)
            && refs.contains(&format!("refs/heads/{branch}"))
        {
            return Err(reject(scope, engine_mark, TmRejectReason::RefInUse));
        }
    }
    let kept_branches: Vec<String> = now
        .keys()
        .filter(|name| !meta.branches.contains_key(*name))
        .cloned()
        .collect();
    let not_returned_branches: Vec<String> = meta
        .branches
        .keys()
        .filter(|name| !now.contains_key(*name) && !refs.contains(&format!("refs/heads/{name}")))
        .cloned()
        .collect();

    // Whose work the restore takes back: the requester of each operation
    // after the point on the plan, and the actor of each raw Git event after
    // it that no operation of ours caused.
    let op_touches = |op: &OperationView| {
        op.record.scope.worktrees.iter().any(|r| touches_plan(r))
            || op.record.scope.refs.iter().any(|r| refs.contains(r))
    };
    // A tampered record on the plan hides whose work it is: the history
    // after the point cannot be trusted, so the point is not restorable.
    if tampered_after.iter().any(|op| op_touches(op)) {
        return Err(reject(
            scope,
            engine_mark,
            TmRejectReason::TargetUnavailable,
        ));
    }
    let mut owners: Vec<Requester> = Vec::new();
    for op in after.iter().filter(|op| op_touches(op)) {
        owners.push(op.record.requester.clone());
    }
    for raw in raw_after.iter().filter(|r| !r.echo) {
        let touches = plan_roots.contains(&raw.root.as_path())
            || raw
                .event
                .branch
                .as_ref()
                .is_some_and(|b| refs.contains(&format!("refs/heads/{b}")));
        if touches {
            owners.push(raw.event.actor.clone());
        }
    }

    // Base permission rule (ADR-TMC-005 § 2), over every owner.
    if let Err(reason) = restore_permission(&who.requester, channel, &owners) {
        return Err(reject(scope, engine_mark, reason));
    }
    // Next, in this order: confirmation (US-TMC-013), Guardrails
    // (US-TMC-021) and overlap (US-TMC-012).

    let (Some(store), Some(main_root)) = (repo.store.as_deref(), repo.main_root.clone()) else {
        return Err(reject(scope, engine_mark, TmRejectReason::Unsupported));
    };
    let Some(git) = env.git else {
        return Err(reject(scope, engine_mark, TmRejectReason::GitUnavailable));
    };
    let write = WriteContext::new(git.clone(), env.invoker.clone(), &repo.tm_dir)
        .map_err(|e| RestoreError::Internal(format!("write layer: {e}")))?;

    let apply_plan = ApplyPlan {
        target_snapshot: snapshot_id.to_owned(),
        prior_snapshot: String::new(),
        worktrees: existing.iter().chain(&recreate).cloned().collect(),
        refs: RefScope::Only(refs.clone()),
    };
    // Git preconditions before the restore's prior (ADR-TMC-005 § 4); the
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
            return Err(reject(scope, engine_mark, refusal_reason(first)));
        }
    }

    // The prior covers the worktrees that exist; the recreated ones are out
    // of the recorded scope and named in its warnings, so an undo of the
    // restore never needs them.
    let declared = StepScope {
        worktrees: roots.iter().skip(1).cloned().collect(),
        refs: refs.iter().cloned().collect(),
    };
    let req = ProtectedRequest {
        kind: OperationKind::Restore,
        scope: scope.clone(),
        worktree_paths: roots.clone(),
        who: who.clone(),
        channel,
        confirmed: false,
        target: Target::Snapshot(snapshot_id.to_owned()),
        warnings: recreate
            .iter()
            .map(|w| recreated_worktree_warning(&w.key))
            .collect(),
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
    // The state the restore left, while the repo is still held: its echo in
    // the engine is never taken for raw Git.
    if let Some(deps) = env.engine {
        let id = match &ran {
            Ok(o) => Some(o.operation_id.as_str()),
            Err(ProtectedError::Step { operation_id, .. }) => Some(operation_id.as_str()),
            Err(_) => None,
        };
        if let Some(id) = id {
            let left: Vec<PathBuf> = existing
                .iter()
                .chain(&recreate)
                .map(|w| w.root.clone())
                .filter(|r| is_dir(r))
                .collect();
            await_moved_branches(deps, repo_id, &left, &now, engine_mark);
            anchor(deps, repo_id, &left, id);
        }
    }
    match ran {
        Ok(outcome) => Ok(RestoreDone {
            operation_id: outcome.operation_id,
            prior_snapshot_id: outcome.prior.snapshot_id,
            target_snapshot_id: snapshot_id.to_owned(),
            worktrees: roots,
            recreated: recreate.into_iter().map(|w| w.root).collect(),
            refs: refs.into_iter().collect(),
            kept_branches,
            not_returned_branches,
            report: match step.result {
                Some(Ok(report)) => report,
                _ => ApplyReport::default(),
            },
        }),
        Err(ProtectedError::Prior {
            reason,
            operation_id,
            ..
        }) => Err(RestoreError::Prior {
            reason,
            operation_id,
        }),
        Err(ProtectedError::Oplog(e)) => Err(RestoreError::Internal(format!("oplog: {e}"))),
        Err(ProtectedError::Step {
            operation_id,
            message,
        }) => match step.result {
            Some(Err(ApplyError::Rejected(refusals))) => Err(RestoreError::Rejected {
                reason: refusals
                    .first()
                    .map_or(TmRejectReason::Unsupported, refusal_reason),
                operation_id: Some(operation_id),
            }),
            _ => Err(RestoreError::Interrupted {
                operation_id,
                message,
            }),
        },
    }
}
