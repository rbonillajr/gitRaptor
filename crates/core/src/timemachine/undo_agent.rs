//! The undo of one agent's work: the set of its operations in a period and what each one
//! changed, subject by subject.
//!
//! Every owner comes from one read of the current attribution (`SessionAttribution`), taken with
//! the repo held: what `timeline --agent X` shows is what `undo --agent X` takes back. The undo
//! writes only the subjects the set changed, each back to its value before the oldest change of
//! the set that touched it; anything of the set that cannot be undone on its own refuses the whole
//! request (NFR-01).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_git::tm_write::files::Kind;
use gitraptor_git::tm_write::worktree::HeadValue;
use gitraptor_git::{MAX_PATH_BYTES, Oid, ReadError, ReaderOptions, RepoReader, TreeChange};

use super::agent_selector::AgentSelector;
use super::apply::{PathTarget, PlanWorktree, SelectiveTarget};
use super::confirm::{Confirmation, PlanFacts, PlanKind};
use super::engine::{RawGitEvent, SessionAttribution, is_undoable};
use super::oplog::{OpRef, OperationKind, Oplog, Requester, Scope, StackScope};
use super::overlap::{self, OverlapReport, SelectedInput};
use super::protected::{registered_worktrees, worktree_key};
use super::store::SnapshotStore;
use super::undo::{
    FUTURE_MARGIN_MS, OverlapRefusal, PlanRefusal, Planned, RawSide, Undone, external_events,
    head_branch, lock, now_ms,
};
use crate::channel::requester::Who;

/// What an operation changes: a path of a worktree, a ref, or a worktree's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    Path { worktree_key: String, path: String },
    Ref(String),
    Head { worktree_key: String },
}

/// The value of a subject; `None` is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Entry {
        file: Option<(Kind, Oid)>,
        index: Option<(Kind, Oid)>,
    },
    Ref(Option<Oid>),
    Head(HeadValue),
}

/// One operation's change: each subject's value before and after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpChange {
    pub op: OpRef,
    pub before: BTreeMap<Subject, Value>,
    pub after: BTreeMap<Subject, Value>,
}

/// How long reading the changes of the set from the repo may take.
const CHANGE_READ_LIMIT: Duration = Duration::from_secs(10);

/// The selective part of a planned undo by agent: what the applier writes and what the overlap
/// check compares in phase 2.
#[derive(Debug, Clone)]
pub(super) struct SelectivePlan {
    pub(super) target: SelectiveTarget,
    /// Per subject: the agent's final value and the engine mark of its last change.
    pub(super) finals: BTreeMap<Subject, (Value, i64)>,
    /// Per branch: the mark of the base of the agent's first change on it.
    pub(super) first_base: BTreeMap<String, i64>,
    pub(super) worktree_key: String,
}

/// What [`plan`] reads besides the repo it holds.
pub(super) struct AgentPlanInput<'a, 'c> {
    pub(super) oplog: &'a Mutex<Oplog>,
    pub(super) store: Option<&'a SnapshotStore>,
    pub(super) common_dir: &'a Path,
    pub(super) worktree: &'a Path,
    pub(super) who: &'a Who,
    pub(super) channel: super::oplog::Channel,
    pub(super) confirm: &'a Confirmation<'c>,
    pub(super) raw: &'a RawSide,
    /// The one read of the current attribution; `None` without an engine.
    pub(super) sessions: Option<&'a SessionAttribution>,
    /// The text of `--agent`.
    pub(super) selector: &'a str,
    pub(super) since_ms: i64,
    /// Where the overlap check's budget ends.
    pub(super) deadline: Instant,
}

/// The current owner of a raw event: what the one attribution read says of its session;
/// unattributed without one.
fn event_owner(event: &RawGitEvent, sessions: &SessionAttribution) -> Requester {
    match &event.actor {
        Requester::Agent { session_id, .. } => sessions
            .0
            .get(session_id)
            .cloned()
            .unwrap_or(Requester::Unattributed),
        Requester::Unattributed => Requester::Unattributed,
    }
}

/// The owner of an oplog operation, with the timeline's rule: the current attribution of a
/// protected operation whose session is known, the frozen requester otherwise.
fn operation_owner(
    kind: OperationKind,
    requester: &Requester,
    sessions: &SessionAttribution,
) -> Requester {
    if kind == OperationKind::Protected
        && let Requester::Agent { session_id, .. } = requester
        && let Some(current) = sessions.0.get(session_id)
    {
        return current.clone();
    }
    requester.clone()
}

/// Plans the undo of what the agent `input.selector` names did in the worktree since
/// `input.since_ms` (steps 2–8 of the design). A refusal carries the scope and set to record it
/// under; a refusal before the set (no attribution, an agent naming another) records none.
pub(super) fn plan(input: &AgentPlanInput<'_, '_>) -> Result<Planned, PlanRefusal> {
    let key = input.worktree.to_string_lossy().into_owned();
    let own_scope = Scope {
        worktrees: vec![key.clone()],
        refs: Vec::new(),
    };
    let reject = |reason, scope: &Scope, set: &[OpRef]| (reason, scope.clone(), set.to_vec(), None);

    // No attribution, nothing is chosen (fail closed).
    let Some(sessions) = input.sessions else {
        return Err(reject(
            TmRejectReason::AttributionUnavailable,
            &own_scope,
            &[],
        ));
    };
    let selector = AgentSelector::resolve(input.selector, sessions);
    // An agent that names another is refused before the set exists: the answer must not tell
    // whether the other has work.
    if let Requester::Agent { session_id, .. } = &input.who.requester {
        let current = sessions.0.get(session_id).unwrap_or(&input.who.requester);
        if !selector.names_requester(current) {
            return Err(reject(TmRejectReason::OtherActor, &own_scope, &[]));
        }
    }

    let registered = registered_worktrees(input.common_dir).unwrap_or_default();
    let wt_key = worktree_key(&registered, input.worktree, 0);
    let log = lock(input.oplog);
    let external = external_events(&log, input.raw, &key, &wt_key);
    let stack = log
        .undo_stack_in(
            &StackScope::Worktree(key.clone()),
            &external,
            input.raw.floor,
        )
        .map_err(|_| reject(TmRejectReason::TargetUnavailable, &own_scope, &[]))?;
    // The set: what is still done in the worktree, with a moment in the period and an owner the
    // selector picks. Unattributed work, other agents' and a gap's never enter.
    let latest = now_ms().saturating_add(FUTURE_MARGIN_MS);
    let in_period = |at: i64| input.since_ms <= at && at <= latest;
    let mut undone: Vec<Undone> = Vec::new();
    for op in stack.done() {
        match op {
            OpRef::GitEvent(seq) => {
                let Some(event) = input.raw.events.iter().find(|e| e.seq == *seq) else {
                    continue;
                };
                if event.in_gap || !in_period(event.observed_ms) {
                    continue;
                }
                let owner = event_owner(event, sessions);
                if selector.selects(&owner) {
                    undone.push(Undone {
                        op: op.clone(),
                        id: format!("git-event-{seq}"),
                        subtype: Some(event.kind.as_str().to_owned()),
                        requester: owner,
                        at_ms: None,
                    });
                }
            }
            OpRef::Oplog(id) => {
                let Some(view) = log.operation(id).ok().flatten() else {
                    continue;
                };
                if !in_period(view.record.recorded_ms) {
                    continue;
                }
                let owner = operation_owner(view.record.kind, &view.record.requester, sessions);
                if selector.selects(&owner) {
                    undone.push(Undone {
                        op: op.clone(),
                        id: view.record.operation_id.clone(),
                        subtype: view.record.subtype.clone(),
                        requester: owner,
                        at_ms: Some(view.record.recorded_ms),
                    });
                }
            }
        }
    }
    let snaps = log.snapshots(&super::oplog::SnapshotFilter::default());
    drop(log);
    let set: Vec<OpRef> = undone.iter().map(|u| u.op.clone()).collect();
    if set.is_empty() {
        return Err(reject(TmRejectReason::NothingToUndo, &own_scope, &[]));
    }

    // Each operation of the set: undoable on its own, and its change.
    let read_until = Instant::now() + CHANGE_READ_LIMIT;
    let reader = RepoReader::open(input.worktree, &ReaderOptions::default())
        .map_err(|_| reject(TmRejectReason::TargetUnavailable, &own_scope, &set))?;
    let mut changes: Vec<(OpChange, i64)> = Vec::with_capacity(set.len());
    let mut tips: Vec<String> = Vec::new();
    for u in &undone {
        let OpRef::GitEvent(seq) = &u.op else {
            // An operation of the oplog (a restore, a protected command) has no change of its
            // own to read here: it is not undone on its own.
            return Err(reject(TmRejectReason::NotSelectable, &own_scope, &set));
        };
        let Some(event) = input.raw.events.iter().find(|e| e.seq == *seq) else {
            return Err(reject(TmRejectReason::TargetUnavailable, &own_scope, &set));
        };
        if !is_undoable(event.kind) {
            return Err(reject(TmRejectReason::RawGitNotCovered, &own_scope, &set));
        }
        // An inferred worktree is not known to be this one's.
        if event.inferred {
            return Err(reject(TmRejectReason::NotSelectable, &own_scope, &set));
        }
        let change = match event.kind {
            GitEventKind::Commit | GitEventKind::Merge | GitEventKind::Rebase => {
                commit_change(&reader, event, &wt_key, read_until)
                    .map_err(|reason| reject(reason, &own_scope, &set))?
            }
            // A reset or a branch move has no commits of its own to diff.
            _ => return Err(reject(TmRejectReason::NotSelectable, &own_scope, &set)),
        };
        tips.extend(event.old_commit.iter().cloned());
        changes.push((change, *seq));
    }

    // The target of each subject: its value before the oldest change that touched it; the
    // agent's final value: after the most recent one.
    let mut target: BTreeMap<Subject, Value> = BTreeMap::new();
    let mut finals: BTreeMap<Subject, (Value, i64)> = BTreeMap::new();
    let mut first_base: BTreeMap<String, i64> = BTreeMap::new();
    for (change, mark) in changes {
        for (subject, value) in change.before {
            if let Subject::Ref(full) = &subject {
                first_base.entry(full.clone()).or_insert(mark - 1);
            }
            target.entry(subject).or_insert(value);
        }
        for (subject, value) in change.after {
            finals.insert(subject, (value, mark));
        }
    }
    let selective = selective_target(target)
        .ok_or_else(|| reject(TmRejectReason::NotSelectable, &own_scope, &set))?;
    let scope = Scope {
        worktrees: vec![key.clone()],
        refs: selective.refs.keys().cloned().collect(),
    };
    let refuse = |reason| reject(reason, &scope, &set);

    // The objects of the targets are in the store, copied from the repo as an anchor does: the
    // redo and the applier read them there.
    let store = input
        .store
        .ok_or_else(|| refuse(TmRejectReason::TargetUnavailable))?;
    if !tips.is_empty() {
        store
            .anchor(input.worktree, &tips)
            .map_err(|_| refuse(TmRejectReason::TargetUnavailable))?;
    }

    // Permissions of every owner, then the confirmation of this very plan: its ids and its
    // subjects with their targets.
    let mut owners: Vec<Requester> = Vec::new();
    for u in &undone {
        if !owners.contains(&u.requester) {
            owners.push(u.requester.clone());
        }
    }
    let oldest = &undone[0];
    let facts = PlanFacts {
        kind: PlanKind::Undo,
        worktree: &key,
        requester: &input.who.requester,
        channel: input.channel,
        target_snapshot: "",
        undone_id: Some(&oldest.id),
        undone_subtype: oldest.subtype.as_deref(),
        scope: &scope,
        owners: &owners,
    };
    let subjects = hashed_subjects(&selective);
    let taken_back: Vec<&str> = undone
        .iter()
        .map(|u| u.id.as_str())
        .chain(subjects.iter().map(String::as_str))
        .collect();
    let confirmed = input
        .confirm
        .gate_over(
            super::restore::restore_permission(&input.who.requester, input.channel, &owners),
            &facts,
            &taken_back,
        )
        .map_err(refuse)?;

    // Phase 1 of the overlap check, against the latest clean point, for the subjects whose last
    // change is older than it (D8); the rest wait for phase 2.
    let point = snaps
        .as_ref()
        .ok()
        .and_then(|s| overlap::last_clean_point(s, &wt_key, input.raw));
    let report = match (&snaps, &point) {
        (Err(_), _) => OverlapReport {
            unverified: true,
            ..OverlapReport::default()
        },
        (Ok(_), None) => OverlapReport::default(),
        (Ok(_), Some((id, point_mark))) => {
            let older: BTreeMap<Subject, (Value, i64)> = finals
                .iter()
                .filter(|(_, (_, mark))| mark < point_mark)
                .map(|(s, v)| (s.clone(), v.clone()))
                .collect();
            let events: Vec<RawGitEvent> = input
                .raw
                .events
                .iter()
                .filter(|e| e.seq <= *point_mark)
                .cloned()
                .collect();
            overlap::check_selected(
                store,
                &SelectedInput {
                    worktree_key: &wt_key,
                    finals: &older,
                    first_base: &first_base,
                    set: &set,
                    events: &events,
                    last_point: Some(id),
                    deadline: input.deadline,
                },
            )
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
            set.clone(),
            Some(OverlapRefusal {
                report: Box::new(report),
                undone: Box::new(undone[0].clone()),
            }),
        ));
    }

    // A branch of the target checked out in another worktree never moves under it.
    for (root, _) in &registered {
        if root.as_path() != input.worktree
            && let Some(branch) = head_branch(input.common_dir, root)
            && selective.refs.contains_key(&format!("refs/heads/{branch}"))
        {
            return Err(refuse(TmRejectReason::RefInUse));
        }
    }

    let refs: BTreeSet<String> = selective.refs.keys().cloned().collect();
    Ok(Planned {
        undone,
        target: String::new(),
        scope,
        worktrees: vec![PlanWorktree {
            key: wt_key.clone(),
            root: PathBuf::from(&key),
            recreate_id: None,
            keep: BTreeSet::new(),
        }],
        refs,
        warnings: Vec::new(),
        confirmed,
        overlap: report,
        selective: Some(SelectivePlan {
            target: selective,
            finals,
            first_base,
            worktree_key: wt_key,
        }),
    })
}

/// The change of a commit, merge or rebase event: the entries its tree difference touches, and
/// its branch (or the worktree's detached `HEAD`).
fn commit_change(
    reader: &RepoReader,
    event: &RawGitEvent,
    wt_key: &str,
    deadline: Instant,
) -> Result<OpChange, TmRejectReason> {
    let (Some(old), Some(new)) = (&event.old_commit, &event.new_commit) else {
        // The first commit of an orphan branch, or an event without its commits.
        return Err(TmRejectReason::NotSelectable);
    };
    let (Some(old_id), Some(new_id)) = (Oid::from_hex(old), Oid::from_hex(new)) else {
        return Err(TmRejectReason::NotSelectable);
    };
    let entries = reader
        .commit_tree_diff(old, new, deadline)
        .map_err(|e| match e {
            // A submodule or an entry of no kind cannot be written on its own.
            ReadError::InvalidInput(_) => TmRejectReason::NotSelectable,
            _ => TmRejectReason::TargetUnavailable,
        })?;
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for TreeChange { path, old, new } in entries {
        // A path that cannot be written as it is is never cut: the request is refused.
        let path = String::from_utf8(path).map_err(|_| TmRejectReason::NotSelectable)?;
        if path.len() > MAX_PATH_BYTES {
            return Err(TmRejectReason::NotSelectable);
        }
        let subject = Subject::Path {
            worktree_key: wt_key.to_owned(),
            path,
        };
        before.insert(
            subject.clone(),
            Value::Entry {
                file: old,
                index: old,
            },
        );
        after.insert(
            subject,
            Value::Entry {
                file: new,
                index: new,
            },
        );
    }
    match &event.branch {
        Some(branch) => {
            let full = format!("refs/heads/{branch}");
            before.insert(Subject::Ref(full.clone()), Value::Ref(Some(old_id)));
            after.insert(Subject::Ref(full), Value::Ref(Some(new_id)));
        }
        None => {
            let head = Subject::Head {
                worktree_key: wt_key.to_owned(),
            };
            before.insert(head.clone(), Value::Head(HeadValue::Detached(old_id)));
            after.insert(head, Value::Head(HeadValue::Detached(new_id)));
        }
    }
    Ok(OpChange {
        op: OpRef::GitEvent(event.seq),
        before,
        after,
    })
}

/// The applier's form of the targets; `None` when a subject has a value of another shape.
fn selective_target(target: BTreeMap<Subject, Value>) -> Option<SelectiveTarget> {
    let mut out = SelectiveTarget::default();
    for (subject, value) in target {
        match (subject, value) {
            (Subject::Path { worktree_key, path }, Value::Entry { file, index }) => {
                out.paths
                    .entry(worktree_key)
                    .or_default()
                    .insert(path, PathTarget { file, index });
            }
            (Subject::Ref(full), Value::Ref(tip)) => {
                out.refs.insert(full, tip);
            }
            (Subject::Head { worktree_key }, Value::Head(head)) => {
                out.head.insert(worktree_key, head);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Each subject with its target, as text for the plan's hash: another target is another plan.
fn hashed_subjects(s: &SelectiveTarget) -> Vec<String> {
    let entry = |e: &Option<(Kind, Oid)>| match e {
        Some((kind, id)) => format!("{kind:?}:{id}"),
        None => "absent".to_owned(),
    };
    let mut out = Vec::new();
    for (key, paths) in &s.paths {
        for (path, to) in paths {
            out.push(format!(
                "path:{key}:{path}={}/{}",
                entry(&to.file),
                entry(&to.index)
            ));
        }
    }
    for (name, tip) in &s.refs {
        out.push(format!(
            "ref:{name}={}",
            tip.map_or_else(|| "absent".to_owned(), |t| t.to_string())
        ));
    }
    for (key, head) in &s.head {
        out.push(format!("head:{key}={head:?}"));
    }
    out
}

/// Whether `path` of worktree `key` is left out of the point whose exclusions are `exclusions`
/// (`<key>:<path>`): the point has no copy of it, so a redo could not bring it back.
pub(super) fn is_excluded(exclusions: &[super::oplog::Exclusion], key: &str, path: &str) -> bool {
    let prefix = format!("{key}:");
    exclusions
        .iter()
        .filter_map(|e| e.path.strip_prefix(&prefix))
        .map(|e| e.trim_end_matches('/'))
        .filter(|e| !e.is_empty())
        .any(|e| {
            path == e
                || path
                    .strip_prefix(e)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

#[cfg(test)]
#[path = "undo_agent_tests.rs"]
mod tests;
