//! Whether an undo would overwrite later work of another actor (ADR-TMC-005 § 5).
//!
//! Phase 1 ([`check`]) runs while the undo is planned, over the clean points after the base of
//! the undone change; phase 2 ([`check_tail`]) covers the tail from the last point to the undo's
//! prior, with the repo held. Both are pure over what they receive and fail closed: a point or
//! object that cannot be read in time is `unverified`, never "no overlap".
//!
//! Every path and branch the undo would write is classified (D5, D6): a **conflict** when the
//! undone change touched it (the target and the base differ there) and a later segment with an
//! actor outside the reference touched it too; **kept** when only that later actor touched it;
//! **written** otherwise. A segment that changes a file or the index counts `Unattributed`
//! among its actors: an edit is not an engine event and the MVP has no per-file attribution.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use super::apply::PlanWorktree;
use super::oplog::{OpRef, OperationState, OperationView, Requester, SnapshotLevel, SnapshotView};
use super::prior_point::{PointFacts, clean_mask, settled_anchors};
use super::store::{CaptureError, PointChanges, SnapshotStore};
use super::undo::{RawSide, external_events_in};

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "overlap_hardening_tests.rs"]
mod hardening_tests;

/// What the overlap check reads.
#[derive(Debug, Clone, Copy)]
pub struct OverlapInput<'a> {
    /// The reference actors (one in the undo): their own later work never overlaps.
    pub reference: &'a [Requester],
    /// What is taken back (one in the undo).
    pub undone: &'a [OpRef],
    /// T: the snapshot the undo returns to.
    pub target: &'a str,
    /// Key and root of each worktree in scope; the first is the undo's own.
    pub worktrees: &'a [PlanWorktree],
    /// Full names of the branches in scope.
    pub refs: &'a BTreeSet<String>,
    pub ops: &'a [OperationView],
    pub snaps: &'a [SnapshotView],
    /// Raw events of each registered worktree, by root.
    pub raw: &'a HashMap<PathBuf, RawSide>,
    pub deadline: Instant,
}

/// What the overlap check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlapReport {
    pub conflicts: Vec<Conflict>,
    /// Worktree key → paths another actor changed that the undone change did not touch.
    pub kept: BTreeMap<String, BTreeSet<String>>,
    /// Full names of the branches another actor moved that the undone change did not.
    pub kept_refs: BTreeSet<String>,
    /// Where the tail starts (phase 2).
    pub last_point: Option<String>,
    /// The check could not be done: the undo is refused as `overlap-unverified`.
    pub unverified: bool,
}

impl OverlapReport {
    fn unverified() -> Self {
        Self {
            unverified: true,
            ..Self::default()
        }
    }

    /// The undo must stop: a conflict, or nothing could be checked.
    pub fn stops(&self) -> bool {
        self.unverified || !self.conflicts.is_empty()
    }
}

/// A path or branch the undone change touched and another actor touched afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub subject: Subject,
    pub actor: Requester,
    /// Wall time (ms since the epoch) of the point that first showed the later change.
    pub seen_ms: i64,
}

/// What a conflict is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subject {
    Path {
        worktree_key: String,
        path: String,
    },
    /// A branch, by its full name (`refs/heads/<name>`).
    Ref(String),
}

/// Phase 1: classifies every path and branch the undo would write against the clean points
/// after the base of the undone change.
pub fn check(store: &SnapshotStore, input: &OverlapInput<'_>) -> OverlapReport {
    check_with(store, input, true)
}

/// [`check`] when the caller knows whether `input.raw` holds the events of every registered
/// worktree. Without them an unattributed reference would take another agent's unseen work for
/// its own: the report is `unverified` (an agent reference already counts what it cannot see
/// as another actor's).
pub fn check_with(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    events_complete: bool,
) -> OverlapReport {
    if blind(input, events_complete) || Instant::now() >= input.deadline {
        return OverlapReport::unverified();
    }
    let Some(base) = base_point(input) else {
        return OverlapReport::unverified();
    };
    let mut chain = vec![base];
    chain.extend(clean_points_after(input, &base));
    let segments: Vec<Segment<'_>> = chain
        .windows(2)
        .map(|w| Segment {
            from: w[0].id,
            to: w[1].id,
            lo: w[0].mark,
            hi: w[1].mark,
            seen_ms: w[1].recorded_ms,
        })
        .collect();
    let last = chain.last().map_or(base.id, |p| p.id);
    let mut report = if segments.is_empty() {
        OverlapReport::default()
    } else {
        match classify(store, input, &base, &segments, last) {
            Ok(report) => report,
            Err(_) => return OverlapReport::unverified(),
        }
    };
    report.last_point = Some(last.to_owned());
    report
}

/// Phase 2: the same check for the tail (last point of phase 1 → `prior`), with the raw events
/// up to `intent_mark`, merged into `phase1`.
pub fn check_tail(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    phase1: &OverlapReport,
    prior: &str,
    intent_mark: i64,
) -> OverlapReport {
    check_tail_with(store, input, phase1, prior, intent_mark, true)
}

/// [`check_tail`] with what [`check_with`] takes.
pub fn check_tail_with(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    phase1: &OverlapReport,
    prior: &str,
    intent_mark: i64,
    events_complete: bool,
) -> OverlapReport {
    if phase1.unverified || blind(input, events_complete) || Instant::now() >= input.deadline {
        return OverlapReport::unverified();
    }
    let Some(base) = base_point(input) else {
        return OverlapReport::unverified();
    };
    let start = phase1.last_point.as_deref().unwrap_or(base.id);
    let start_mark = point_of(input.snaps, start).map_or(base.mark, |p| p.mark);
    let seen_ms = point_of(input.snaps, prior).map_or_else(now_ms, |p| p.recorded_ms);
    let segment = Segment {
        from: start,
        to: prior,
        lo: start_mark,
        hi: intent_mark,
        seen_ms,
    };
    let tail = match classify(store, input, &base, std::slice::from_ref(&segment), prior) {
        Ok(tail) => tail,
        Err(_) => return OverlapReport::unverified(),
    };
    merge(phase1, tail, prior)
}

/// The offset of the local time zone from UTC, in seconds: a client renders the times of an
/// overlap with it ("seen at HH:MM:SS", local time). UTC if it cannot be known.
pub fn local_utc_offset_s() -> i32 {
    gitraptor_git::local_utc_offset_s()
}

/// Events are missing and the reference cannot tell an unseen actor from itself.
fn blind(input: &OverlapInput<'_>, events_complete: bool) -> bool {
    !events_complete
        && input
            .reference
            .iter()
            .any(|r| matches!(r, Requester::Unattributed))
}

/// A point of the timeline the check cuts at.
#[derive(Debug, Clone, Copy)]
struct Point<'s> {
    id: &'s str,
    seq: i64,
    mark: i64,
    recorded_ms: i64,
}

/// Two consecutive points and the event marks between them, `(lo, hi]`.
struct Segment<'s> {
    from: &'s str,
    to: &'s str,
    lo: i64,
    hi: i64,
    seen_ms: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn point_of<'s>(snaps: &'s [SnapshotView], id: &str) -> Option<Point<'s>> {
    snaps
        .iter()
        .find(|s| s.record.snapshot_id == id)
        .map(|s| Point {
            id: s.record.snapshot_id.as_str(),
            seq: s.record.seq,
            mark: s.record.engine_mark.unwrap_or(i64::MIN),
            recorded_ms: s.record.recorded_ms,
        })
}

fn usable(s: &SnapshotView) -> bool {
    s.state.is_available() && !s.tampered
}

/// Whether a snapshot holds every worktree of the check.
fn covers(s: &SnapshotView, worktrees: &[PlanWorktree]) -> bool {
    worktrees
        .iter()
        .all(|w| s.record.worktrees.iter().any(|k| k == &w.key))
}

fn as_point(s: &SnapshotView) -> Option<Point<'_>> {
    Some(Point {
        id: s.record.snapshot_id.as_str(),
        seq: s.record.seq,
        mark: s.record.engine_mark?,
        recorded_ms: s.record.recorded_ms,
    })
}

/// The raw side of the undo's own worktree (empty without an engine).
fn own_raw<'a>(input: &OverlapInput<'a>) -> Option<&'a RawSide> {
    input.worktrees.first().and_then(|w| input.raw.get(&w.root))
}

/// Facts and clean mask of every point holding the check's worktrees.
fn facts<'s>(input: &OverlapInput<'s>) -> (Vec<&'s SnapshotView>, Vec<bool>) {
    let views: Vec<&SnapshotView> = input
        .snaps
        .iter()
        .filter(|s| covers(s, input.worktrees) && s.record.engine_mark.is_some())
        .collect();
    let anchors = settled_anchors(input.snaps);
    let points: Vec<PointFacts> = views
        .iter()
        .filter_map(|s| PointFacts::of(s, &anchors))
        .collect();
    let (mut seqs, floor) = own_raw(input).map_or((Vec::new(), 0), |r| {
        (r.events.iter().map(|e| e.seq).collect(), r.floor)
    });
    seqs.sort_unstable();
    let clean = clean_mask(&points, &seqs, floor);
    (views, clean)
}

/// B, the state the undone change left (its anchor). Without one, an unattributed reference
/// uses the target: its own edits never count, and what an agent did since is kept rather than
/// written. An agent's change without B cannot be told from what came after it: `None`.
fn base_point<'s>(input: &OverlapInput<'s>) -> Option<Point<'s>> {
    // The base must be a clean point of the current generation: a snapshot of an older engine
    // generation has marks of another numbering, so its `cause_event_seq` names another event.
    let (views, clean) = facts(input);
    let floor = own_raw(input).map_or(0, |r| r.floor);
    // The generation decides which snapshots are candidates at all, so it filters before the
    // newest is chosen: a newer snapshot of an older generation must not hide this one's. Being
    // usable and clean is checked on the chosen one: a newer anchor that is not is no reason to
    // fall back to an older one, the base is then unknown.
    let generation = |s: &&SnapshotView| s.record.seq >= floor;
    let current = |s: &&SnapshotView| {
        views
            .iter()
            .zip(&clean)
            .any(|(v, c)| *c && v.record.snapshot_id == s.record.snapshot_id)
    };
    let found = match input.undone.first()? {
        OpRef::Oplog(id) => input
            .snaps
            .iter()
            .filter(|s| {
                s.record.level == SnapshotLevel::Observation
                    && s.record.cause_operation.as_deref() == Some(id.as_str())
                    && s.record.engine_mark.is_some()
            })
            .filter(generation)
            .max_by_key(|s| (s.record.engine_mark, s.record.seq))
            .filter(|s| usable(s))
            .filter(current)
            .and_then(as_point),
        OpRef::GitEvent(seq) => {
            let caused: Vec<&SnapshotView> = input
                .snaps
                .iter()
                .filter(|s| {
                    s.record.cause_event_seq == Some(*seq) && s.record.engine_mark.is_some()
                })
                .collect();
            if caused.is_empty() {
                views
                    .iter()
                    .zip(clean.iter().copied())
                    .filter(|(s, c)| *c && usable(s))
                    .filter_map(|(s, _)| as_point(s))
                    .filter(|p| p.mark >= *seq)
                    .min_by_key(|p| (p.mark, p.seq))
            } else {
                // A capture of another generation that names this seq makes the first point
                // after it ambiguous: without one of the current generation there is no base.
                caused
                    .into_iter()
                    .filter(generation)
                    .max_by_key(|s| (s.record.engine_mark, s.record.seq))
                    .filter(|s| usable(s))
                    .filter(current)
                    .and_then(as_point)
            }
        }
    };
    if found.is_some() {
        return found;
    }
    let unattributed = input
        .reference
        .iter()
        .all(|r| matches!(r, Requester::Unattributed));
    if unattributed {
        point_of(input.snaps, input.target)
    } else {
        None
    }
}

/// The clean, available points after `base`, oldest first. A point that is unavailable or
/// shadowed by a prior is not a place to cut: its two segments merge.
fn clean_points_after<'s>(input: &OverlapInput<'s>, base: &Point<'_>) -> Vec<Point<'s>> {
    let (views, clean) = facts(input);
    let mut out: Vec<Point<'s>> = views
        .iter()
        .zip(clean)
        .filter(|(s, c)| *c && usable(s) && s.record.snapshot_id != base.id)
        .filter_map(|(s, _)| as_point(s))
        .filter(|p| (p.mark, p.seq) > (base.mark, base.seq))
        .collect();
    out.sort_by_key(|p| (p.mark, p.seq));
    out
}

/// Agents are the same actor by session; every unattributed change is one class (D4).
fn same_actor(a: &Requester, b: &Requester) -> bool {
    match (a, b) {
        (Requester::Unattributed, Requester::Unattributed) => true,
        (Requester::Agent { session_id: x, .. }, Requester::Agent { session_id: y, .. }) => x == y,
        _ => false,
    }
}

fn push_actor(actors: &mut Vec<Requester>, actor: &Requester) {
    if !actors.iter().any(|a| same_actor(a, actor)) {
        actors.push(actor.clone());
    }
}

/// A raw event with the actor it counts with: an echo counts with its operation's requester.
struct Event {
    seq: i64,
    branch: Option<String>,
    actor: Requester,
}

/// The events of every registered worktree, by root.
fn events_by_root(input: &OverlapInput<'_>) -> HashMap<PathBuf, Vec<Event>> {
    input
        .raw
        .iter()
        .map(|(root, side)| {
            let key = root.to_string_lossy().into_owned();
            let wt_key = input
                .worktrees
                .iter()
                .find(|w| &w.root == root)
                .map_or_else(|| key.clone(), |w| w.key.clone());
            let echoes = external_events_in(input.ops, input.snaps, side, &key, &wt_key);
            let events = side
                .events
                .iter()
                .map(|e| {
                    let by_op = echoes
                        .iter()
                        .find(|x| x.seq == e.seq)
                        .and_then(|x| x.caused_by.as_deref())
                        .and_then(|id| {
                            input
                                .ops
                                .iter()
                                .find(|o| o.record.operation_id == id)
                                .map(|o| o.record.requester.clone())
                        });
                    Event {
                        seq: e.seq,
                        branch: e.branch.clone(),
                        actor: by_op.unwrap_or_else(|| e.actor.clone()),
                    }
                })
                .collect();
            (root.clone(), events)
        })
        .collect()
}

/// The operations that ran (not only planned or rejected).
fn ran(o: &OperationView) -> bool {
    matches!(
        o.state,
        OperationState::Applying | OperationState::Finished | OperationState::Interrupted
    )
}

fn within(seq: i64, seg: &Segment<'_>) -> bool {
    seg.lo < seq && seq <= seg.hi
}

/// Who changed the files of worktree `w` in `seg`: its events, the operations on it and, when a
/// file or index path changed, `Unattributed`.
fn path_actors(
    input: &OverlapInput<'_>,
    events: &HashMap<PathBuf, Vec<Event>>,
    w: &PlanWorktree,
    seg: &Segment<'_>,
    changed: bool,
) -> Vec<Requester> {
    let mut actors = Vec::new();
    for e in events.get(&w.root).into_iter().flatten() {
        if within(e.seq, seg) {
            push_actor(&mut actors, &e.actor);
        }
    }
    let root = w.root.to_string_lossy();
    for o in input.ops.iter().filter(|o| ran(o)) {
        if within(o.record.engine_mark, seg) && o.record.scope.worktrees.iter().any(|x| *x == root)
        {
            push_actor(&mut actors, &o.record.requester);
        }
    }
    if changed {
        push_actor(&mut actors, &Requester::Unattributed);
    }
    actors
}

/// Who moved branch `short` (`full` is its ref) in `seg`: the events of any worktree that name
/// it and the operations whose scope holds it. Empty when nothing explains the move.
fn ref_actors(
    input: &OverlapInput<'_>,
    events: &HashMap<PathBuf, Vec<Event>>,
    short: &str,
    full: &str,
    seg: &Segment<'_>,
) -> Vec<Requester> {
    let mut actors = Vec::new();
    for e in events.values().flatten() {
        if within(e.seq, seg) && e.branch.as_deref() == Some(short) {
            push_actor(&mut actors, &e.actor);
        }
    }
    for o in input.ops.iter().filter(|o| ran(o)) {
        if within(o.record.engine_mark, seg) && o.record.scope.refs.iter().any(|r| r == full) {
            push_actor(&mut actors, &o.record.requester);
        }
    }
    actors
}

/// Later touches of one subject by actors outside the reference, first seen first.
type Touches = BTreeMap<Subject, Vec<(Requester, i64)>>;

fn touch(touches: &mut Touches, subject: Subject, actor: &Requester, seen_ms: i64) {
    let list = touches.entry(subject).or_default();
    if !list.iter().any(|(a, _)| same_actor(a, actor)) {
        list.push((actor.clone(), seen_ms));
    }
}

fn changes(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    from: &str,
    to: &str,
    key: &str,
) -> Result<PointChanges, CaptureError> {
    if from == to {
        return Ok(PointChanges::default());
    }
    store.changed_between(from, to, key, input.deadline)
}

/// Classifies what differs between the target and `last` over `segments`, with `base` as the
/// state the undone change left. Any read that fails or runs out of time is an `Err`.
fn classify(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    base: &Point<'_>,
    segments: &[Segment<'_>],
    last: &str,
) -> Result<OverlapReport, CaptureError> {
    let events = events_by_root(input);
    let outside = |actor: &Requester| !input.reference.iter().any(|r| same_actor(r, actor));
    let mut touches = Touches::new();
    let mut undone_paths: BTreeSet<(String, String)> = BTreeSet::new();
    let mut undone_refs: BTreeSet<String> = BTreeSet::new();
    let mut final_refs: Option<BTreeSet<String>> = None;
    let mut seg_refs: Vec<Option<BTreeSet<String>>> = segments.iter().map(|_| None).collect();

    for w in input.worktrees {
        let by_undone = changes(store, input, input.target, base.id, &w.key)?;
        undone_paths.extend(by_undone.paths.into_iter().map(|p| (w.key.clone(), p)));
        undone_refs.extend(by_undone.refs);
        let to_last = changes(store, input, input.target, last, &w.key)?;
        final_refs.get_or_insert(to_last.refs);
        for (i, seg) in segments.iter().enumerate() {
            let ch = changes(store, input, seg.from, seg.to, &w.key)?;
            let changed: Vec<&String> = ch
                .paths
                .iter()
                .filter(|p| to_last.paths.contains(*p))
                .collect();
            if !ch.paths.is_empty() {
                let actors = path_actors(input, &events, w, seg, true);
                for path in changed {
                    for actor in actors.iter().filter(|a| outside(a)) {
                        let subject = Subject::Path {
                            worktree_key: w.key.clone(),
                            path: path.clone(),
                        };
                        touch(&mut touches, subject, actor, seg.seen_ms);
                    }
                }
            }
            seg_refs[i].get_or_insert(ch.refs);
        }
    }

    let final_refs = final_refs.unwrap_or_default();
    for full in input.refs {
        let Some(short) = full.strip_prefix("refs/heads/") else {
            continue;
        };
        if !final_refs.contains(short) {
            continue;
        }
        for (seg, refs) in segments.iter().zip(&seg_refs) {
            if !refs.as_ref().is_some_and(|r| r.contains(short)) {
                continue;
            }
            let mut actors = ref_actors(input, &events, short, full, seg);
            if actors.is_empty() {
                // A move no event or operation explains: `Unattributed`. Unattributed is one class
                // (D4), so for an unattributed reference it cannot be told from the reference's
                // own work: the check cannot be done.
                if !outside(&Requester::Unattributed) {
                    return Err(CaptureError::InvalidInput(
                        "a branch moved without an event that explains it".into(),
                    ));
                }
                actors.push(Requester::Unattributed);
            }
            for actor in actors.iter().filter(|a| outside(a)) {
                touch(&mut touches, Subject::Ref(full.clone()), actor, seg.seen_ms);
            }
        }
    }

    let mut report = OverlapReport::default();
    for (subject, actors) in touches {
        let by_undone = match &subject {
            Subject::Path { worktree_key, path } => {
                undone_paths.contains(&(worktree_key.clone(), path.clone()))
            }
            Subject::Ref(full) => full
                .strip_prefix("refs/heads/")
                .is_some_and(|short| undone_refs.contains(short)),
        };
        if by_undone {
            report
                .conflicts
                .extend(actors.into_iter().map(|(actor, seen_ms)| Conflict {
                    subject: subject.clone(),
                    actor,
                    seen_ms,
                }));
        } else {
            match subject {
                Subject::Path { worktree_key, path } => {
                    report.kept.entry(worktree_key).or_default().insert(path);
                }
                Subject::Ref(full) => {
                    report.kept_refs.insert(full);
                }
            }
        }
    }
    Ok(report)
}

/// Phase 1 and the tail together: a subject in conflict in either is never kept.
fn merge(phase1: &OverlapReport, tail: OverlapReport, prior: &str) -> OverlapReport {
    let mut out = phase1.clone();
    for c in tail.conflicts {
        let known = out
            .conflicts
            .iter()
            .any(|k| k.subject == c.subject && same_actor(&k.actor, &c.actor));
        if !known {
            out.conflicts.push(c);
        }
    }
    for (key, paths) in tail.kept {
        out.kept.entry(key).or_default().extend(paths);
    }
    out.kept_refs.extend(tail.kept_refs);
    let in_conflict: Vec<Subject> = out.conflicts.iter().map(|c| c.subject.clone()).collect();
    for subject in &in_conflict {
        match subject {
            Subject::Path { worktree_key, path } => {
                if let Some(paths) = out.kept.get_mut(worktree_key) {
                    paths.remove(path);
                }
            }
            Subject::Ref(full) => {
                out.kept_refs.remove(full);
            }
        }
    }
    out.kept.retain(|_, paths| !paths.is_empty());
    out.conflicts.sort_by(|a, b| a.subject.cmp(&b.subject));
    out.last_point = Some(prior.to_owned());
    out.unverified = false;
    out
}
