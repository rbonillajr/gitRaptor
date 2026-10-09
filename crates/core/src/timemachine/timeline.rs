//! The timeline of a repo: what changed, when and who did it (US-TMC-006, DS-US-TMC-006 T003).
//!
//! A read that crosses two sources and writes nothing: the oplog (operations and snapshots) and
//! the engine's history (raw Git events, with their current actor). [`build_timeline`] is the
//! assembly; [`fill_files`] adds the changed paths afterwards, once the list is cut, so Git is
//! only read for what is shown (ADR-GRP-009).
//!
//! An operation of GitRaptor and the raw events it caused are one entry, the operation
//! (`external_events`, the criterion of the undo stack, ADR-TMC-003 § 4). The actor of an event
//! or a protected operation is the current attribution (ADR-GRP-013 § 2); the one of an undo,
//! redo or restore is its requester as recorded (D-TMC-18). There is no "human": what nobody
//! attributed stays `unattributed`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventKind, GitEventView, SessionView};
use gitraptor_api::timemachine::{
    ActedOn, Attribution, ChangedFiles, EntryOrigin, Protection, ProtectionLevel,
    TIMELINE_MAX_FILES, TimelineChannel, TimelineEntry, TimelineOperationKind,
    TimelineOperationState, TimelineResult, TimelineSource,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use gitraptor_git::{ChangedPaths, ReadError, RepoReader};

use super::manual::identity_key_root;
use super::oplog::{
    Channel, CurrentAttribution, OpRef, OperationFilter, OperationKind, OperationState,
    OperationView, Oplog, Requester, RequesterOrigin, SnapshotFilter, SnapshotLevel, SnapshotRefs,
    SnapshotView, Target,
};
use super::undo::{RawSide, external_events_in};

/// Total time a request may spend reading the paths of its entries (DS-US-TMC-006 D2, D10).
pub const FILES_BUDGET: Duration = Duration::from_millis(750);
/// Most `(old, new)` pairs whose paths are kept between requests.
const CACHE_CAPACITY: usize = 256;

/// The current actor of each agent session, resolved on every query (Q37, D8): `sessions.list`
/// already applies the attribution, so a correction shows without changes here.
#[derive(Debug, Clone, Default)]
pub struct SessionActors(HashMap<String, Actor>);

impl SessionActors {
    pub fn from_sessions(sessions: &[SessionView]) -> Self {
        Self(
            sessions
                .iter()
                .map(|s| (s.session_id.clone(), s.actor.clone()))
                .collect(),
        )
    }
}

impl CurrentAttribution for SessionActors {
    type Actor = Actor;

    fn current_actor(&self, session_id: &str) -> Option<Actor> {
        self.0.get(session_id).cloned()
    }
}

/// Who an `--agent` filter keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentFilter {
    /// Only what nobody attributed.
    Unattributed,
    /// The declared name of an agent, or its kind (`claude-code`), exactly.
    Named(String),
}

impl AgentFilter {
    /// `unattributed` is reserved: it is never a name.
    pub fn parse(text: &str) -> Self {
        if text == "unattributed" {
            Self::Unattributed
        } else {
            Self::Named(text.to_owned())
        }
    }

    fn keeps(&self, actor: &Actor) -> bool {
        match (self, actor) {
            (Self::Unattributed, Actor::Unattributed) => true,
            (Self::Named(wanted), Actor::Agent { kind, name, .. }) => {
                name.as_ref().is_some_and(|n| {
                    n.raw() == wanted || (wanted == "claude-code" && is_claude_code_name(n.raw()))
                }) || (*kind == AgentKind::ClaudeCode && wanted == "claude-code")
            }
            _ => false,
        }
    }
}

/// The names a recorded requester can carry for Claude Code: its kind (`claude-code`, what the
/// daemon records) or its display name, in any case (`Claude Code`). The recorded requester keeps
/// no kind (G4), so the name is all `--agent claude-code` has to match an undo, redo or restore.
fn is_claude_code_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("claude-code")
        || name.eq_ignore_ascii_case("claude code")
        || name.eq_ignore_ascii_case("claude_code")
}

/// What a timeline request asks for, already validated.
#[derive(Debug, Clone)]
pub struct TimelineQuery {
    pub since_ms: Option<i64>,
    pub agent: Option<AgentFilter>,
    /// Canonical root of the only worktree to keep.
    pub only_worktree: Option<String>,
    pub limit: usize,
}

/// The engine's side of a timeline: its history of the repo and what the echo needs. `None` to
/// [`build_timeline`] when it could not be read.
#[derive(Debug, Clone, Default)]
pub struct EngineSide {
    /// `events.history`, oldest first, with the actor already resolved.
    pub events: Vec<GitEventView>,
    /// The raw events and the generation floor of each worktree root, for the echo.
    pub raw: HashMap<String, RawSide>,
    /// The key of each worktree root in the snapshots (`main`, `wt-<name>`).
    pub snapshot_keys: HashMap<String, String>,
    /// From `sessions.list`: `false` is "unknown", not "no agents".
    pub detection_available: bool,
    /// The page of `events.history` came back full, before anything was filtered out of it:
    /// older events may exist that it did not carry.
    pub history_full: bool,
    /// When the oldest event of that page was observed, before any filtering.
    pub history_oldest_ms: Option<i64>,
}

/// The oplog's reads once: what a timeline needs, taken under the oplog's lock and used after
/// it is let go (the work on it is proportional to operations x snapshots).
pub struct OplogRead {
    repo_id: String,
    /// The operations of the query's window; `None` when they could not be read.
    operations: Option<Vec<OperationView>>,
    /// Every operation and snapshot, for the echo of GitRaptor's own operations.
    all_operations: Vec<OperationView>,
    all_snapshots: Vec<SnapshotView>,
    /// The offerable points; `None` when they could not be read.
    points: Option<Vec<SnapshotView>>,
}

impl OplogRead {
    /// The only reads of the oplog a timeline makes.
    pub fn read(oplog: &Oplog, refs: &dyn SnapshotRefs, query: &TimelineQuery) -> Self {
        let filter = OperationFilter {
            from_ms: query.since_ms,
            worktree: query.only_worktree.clone(),
            ..OperationFilter::default()
        };
        Self {
            repo_id: oplog.repo_id().to_owned(),
            operations: oplog.operations(&filter).ok(),
            all_operations: oplog.operations(&Default::default()).unwrap_or_default(),
            all_snapshots: oplog
                .snapshots(&SnapshotFilter::default())
                .unwrap_or_default(),
            points: oplog
                .offerable_snapshots(&SnapshotFilter::default(), refs)
                .ok(),
        }
    }
}

/// The offerable points, indexed: by id and, by worktree key, in the order the "before an event"
/// rule picks from.
struct Points<'a> {
    all: &'a [SnapshotView],
    by_id: HashMap<String, usize>,
    /// Per worktree key: `(engine_mark, seq, index)` ascending, of the points with a mark.
    by_key: HashMap<String, Vec<(i64, i64, usize)>>,
}

impl<'a> Points<'a> {
    fn new(all: &'a [SnapshotView]) -> Self {
        let mut by_id = HashMap::new();
        let mut by_key: HashMap<String, Vec<(i64, i64, usize)>> = HashMap::new();
        for (i, s) in all.iter().enumerate() {
            by_id.entry(s.record.snapshot_id.clone()).or_insert(i);
            if let Some(mark) = s.record.engine_mark {
                for key in &s.record.worktrees {
                    by_key
                        .entry(key.clone())
                        .or_default()
                        .push((mark, s.record.seq, i));
                }
            }
        }
        for list in by_key.values_mut() {
            list.sort();
        }
        Self { all, by_id, by_key }
    }

    fn get(&self, id: &str) -> Option<&SnapshotView> {
        self.by_id.get(id).map(|i| &self.all[*i])
    }

    /// The latest point of the worktree `key` before the event `seq`, in the current generation
    /// of the engine (the rule of the undo's target, without `store.verify`, D9).
    fn before_event(&self, key: &str, seq: i64, floor: i64) -> Option<&SnapshotView> {
        let list = self.by_key.get(key)?;
        let end = list.partition_point(|(mark, _, _)| *mark < seq);
        list[..end]
            .iter()
            .rev()
            .map(|(_, _, i)| &self.all[*i])
            .find(|s| s.record.seq >= floor)
    }
}

fn protection(point: Option<&SnapshotView>) -> Protection {
    match point {
        Some(s) => Protection {
            level: match s.record.level {
                SnapshotLevel::GuaranteedPrior => ProtectionLevel::GuaranteedPrior,
                SnapshotLevel::HookPrior => ProtectionLevel::HookPrior,
                SnapshotLevel::Observation => ProtectionLevel::Observation,
                SnapshotLevel::Manual => ProtectionLevel::Manual,
            },
            snapshot_id: Some(s.record.snapshot_id.clone()),
        },
        None => Protection {
            level: ProtectionLevel::None,
            snapshot_id: None,
        },
    }
}

/// A requester as it was recorded: by its name. The kind is not kept with it (G4).
pub(crate) fn recorded_actor(requester: &Requester) -> Actor {
    match requester {
        Requester::Agent { name, origin, .. } => Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new(name.clone())),
            origin: match origin {
                RequesterOrigin::Detected => AgentOrigin::Detected,
                RequesterOrigin::Registered => AgentOrigin::Registered,
            },
        },
        Requester::Unattributed => Actor::Unattributed,
    }
}

/// Who an operation shows as: undo, redo and restore as recorded; a protected operation as the
/// session is attributed now, or as recorded when the session is no longer known.
fn operation_actor(
    kind: OperationKind,
    requester: &Requester,
    actors: &SessionActors,
) -> (Actor, Attribution) {
    if kind == OperationKind::Protected {
        match requester {
            Requester::Unattributed => return (Actor::Unattributed, Attribution::Current),
            Requester::Agent { session_id, .. } => {
                if let Some(actor) = actors.current_actor(session_id) {
                    return (actor, Attribution::Current);
                }
            }
        }
    }
    (recorded_actor(requester), Attribution::Recorded)
}

fn acted_on(target: &Target) -> Vec<ActedOn> {
    match target {
        Target::None => Vec::new(),
        Target::Undo(refs) => refs
            .iter()
            .map(|r| match r {
                OpRef::Oplog(id) => ActedOn::Operation(id.clone()),
                OpRef::GitEvent(seq) => ActedOn::GitEvent(*seq),
            })
            .collect(),
        Target::Redo(undo) => vec![ActedOn::Operation(undo.clone())],
        Target::Snapshot(id) => vec![ActedOn::Snapshot(id.clone())],
    }
}

/// Sort key: when, operations before events at the same time, then the source's own order.
type Order = (i64, u8, i64);

/// Assembles the timeline of the oplog's repo, oldest entry first, cut to `query.limit`. Every
/// `files` is `Unavailable`: [`fill_files`] reads them for what is left. A source that could not
/// be read is declared in `unavailable`, never taken for "no activity".
pub fn build_timeline(
    oplog: &Oplog,
    refs: &dyn SnapshotRefs,
    query: &TimelineQuery,
    engine: Option<&EngineSide>,
    actors: &SessionActors,
    now: (i64, i32),
) -> TimelineResult {
    let read = OplogRead::read(oplog, refs, query);
    build_timeline_from(&read, query, engine, actors, now)
}

/// [`build_timeline`] over an [`OplogRead`]: no oplog, no lock.
pub fn build_timeline_from(
    read: &OplogRead,
    query: &TimelineQuery,
    engine: Option<&EngineSide>,
    actors: &SessionActors,
    now: (i64, i32),
) -> TimelineResult {
    let mut unavailable = Vec::new();
    let points = match &read.points {
        Some(all) => Points::new(all),
        None => {
            unavailable.push(TimelineSource::Operations);
            Points::new(&[])
        }
    };
    let mut entries: Vec<(Order, TimelineEntry)> = Vec::new();

    // A full page of events leaves older ones unseen: the operations older than the oldest event
    // shown would leave a silent gap, so they are dropped and the result says it is cut.
    let events_full_page = engine.is_some_and(|e| {
        e.history_full
            && e.history_oldest_ms
                .is_some_and(|oldest| query.since_ms.is_none_or(|since| oldest >= since))
    });
    let operations_floor = engine
        .filter(|_| events_full_page)
        .and_then(|e| e.history_oldest_ms);
    match &read.operations {
        Some(operations) => {
            for op in operations {
                if operations_floor.is_some_and(|floor| op.record.recorded_ms < floor) {
                    continue;
                }
                if op.tampered
                    || !matches!(
                        op.state,
                        OperationState::Applying
                            | OperationState::Finished
                            | OperationState::Interrupted
                    )
                {
                    continue;
                }
                let r = &op.record;
                let (actor, attribution) = operation_actor(r.kind, &r.requester, actors);
                if query.agent.as_ref().is_some_and(|a| !a.keeps(&actor)) {
                    continue;
                }
                let point = op.prior_snapshot.as_deref().and_then(|id| points.get(id));
                let entry = TimelineEntry {
                    id: format!("operation:{}", r.operation_id),
                    origin: EntryOrigin::Operation {
                        operation_id: r.operation_id.clone(),
                        kind: match r.kind {
                            OperationKind::Protected => TimelineOperationKind::Protected,
                            OperationKind::Undo => TimelineOperationKind::Undo,
                            OperationKind::Redo => TimelineOperationKind::Redo,
                            OperationKind::Restore => TimelineOperationKind::Restore,
                        },
                        subtype: r.subtype.clone().map(Untrusted::new),
                        state: match op.state {
                            OperationState::Applying => TimelineOperationState::Applying,
                            OperationState::Interrupted => TimelineOperationState::Interrupted,
                            _ => TimelineOperationState::Finished,
                        },
                        acted_on: acted_on(&r.target),
                    },
                    occurred_utc_ms: r.recorded_ms,
                    utc_offset_s: now.1,
                    worktrees: r.scope.worktrees.iter().map(Untrusted::new).collect(),
                    actor,
                    attribution,
                    protection: protection(point),
                    files: ChangedFiles::Unavailable,
                };
                entries.push(((r.recorded_ms, 0, r.seq), entry));
            }
        }
        None => unavailable.push(TimelineSource::Operations),
    }

    // The points an agent or the developer took by hand: one entry each, with the label as data.
    // Only the offerable ones (their ref exists and their row is complete and intact).
    if read.points.is_some() {
        for view in points
            .all
            .iter()
            .filter(|s| s.record.level == SnapshotLevel::Manual)
        {
            let r = &view.record;
            let Some(meta) = &r.manual else {
                continue;
            };
            if operations_floor.is_some_and(|floor| r.recorded_ms < floor)
                || query.since_ms.is_some_and(|since| r.recorded_ms < since)
            {
                continue;
            }
            let root = identity_key_root(&meta.worktree_key);
            if query
                .only_worktree
                .as_deref()
                .is_some_and(|only| root != Some(only))
            {
                continue;
            }
            let actor = recorded_actor(&meta.requester);
            if query.agent.as_ref().is_some_and(|a| !a.keeps(&actor)) {
                continue;
            }
            let entry = TimelineEntry {
                id: format!("manual:{}", r.snapshot_id),
                origin: EntryOrigin::ManualSnapshot {
                    snapshot_id: r.snapshot_id.clone(),
                    label: UntrustedName::new(meta.label.clone()),
                    channel: match meta.channel {
                        Channel::Cli => TimelineChannel::Cli,
                        Channel::Tui => TimelineChannel::Tui,
                        Channel::Mcp => TimelineChannel::Mcp,
                        Channel::Hook => TimelineChannel::Hook,
                    },
                },
                occurred_utc_ms: r.recorded_ms,
                utc_offset_s: now.1,
                worktrees: root.map(Untrusted::new).into_iter().collect(),
                actor,
                attribution: Attribution::Recorded,
                protection: Protection {
                    level: ProtectionLevel::Manual,
                    snapshot_id: Some(r.snapshot_id.clone()),
                },
                files: ChangedFiles::Available {
                    paths: Vec::new(),
                    total: 0,
                    first_parent: false,
                },
            };
            entries.push(((r.recorded_ms, 0, r.seq), entry));
        }
    }

    match engine {
        None => unavailable.push(TimelineSource::Events),
        Some(engine) => {
            let echoes = echoes(read, engine);
            for ev in &engine.events {
                let root = ev.worktree.raw();
                if ev.kind == GitEventKind::Reconciled
                    || echoes.get(root).is_some_and(|s| s.contains(&ev.seq))
                    || query
                        .since_ms
                        .is_some_and(|since| ev.observed_utc_ms < since)
                    || query
                        .only_worktree
                        .as_deref()
                        .is_some_and(|only| only != root)
                    || query.agent.as_ref().is_some_and(|a| !a.keeps(&ev.actor))
                {
                    continue;
                }
                let floor = engine.raw.get(root).map_or(0, |r| r.floor);
                let point = engine
                    .snapshot_keys
                    .get(root)
                    .and_then(|key| points.before_event(key, ev.seq, floor));
                let entry = TimelineEntry {
                    id: format!("event:{}", ev.seq),
                    origin: EntryOrigin::GitEvent {
                        seq: ev.seq,
                        kind: ev.kind,
                        branch: ev.details.branch.clone(),
                    },
                    occurred_utc_ms: ev.observed_utc_ms,
                    utc_offset_s: ev.utc_offset_s,
                    worktrees: vec![ev.worktree.clone()],
                    actor: ev.actor.clone(),
                    attribution: Attribution::Current,
                    protection: protection(point),
                    files: ChangedFiles::Unavailable,
                };
                entries.push(((ev.observed_utc_ms, 1, ev.seq), entry));
            }
        }
    }

    entries.sort_by_key(|(order, _)| *order);
    let over = entries.len().saturating_sub(query.limit);
    entries.drain(..over);
    TimelineResult {
        repo_id: read.repo_id.clone(),
        entries: entries.into_iter().map(|(_, e)| e).collect(),
        truncated: over > 0 || events_full_page,
        unavailable,
        detection_available: engine.is_some_and(|e| e.detection_available),
    }
}

/// The timeline as a connection without `timemachine.timeline-manual` sees it: no manual point,
/// and an event they protect shows the point as an observation, as it always did.
pub fn without_manual(result: &mut TimelineResult) {
    result
        .entries
        .retain(|e| !matches!(e.origin, EntryOrigin::ManualSnapshot { .. }));
    for entry in &mut result.entries {
        if entry.protection.level == ProtectionLevel::Manual {
            entry.protection.level = ProtectionLevel::Observation;
        }
    }
}

/// The events each worktree root has that an operation of GitRaptor caused. A worktree whose raw
/// events could not be read has none marked: its events show.
fn echoes(read: &OplogRead, engine: &EngineSide) -> HashMap<String, HashSet<i64>> {
    let mut out = HashMap::new();
    for (root, raw) in &engine.raw {
        let Some(snapshot_key) = engine.snapshot_keys.get(root) else {
            continue;
        };
        let caused = external_events_in(
            &read.all_operations,
            &read.all_snapshots,
            raw,
            root,
            snapshot_key,
        )
        .into_iter()
        .filter(|e| e.caused_by.is_some())
        .map(|e| e.seq)
        .collect();
        out.insert(root.clone(), caused);
    }
    out
}

/// What reads the paths between two commits: the Git reader, or a double.
pub trait PathsSource {
    fn changed_paths(
        &self,
        old: Option<&str>,
        new: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<ChangedPaths, ReadError>;
}

impl PathsSource for RepoReader {
    fn changed_paths(
        &self,
        old: Option<&str>,
        new: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<ChangedPaths, ReadError> {
        RepoReader::changed_paths(self, old, new, max, deadline)
    }
}

type CacheKey = (String, String, String);

/// A bounded memory of the paths between two commits, by `(repo, old, new)`: the same pair is
/// the same answer. Only successful reads are kept; the oldest goes first.
#[derive(Debug, Default)]
pub struct PathsCache {
    inner: Mutex<(HashMap<CacheKey, ChangedPaths>, VecDeque<CacheKey>)>,
}

impl PathsCache {
    fn get(&self, key: &CacheKey) -> Option<ChangedPaths> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.0.get(key).cloned()
    }

    fn put(&self, key: CacheKey, value: ChangedPaths) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.0.insert(key.clone(), value).is_none() {
            inner.1.push_back(key);
        }
        while inner.1.len() > CACHE_CAPACITY {
            if let Some(oldest) = inner.1.pop_front() {
                inner.0.remove(&oldest);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).0.len()
    }

    /// The cache every request of the daemon shares.
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<PathsCache> = OnceLock::new();
        SHARED.get_or_init(Self::default)
    }
}

/// The commits a Git event's paths come from: both, and different; or, for a first commit, only
/// the new one. Anything else has no pair (D2).
fn commit_pair(ev: &GitEventView) -> Option<(Option<&str>, &str)> {
    match (
        ev.details.old_commit.as_deref(),
        ev.details.new_commit.as_deref(),
    ) {
        (Some(old), Some(new)) if old != new => Some((Some(old), new)),
        (None, Some(new)) if ev.kind == GitEventKind::Commit => Some((None, new)),
        _ => None,
    }
}

/// Most bytes of paths (as they travel, JSON-escaped) one response carries (M-01). Well under
/// half of a frame ([`gitraptor_api::framing::MAX_MESSAGE_BYTES`]) so the entries around them
/// fit: what goes beyond it degrades to `Unavailable`, like what goes beyond the time budget.
pub const FILES_BYTES_BUDGET: usize = 384 * 1024;

/// What an entry's `files` weigh on the wire: its paths escaped, plus a fixed overhead.
fn wire_bytes(paths: &[String]) -> usize {
    32 + paths
        .iter()
        .map(|p| serde_json::to_string(p).map_or(p.len() * 6 + 2, |j| j.len()) + 1)
        .sum::<usize>()
}

/// Fills `files` of the entries from the newest to the oldest within `budget` for the whole
/// request, and within [`FILES_BYTES_BUDGET`] of paths. An entry whose paths could not be read, or
/// that a budget did not reach, stays `Unavailable`: never a zero (D2). Paths come from trees
/// only, and nothing but them is kept.
pub fn fill_files(
    result: &mut TimelineResult,
    events: &[GitEventView],
    source: Option<&dyn PathsSource>,
    cache: &PathsCache,
    budget: Duration,
) {
    let deadline = Instant::now() + budget;
    fill_files_with(
        result,
        events,
        source,
        cache,
        deadline,
        &mut || Instant::now() >= deadline,
        FILES_BYTES_BUDGET,
    );
}

/// [`fill_files`] with its clock and its byte budget explicit: `expired` says whether the time
/// budget is gone (a test drives it by calls, not by time).
fn fill_files_with(
    result: &mut TimelineResult,
    events: &[GitEventView],
    source: Option<&dyn PathsSource>,
    cache: &PathsCache,
    deadline: Instant,
    expired: &mut dyn FnMut() -> bool,
    mut bytes_left: usize,
) {
    let Some(source) = source else {
        return;
    };
    let by_seq: HashMap<i64, &GitEventView> = events.iter().map(|e| (e.seq, e)).collect();
    for entry in result.entries.iter_mut().rev() {
        let EntryOrigin::GitEvent { seq, kind, .. } = &entry.origin else {
            continue;
        };
        let kind = *kind;
        let Some((old, new)) = by_seq.get(seq).and_then(|ev| commit_pair(ev)) else {
            continue;
        };
        let key = (
            result.repo_id.clone(),
            old.unwrap_or_default().to_owned(),
            new.to_owned(),
        );
        // A hit costs no Git read: it is used even after the time budget is gone.
        let paths = match cache.get(&key) {
            Some(hit) => hit,
            None => {
                if expired() {
                    continue;
                }
                match source.changed_paths(old, new, TIMELINE_MAX_FILES, deadline) {
                    Ok(read) => {
                        cache.put(key, read.clone());
                        read
                    }
                    Err(_) => continue,
                }
            }
        };
        let cost = wire_bytes(&paths.paths);
        if cost > bytes_left {
            // The older entries are the ones that degrade.
            break;
        }
        bytes_left -= cost;
        entry.files = ChangedFiles::Available {
            paths: paths.paths.into_iter().map(Untrusted::new).collect(),
            total: paths.total,
            // Only a commit or a merge was reached from its first parent; a reset or a checkout
            // to a merge is plain `old..new`.
            first_parent: paths.first_parent
                && matches!(kind, GitEventKind::Commit | GitEventKind::Merge),
        };
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use gitraptor_api::messages::{GitEventDetails, MAX_HISTORY_PAGE};

    use super::super::oplog::{
        Channel, CompleteInfo, NewOperation, NewSnapshot, OperationTransition, Scope,
    };
    use super::*;
    use crate::profile::ProfileDirs;

    const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";
    const WT: &str = "/repo/feat-login";
    const KEY: &str = "wt-feat-login";

    struct Refs(Vec<String>);

    impl SnapshotRefs for Refs {
        fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>> {
            Ok(Some(self.0.clone()))
        }
        fn delete(&mut self, _: &[String]) -> io::Result<()> {
            Ok(())
        }
    }

    struct World {
        _tmp: tempfile::TempDir,
        dirs: ProfileDirs,
        log: Oplog,
        refs: Refs,
    }

    fn world() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
        let (log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
        World {
            _tmp: tmp,
            dirs,
            log,
            refs: Refs(Vec::new()),
        }
    }

    fn agent(name: &str, session: &str) -> Requester {
        Requester::Agent {
            name: name.into(),
            origin: RequesterOrigin::Detected,
            session_id: session.into(),
        }
    }

    fn new_op(
        kind: OperationKind,
        requester: Requester,
        target: Target,
        mark: i64,
    ) -> NewOperation {
        NewOperation {
            kind,
            subtype: (kind == OperationKind::Protected).then(|| "reset-hard".to_owned()),
            scope: Scope {
                worktrees: vec![WT.into()],
                refs: vec![],
            },
            requester,
            channel: Channel::Cli,
            confirmed: false,
            target,
            warnings: vec![],
            engine_mark: mark,
        }
    }

    impl World {
        fn snapshot(
            &mut self,
            level: SnapshotLevel,
            mark: i64,
            op: Option<&str>,
            at: i64,
        ) -> String {
            let id = self
                .log
                .begin_snapshot(
                    &NewSnapshot {
                        level,
                        worktrees: vec![KEY.into()],
                        engine_mark: Some(mark),
                        cause_operation: op.map(str::to_owned),
                        cause_event_seq: None,
                    },
                    at,
                )
                .unwrap();
            self.log
                .complete_snapshot(&id, &CompleteInfo::default(), at)
                .unwrap();
            self.refs.0.push(id.clone());
            id
        }

        /// An operation driven to `state` (`Rejected` and `Aborted` run no step).
        fn operation(&mut self, new: &NewOperation, state: OperationState, at: i64) -> String {
            let id = self.log.record_operation(new, at).unwrap();
            let t = |log: &mut Oplog, t| log.advance_operation(&id, t, at).unwrap();
            match state {
                OperationState::Intent => {}
                OperationState::Rejected => {
                    t(&mut self.log, OperationTransition::Rejected { reason: "x" })
                }
                OperationState::Aborted => {
                    t(&mut self.log, OperationTransition::Aborted { reason: "x" })
                }
                _ => {
                    let snap = self.snapshot(
                        SnapshotLevel::GuaranteedPrior,
                        new.engine_mark,
                        Some(&id),
                        at,
                    );
                    t(
                        &mut self.log,
                        OperationTransition::PriorSnapshot { snapshot_id: &snap },
                    );
                    if state != OperationState::PriorSnapshot {
                        t(&mut self.log, OperationTransition::Ready);
                    }
                    if !matches!(state, OperationState::PriorSnapshot | OperationState::Ready) {
                        t(&mut self.log, OperationTransition::Applying { step: 1 });
                    }
                    match state {
                        OperationState::Finished => t(&mut self.log, OperationTransition::Finished),
                        OperationState::Interrupted => {
                            t(&mut self.log, OperationTransition::Interrupted)
                        }
                        _ => {}
                    }
                }
            }
            id
        }

        fn timeline(
            &self,
            query: &TimelineQuery,
            engine: Option<&EngineSide>,
            actors: &SessionActors,
        ) -> TimelineResult {
            build_timeline(&self.log, &self.refs, query, engine, actors, (5_000, 3_600))
        }
    }

    fn query() -> TimelineQuery {
        TimelineQuery {
            since_ms: None,
            agent: None,
            only_worktree: None,
            limit: 50,
        }
    }

    fn event(seq: i64, kind: GitEventKind, at: i64, actor: Actor) -> GitEventView {
        GitEventView {
            repo_id: REPO.into(),
            seq,
            worktree: Untrusted::new(WT),
            kind,
            actor,
            observed_utc_ms: at,
            utc_offset_s: 7_200,
            details: GitEventDetails::default(),
            gap_id: None,
            inferred: None,
            authorship: None,
        }
    }

    fn engine(events: Vec<GitEventView>) -> EngineSide {
        EngineSide {
            raw: HashMap::from([(WT.to_owned(), RawSide::default())]),
            snapshot_keys: HashMap::from([(WT.to_owned(), KEY.to_owned())]),
            history_full: events.len() >= usize::try_from(MAX_HISTORY_PAGE).unwrap(),
            history_oldest_ms: events.iter().map(|e| e.observed_utc_ms).min(),
            events,
            detection_available: true,
        }
    }

    fn detected(kind: AgentKind) -> Actor {
        Actor::Agent {
            kind,
            name: None,
            origin: AgentOrigin::Detected,
        }
    }

    fn ids(t: &TimelineResult) -> Vec<&str> {
        t.entries.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn operations_that_did_not_run_are_not_entries() {
        let mut w = world();
        for (state, at) in [
            (OperationState::Intent, 10),
            (OperationState::PriorSnapshot, 11),
            (OperationState::Ready, 12),
            (OperationState::Rejected, 13),
            (OperationState::Aborted, 14),
        ] {
            w.operation(
                &new_op(
                    OperationKind::Protected,
                    Requester::Unattributed,
                    Target::None,
                    1,
                ),
                state,
                at,
            );
        }
        let ran = [
            OperationState::Applying,
            OperationState::Finished,
            OperationState::Interrupted,
        ]
        .map(|s| {
            w.operation(
                &new_op(
                    OperationKind::Protected,
                    Requester::Unattributed,
                    Target::None,
                    1,
                ),
                s,
                20,
            )
        });
        let t = w.timeline(&query(), Some(&engine(vec![])), &SessionActors::default());
        let mut got: Vec<_> = ids(&t).iter().map(|s| s.to_owned()).collect();
        let mut want: Vec<_> = ran.iter().map(|id| format!("operation:{id}")).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn reconciled_events_are_not_entries() {
        let w = world();
        let e = engine(vec![
            event(1, GitEventKind::Reconciled, 10, Actor::Unattributed),
            event(2, GitEventKind::Commit, 11, Actor::Unattributed),
        ]);
        let t = w.timeline(&query(), Some(&e), &SessionActors::default());
        assert_eq!(ids(&t), ["event:2"]);
    }

    #[test]
    fn ties_keep_a_stable_order() {
        let mut w = world();
        let op = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                1,
            ),
            OperationState::Finished,
            100,
        );
        let e = engine(vec![
            event(9, GitEventKind::Commit, 100, Actor::Unattributed),
            event(8, GitEventKind::Commit, 100, Actor::Unattributed),
        ]);
        let first = w.timeline(&query(), Some(&e), &SessionActors::default());
        let again = w.timeline(&query(), Some(&e), &SessionActors::default());
        assert_eq!(first, again);
        // Same instant: the operation, then the events by sequence.
        assert_eq!(
            ids(&first),
            [format!("operation:{op}").as_str(), "event:8", "event:9"]
        );
    }

    #[test]
    fn limit_keeps_the_latest_in_order_and_says_it_cut() {
        let w = world();
        let e = engine(
            (1..=5)
                .map(|i| event(i, GitEventKind::Commit, i * 10, Actor::Unattributed))
                .collect(),
        );
        let q = TimelineQuery {
            limit: 2,
            ..query()
        };
        let t = w.timeline(&q, Some(&e), &SessionActors::default());
        assert_eq!(ids(&t), ["event:4", "event:5"]);
        assert!(t.truncated);
        let all = w.timeline(&query(), Some(&e), &SessionActors::default());
        assert!(!all.truncated);
    }

    #[test]
    fn a_full_page_of_events_is_truncated() {
        let w = world();
        let page = usize::try_from(MAX_HISTORY_PAGE).unwrap();
        let e = engine(
            (1..=page as i64)
                .map(|i| event(i, GitEventKind::Commit, 100 + i, Actor::Unattributed))
                .collect(),
        );
        assert!(
            w.timeline(&query(), Some(&e), &SessionActors::default())
                .truncated
        );
        // The page reaches back before the window: nothing older to miss.
        let q = TimelineQuery {
            since_ms: Some(150),
            limit: 500,
            ..query()
        };
        assert!(
            !w.timeline(&q, Some(&e), &SessionActors::default())
                .truncated
        );
        let q = TimelineQuery {
            since_ms: Some(50),
            limit: 500,
            ..query()
        };
        assert!(
            w.timeline(&q, Some(&e), &SessionActors::default())
                .truncated
        );
    }

    #[test]
    fn an_unavailable_source_is_declared_not_hidden() {
        let w = world();
        let t = w.timeline(&query(), None, &SessionActors::default());
        assert!(t.entries.is_empty());
        assert_eq!(t.unavailable, [TimelineSource::Events]);
        assert!(!t.detection_available);
        let ok = w.timeline(&query(), Some(&engine(vec![])), &SessionActors::default());
        assert!(ok.unavailable.is_empty() && ok.entries.is_empty());
    }

    /// Guard: an undo shows its requester as recorded; a protected operation shows the session's
    /// attribution as of now (Q37, D-TMC-18).
    #[test]
    fn an_undo_entry_shows_the_recorded_requester_not_the_current_actor() {
        let mut w = world();
        let undo = w.operation(
            &new_op(
                OperationKind::Undo,
                agent("claude-1", "s1"),
                Target::Undo(vec![OpRef::GitEvent(4)]),
                1,
            ),
            OperationState::Finished,
            10,
        );
        let prot = w.operation(
            &new_op(
                OperationKind::Protected,
                agent("claude-1", "s1"),
                Target::None,
                1,
            ),
            OperationState::Finished,
            20,
        );
        let corrected = SessionActors(HashMap::from([(
            "s1".to_owned(),
            Actor::Agent {
                kind: AgentKind::Other,
                name: Some(UntrustedName::new("corrected")),
                origin: AgentOrigin::Registered,
            },
        )]));
        let t = w.timeline(&query(), Some(&engine(vec![])), &corrected);
        let by = |id: &str| {
            t.entries
                .iter()
                .find(|e| e.id == format!("operation:{id}"))
                .unwrap()
        };
        let u = by(&undo);
        assert_eq!(u.attribution, Attribution::Recorded);
        let Actor::Agent { name, origin, .. } = &u.actor else {
            panic!("{u:?}")
        };
        assert_eq!(name.as_ref().unwrap().raw(), "claude-1");
        assert_eq!(*origin, AgentOrigin::Detected);
        match &u.origin {
            EntryOrigin::Operation { kind, acted_on, .. } => {
                assert_eq!(*kind, TimelineOperationKind::Undo);
                assert_eq!(acted_on, &[ActedOn::GitEvent(4)]);
            }
            other => panic!("{other:?}"),
        }
        let p = by(&prot);
        assert_eq!(p.attribution, Attribution::Current);
        let Actor::Agent { name, .. } = &p.actor else {
            panic!("{p:?}")
        };
        assert_eq!(name.as_ref().unwrap().raw(), "corrected");
        // A session nobody knows any more: shown as recorded, never as unattributed.
        let t = w.timeline(&query(), Some(&engine(vec![])), &SessionActors::default());
        let p = t
            .entries
            .iter()
            .find(|e| e.id == format!("operation:{prot}"))
            .unwrap();
        assert_eq!(p.attribution, Attribution::Recorded);
        assert!(matches!(p.actor, Actor::Agent { .. }));
    }

    /// SEC-TMC-09: a row edited outside the daemon is neither an entry nor a point.
    #[test]
    fn a_tampered_operation_is_not_an_entry() {
        let mut w = world();
        let requester = || agent("claude-1", "s1");
        let honest = w.operation(
            &new_op(OperationKind::Protected, requester(), Target::None, 1),
            OperationState::Finished,
            10,
        );
        let edited = w.operation(
            &new_op(OperationKind::Protected, requester(), Target::None, 1),
            OperationState::Finished,
            20,
        );
        let World {
            _tmp, dirs, log, ..
        } = w;
        drop(log);
        let file = super::super::oplog::repo_dir(&dirs, REPO)
            .unwrap()
            .join(super::super::oplog::OPLOG_FILE);
        let conn = rusqlite::Connection::open(file).unwrap();
        for t in ["snapshots", "operations", "journal", "notices", "chain"] {
            for k in ["update", "delete"] {
                conn.execute_batch(&["DROP TRIGGER ", t, "_no_", k].concat())
                    .unwrap();
            }
        }
        conn.execute(
            "UPDATE operations SET requester = '{\"variant\":\"unattributed\"}' WHERE operation_id = ?1",
            [&edited],
        )
        .unwrap();
        drop(conn);
        let (log, _) = Oplog::open(&dirs, REPO, 2_000).unwrap();
        let t = build_timeline(
            &log,
            &Refs(Vec::new()),
            &query(),
            Some(&engine(vec![])),
            &SessionActors::default(),
            (0, 0),
        );
        assert_eq!(ids_owned(&t), [format!("operation:{honest}")]);
    }

    #[test]
    fn the_actor_is_resolved_on_each_query() {
        let w = world();
        let ev = event(1, GitEventKind::Commit, 10, Actor::Unattributed);
        let sessions = |name: &str| {
            SessionActors(HashMap::from([(
                "s".to_owned(),
                Actor::Agent {
                    kind: AgentKind::Other,
                    name: Some(UntrustedName::new(name)),
                    origin: AgentOrigin::Registered,
                },
            )]))
        };
        let mut w = w;
        let op = w.operation(
            &new_op(OperationKind::Protected, agent("old", "s"), Target::None, 1),
            OperationState::Finished,
            5,
        );
        let name_of = |t: &TimelineResult| match &t
            .entries
            .iter()
            .find(|e| e.id == format!("operation:{op}"))
            .unwrap()
            .actor
        {
            Actor::Agent { name, .. } => name.as_ref().unwrap().raw().to_owned(),
            Actor::Unattributed => "unattributed".to_owned(),
        };
        let e = engine(vec![ev]);
        assert_eq!(
            name_of(&w.timeline(&query(), Some(&e), &sessions("one"))),
            "one"
        );
        assert_eq!(
            name_of(&w.timeline(&query(), Some(&e), &sessions("two"))),
            "two"
        );
    }

    #[test]
    fn the_protection_is_the_latest_offerable_point_before_the_event() {
        let mut w = world();
        let early = w.snapshot(SnapshotLevel::Observation, 2, None, 10);
        let late = w.snapshot(SnapshotLevel::Observation, 5, None, 20);
        // After the event: not its protection.
        w.snapshot(SnapshotLevel::Observation, 9, None, 30);
        // Not offerable (its ref is gone): not a point.
        let gone = w.snapshot(SnapshotLevel::Observation, 6, None, 25);
        w.refs.0.retain(|id| *id != gone);
        let e = engine(vec![
            event(1, GitEventKind::Commit, 5, Actor::Unattributed),
            event(8, GitEventKind::Commit, 40, Actor::Unattributed),
        ]);
        let t = w.timeline(&query(), Some(&e), &SessionActors::default());
        assert_eq!(t.entries[0].protection.level, ProtectionLevel::None);
        assert_eq!(t.entries[0].protection.snapshot_id, None);
        assert_eq!(t.entries[1].protection.level, ProtectionLevel::Observation);
        assert_eq!(
            t.entries[1].protection.snapshot_id.as_deref(),
            Some(late.as_str())
        );
        assert_ne!(
            Some(early.as_str()),
            t.entries[1].protection.snapshot_id.as_deref()
        );
    }

    #[test]
    fn the_echo_of_an_operation_is_not_a_second_entry() {
        let mut w = world();
        let op = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                5,
            ),
            OperationState::Finished,
            10,
        );
        let mut e = engine(vec![
            event(3, GitEventKind::Commit, 5, Actor::Unattributed),
            event(6, GitEventKind::BranchUpdate, 11, Actor::Unattributed),
        ]);
        let raw = |seq| gitraptor_core_raw(seq);
        e.raw.insert(
            WT.to_owned(),
            RawSide {
                events: vec![raw(3), raw(6)],
                floor: 0,
            },
        );
        let t = w.timeline(&query(), Some(&e), &SessionActors::default());
        assert_eq!(
            ids(&t),
            ["event:3".to_owned(), format!("operation:{op}")]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
    }

    fn gitraptor_core_raw(seq: i64) -> super::super::engine::RawGitEvent {
        super::super::engine::RawGitEvent {
            seq,
            kind: GitEventKind::Commit,
            branch: None,
            actor: Requester::Unattributed,
        }
    }

    #[test]
    fn the_filters_narrow_by_agent_period_and_worktree() {
        let w = world();
        let mut other = event(3, GitEventKind::Commit, 30, detected(AgentKind::Other));
        other.worktree = Untrusted::new("/repo/other");
        let named = Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new("claude-2")),
            origin: AgentOrigin::Registered,
        };
        let e = engine(vec![
            event(1, GitEventKind::Commit, 10, Actor::Unattributed),
            event(2, GitEventKind::Commit, 20, detected(AgentKind::ClaudeCode)),
            other,
            event(4, GitEventKind::Commit, 40, named),
        ]);
        let ids_of =
            |q: TimelineQuery| ids_owned(&w.timeline(&q, Some(&e), &SessionActors::default()));
        let un = TimelineQuery {
            agent: Some(AgentFilter::parse("unattributed")),
            ..query()
        };
        assert_eq!(ids_of(un), ["event:1"]);
        let kind = TimelineQuery {
            agent: Some(AgentFilter::parse("claude-code")),
            ..query()
        };
        assert_eq!(ids_of(kind), ["event:2"]);
        let name = TimelineQuery {
            agent: Some(AgentFilter::parse("claude-2")),
            ..query()
        };
        assert_eq!(ids_of(name), ["event:4"]);
        let since = TimelineQuery {
            since_ms: Some(20),
            ..query()
        };
        assert_eq!(ids_of(since), ["event:2", "event:3", "event:4"]);
        let only = TimelineQuery {
            only_worktree: Some(WT.to_owned()),
            ..query()
        };
        assert_eq!(ids_of(only), ["event:1", "event:2", "event:4"]);
    }

    fn ids_owned(t: &TimelineResult) -> Vec<String> {
        t.entries.iter().map(|e| e.id.clone()).collect()
    }

    // ----- fill_files ---------------------------------------------------------------

    struct Fake {
        calls: Mutex<Vec<String>>,
        fail: HashSet<String>,
        sleep: Duration,
        /// Bytes of each path's last component.
        width: usize,
    }

    impl Fake {
        fn new(fail: &[&str], sleep: Duration) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail: fail.iter().map(|s| (*s).to_owned()).collect(),
                sleep,
                width: 0,
            }
        }

        fn wide(width: usize) -> Self {
            Self {
                width,
                ..Self::new(&[], Duration::ZERO)
            }
        }
    }

    impl PathsSource for Fake {
        fn changed_paths(
            &self,
            _: Option<&str>,
            new: &str,
            max: usize,
            deadline: Instant,
        ) -> Result<ChangedPaths, ReadError> {
            self.calls.lock().unwrap().push(new.to_owned());
            std::thread::sleep(self.sleep);
            if Instant::now() >= deadline {
                return Err(ReadError::TemporarilyUnavailable("deadline".into()));
            }
            if self.fail.contains(new) {
                return Err(ReadError::Unavailable("gone".into()));
            }
            let all: Vec<String> = (0..25)
                .map(|i| format!("{new}/f{i:02}{}", "p".repeat(self.width)))
                .collect();
            Ok(ChangedPaths {
                total: 25,
                paths: all.into_iter().take(max).collect(),
                first_parent: new == "merge",
            })
        }
    }

    fn commit_event(seq: i64, old: &str, new: &str) -> GitEventView {
        let mut e = event(seq, GitEventKind::Commit, seq * 10, Actor::Unattributed);
        e.details.old_commit = Some(old.into());
        e.details.new_commit = Some(new.into());
        e
    }

    fn timeline_of(events: &[GitEventView]) -> TimelineResult {
        let w = world();
        w.timeline(
            &query(),
            Some(&engine(events.to_vec())),
            &SessionActors::default(),
        )
    }

    #[test]
    fn plus_k_more_is_possible_files_are_capped_with_the_real_total() {
        let events = [commit_event(1, "a", "b")];
        let mut t = timeline_of(&events);
        fill_files(
            &mut t,
            &events,
            Some(&Fake::new(&[], Duration::ZERO)),
            &PathsCache::default(),
            FILES_BUDGET,
        );
        match &t.entries[0].files {
            ChangedFiles::Available {
                paths,
                total,
                first_parent,
            } => {
                assert_eq!(paths.len(), TIMELINE_MAX_FILES);
                assert_eq!(*total, 25);
                assert!(!first_parent);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn what_cannot_be_read_is_unavailable_never_zero() {
        let mut none = commit_event(2, "a", "a");
        none.kind = GitEventKind::Commit;
        let mut delete = event(3, GitEventKind::BranchDelete, 30, Actor::Unattributed);
        delete.details.old_commit = Some("x".into());
        let events = [commit_event(1, "a", "gone"), none, delete];
        let mut t = timeline_of(&events);
        fill_files(
            &mut t,
            &events,
            Some(&Fake::new(&["gone"], Duration::ZERO)),
            &PathsCache::default(),
            FILES_BUDGET,
        );
        assert!(
            t.entries
                .iter()
                .all(|e| e.files == ChangedFiles::Unavailable)
        );
        // No reader at all: the same.
        let mut t = timeline_of(&events[..1]);
        fill_files(
            &mut t,
            &events[..1],
            None,
            &PathsCache::default(),
            FILES_BUDGET,
        );
        assert_eq!(t.entries[0].files, ChangedFiles::Unavailable);
    }

    #[test]
    fn the_files_budget_degrades_to_unavailable_oldest_first() {
        let events: Vec<_> = (1..=6)
            .map(|i| commit_event(i, "o", &format!("c{i}")))
            .collect();
        let mut t = timeline_of(&events);
        // The budget lasts for three reads: counted, not timed.
        let fake = Fake::new(&[], Duration::ZERO);
        let mut checks = 0;
        fill_files_with(
            &mut t,
            &events,
            Some(&fake),
            &PathsCache::default(),
            Instant::now() + Duration::from_secs(60),
            &mut || {
                checks += 1;
                checks > 3
            },
            FILES_BYTES_BUDGET,
        );
        let available: Vec<bool> = t
            .entries
            .iter()
            .map(|e| matches!(e.files, ChangedFiles::Available { .. }))
            .collect();
        assert_eq!(available, [false, false, false, true, true, true]);
        assert_eq!(fake.calls.lock().unwrap().len(), 3);
        // Never partial: what is not available says so.
        assert!(t.entries.iter().all(|e| matches!(
            e.files,
            ChangedFiles::Available { total: 25, .. } | ChangedFiles::Unavailable
        )));
    }

    #[test]
    fn cache_hits_are_used_after_the_time_budget_is_gone() {
        let events: Vec<_> = (1..=3)
            .map(|i| commit_event(i, "o", &format!("c{i}")))
            .collect();
        let cache = PathsCache::default();
        let fake = Fake::new(&[], Duration::ZERO);
        let mut warm = timeline_of(&events);
        fill_files(&mut warm, &events, Some(&fake), &cache, FILES_BUDGET);
        assert_eq!(fake.calls.lock().unwrap().len(), 3);
        let mut t = timeline_of(&events);
        fill_files_with(
            &mut t,
            &events,
            Some(&fake),
            &cache,
            Instant::now(),
            &mut || true,
            FILES_BYTES_BUDGET,
        );
        assert!(
            t.entries
                .iter()
                .all(|e| matches!(e.files, ChangedFiles::Available { .. }))
        );
        assert_eq!(fake.calls.lock().unwrap().len(), 3, "no new read");
    }

    #[test]
    fn the_paths_of_a_response_have_a_byte_budget_and_the_frame_fits() {
        use gitraptor_api::framing::MAX_MESSAGE_BYTES;
        let page = usize::try_from(MAX_HISTORY_PAGE).unwrap();
        let events: Vec<_> = (1..=page as i64)
            .map(|i| commit_event(i, "o", &format!("c{i}")))
            .collect();
        let mut t = World::timeline(
            &world(),
            &TimelineQuery {
                limit: page,
                ..query()
            },
            Some(&engine(events.clone())),
            &SessionActors::default(),
        );
        assert_eq!(t.entries.len(), page);
        // Each path at the longest the git layer lets through.
        let fake = Fake::wide(gitraptor_git::MAX_PATH_BYTES - 10);
        fill_files(
            &mut t,
            &events,
            Some(&fake),
            &PathsCache::default(),
            Duration::from_secs(60),
        );
        let bytes = serde_json::to_vec(&t).unwrap().len();
        assert!(bytes < MAX_MESSAGE_BYTES, "{bytes} bytes");
        let available = t
            .entries
            .iter()
            .filter(|e| matches!(e.files, ChangedFiles::Available { .. }))
            .count();
        assert!(available > 0 && available < page, "{available}");
        // The newest ones kept their paths, the oldest degraded.
        assert!(matches!(
            t.entries.last().unwrap().files,
            ChangedFiles::Available { .. }
        ));
        assert_eq!(t.entries[0].files, ChangedFiles::Unavailable);
        const _: () = assert!(FILES_BYTES_BUDGET < MAX_MESSAGE_BYTES / 2);
    }

    #[test]
    fn first_parent_is_only_said_for_a_commit_or_a_merge() {
        let mut reset = commit_event(1, "a", "merge");
        reset.kind = GitEventKind::BranchUpdate;
        let mut merge = commit_event(2, "a", "merge");
        merge.kind = GitEventKind::Merge;
        let events = [reset, merge];
        let mut t = timeline_of(&events);
        fill_files(
            &mut t,
            &events,
            Some(&Fake::new(&[], Duration::ZERO)),
            &PathsCache::default(),
            FILES_BUDGET,
        );
        let flags: Vec<bool> = t
            .entries
            .iter()
            .map(|e| match &e.files {
                ChangedFiles::Available { first_parent, .. } => *first_parent,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(flags, [false, true]);
    }

    #[test]
    fn a_full_history_page_drops_the_operations_older_than_its_oldest_event() {
        let mut w = world();
        let old = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                1,
            ),
            OperationState::Finished,
            50,
        );
        let recent = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                1,
            ),
            OperationState::Finished,
            500,
        );
        let page = usize::try_from(MAX_HISTORY_PAGE).unwrap();
        let events: Vec<_> = (1..=page as i64)
            .map(|i| event(i, GitEventKind::Commit, 100 + i, Actor::Unattributed))
            .collect();
        let mut e = engine(events);
        let q = TimelineQuery {
            limit: 500,
            ..query()
        };
        let t = w.timeline(&q, Some(&e), &SessionActors::default());
        assert!(t.truncated);
        let got = ids(&t);
        assert!(!got.contains(&format!("operation:{old}").as_str()), "a gap");
        assert!(got.contains(&format!("operation:{recent}").as_str()));
        // A page that is not full leaves nothing unseen: the old operation shows.
        e.history_full = false;
        let t = w.timeline(&q, Some(&e), &SessionActors::default());
        assert!(ids(&t).contains(&format!("operation:{old}").as_str()));
        assert!(!t.truncated);
    }

    #[test]
    fn a_full_page_stays_full_when_events_were_filtered_out_of_it() {
        // `history_full` is the page's, not the shown events': a client that cannot read resets
        // still sees a truncated list.
        let w = world();
        let e = EngineSide {
            history_full: true,
            history_oldest_ms: Some(1),
            ..engine(vec![event(
                1,
                GitEventKind::Commit,
                100,
                Actor::Unattributed,
            )])
        };
        assert!(
            w.timeline(&query(), Some(&e), &SessionActors::default())
                .truncated
        );
    }

    #[test]
    fn claude_code_also_matches_a_recorded_requester_by_its_display_name() {
        let mut w = world();
        let mut recorded = Vec::new();
        for (name, at) in [
            ("claude-code", 10),
            ("Claude Code", 11),
            ("CLAUDE CODE", 12),
            ("codex", 13),
        ] {
            recorded.push(w.operation(
                &new_op(
                    OperationKind::Undo,
                    agent(name, "s"),
                    Target::Undo(vec![OpRef::GitEvent(1)]),
                    1,
                ),
                OperationState::Finished,
                at,
            ));
        }
        let q = TimelineQuery {
            agent: Some(AgentFilter::parse("claude-code")),
            ..query()
        };
        let t = w.timeline(&q, Some(&engine(vec![])), &SessionActors::default());
        let want: Vec<String> = recorded[..3]
            .iter()
            .map(|id| format!("operation:{id}"))
            .collect();
        assert_eq!(ids_owned(&t), want);
    }

    #[test]
    fn since_leaves_out_what_is_older_in_operations_and_events() {
        // Unit level: the integration clock is the wall clock, and a test must not sleep to age
        // an entry; here the entries carry their own times.
        let mut w = world();
        let old_op = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                1,
            ),
            OperationState::Finished,
            100,
        );
        let new_op_id = w.operation(
            &new_op(
                OperationKind::Protected,
                Requester::Unattributed,
                Target::None,
                1,
            ),
            OperationState::Finished,
            300,
        );
        let e = engine(vec![
            event(1, GitEventKind::Commit, 150, Actor::Unattributed),
            event(2, GitEventKind::Commit, 350, Actor::Unattributed),
        ]);
        let q = TimelineQuery {
            since_ms: Some(200),
            ..query()
        };
        let t = w.timeline(&q, Some(&e), &SessionActors::default());
        assert_eq!(
            ids_owned(&t),
            [format!("operation:{new_op_id}"), "event:2".to_owned()]
        );
        assert!(!ids_owned(&t).contains(&format!("operation:{old_op}")));
    }

    #[test]
    fn the_point_before_an_event_is_the_latest_one_of_its_generation() {
        let mut w = world();
        let early = w.snapshot(SnapshotLevel::Observation, 5, None, 10);
        let late = w.snapshot(SnapshotLevel::HookPrior, 9, None, 20);
        let after = w.snapshot(SnapshotLevel::Observation, 20, None, 30);
        let read = OplogRead::read(&w.log, &w.refs, &query());
        let points = Points::new(read.points.as_deref().unwrap());
        let id = |p: Option<&SnapshotView>| p.map(|s| s.record.snapshot_id.clone());
        assert_eq!(id(points.before_event(KEY, 7, 0)), Some(early.clone()));
        assert_eq!(id(points.before_event(KEY, 10, 0)), Some(late.clone()));
        assert_eq!(id(points.before_event(KEY, 21, 0)), Some(after));
        assert_eq!(id(points.before_event(KEY, 5, 0)), None);
        assert_eq!(id(points.before_event("other", 21, 0)), None);
        // A floor past the points leaves none.
        assert_eq!(id(points.before_event(KEY, 21, i64::MAX)), None);
        assert_eq!(id(points.get(&early)), Some(early));
    }

    #[test]
    fn a_merge_says_it_is_against_its_first_parent_and_the_cache_is_bounded() {
        let events = [commit_event(1, "a", "merge")];
        let mut t = timeline_of(&events);
        let cache = PathsCache::default();
        let fake = Fake::new(&[], Duration::ZERO);
        fill_files(&mut t, &events, Some(&fake), &cache, FILES_BUDGET);
        assert!(matches!(
            t.entries[0].files,
            ChangedFiles::Available {
                first_parent: true,
                ..
            }
        ));
        // The same pair is not read again.
        let mut t = timeline_of(&events);
        fill_files(&mut t, &events, Some(&fake), &cache, FILES_BUDGET);
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        for i in 0..(CACHE_CAPACITY + 10) {
            cache.put(
                ("r".into(), "o".into(), format!("n{i}")),
                ChangedPaths {
                    paths: vec![],
                    total: 0,
                    first_parent: false,
                },
            );
        }
        assert!(cache.len() <= CACHE_CAPACITY);
    }
}
