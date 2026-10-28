//! TMC012-CLASSIFY / SEGMENTS / FAIL-CLOSED (T003 of DS-US-TMC-012): `overlap::check` over real
//! captures of a temporary repo and synthetic raw events. Nothing here touches this repo or the
//! real profile (NFR-01). Child module of `timemachine::overlap` (`use super::*`).
//!
//! Marks: the oplog's operations and the engine's raw events share one numbering. The base of
//! the undone operation (its anchor) is the `Observation` captured with `cause_operation`.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::apply::PlanWorktree;
use crate::timemachine::engine::RawGitEvent;
use crate::timemachine::oplog::{
    Channel, NewOperation, OpRef, OperationFilter, OperationKind, OperationTransition, Oplog,
    Requester, RequesterOrigin, Scope, SnapshotFilter, SnapshotLevel, SnapshotState, Target,
};
use crate::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use crate::timemachine::undo::RawSide;
use gitraptor_api::messages::GitEventKind;
use gitraptor_testkit::Fixture;

const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const KEY: &str = "main";
const BRANCH: &str = "feat-pagos";

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

struct World {
    f: Fixture,
    store: SnapshotStore,
    oplog: Mutex<Oplog>,
}

impl World {
    /// A repo with `api.rs` committed as `v0` and the branch `feat-pagos` on that commit.
    fn new() -> Self {
        let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        f.write("api.rs", "v0\n");
        f.git(&["add", "api.rs"]);
        f.git(&["commit", "-q", "-m", "api"]);
        f.git(&["branch", BRANCH]);
        let dirs = ProfileDirs::under_root(&f.profile);
        let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
        let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
        Self {
            f,
            store,
            oplog: Mutex::new(oplog),
        }
    }

    fn root(&self) -> PathBuf {
        self.f.repo.clone()
    }

    fn write_api(&self, content: &str) {
        std::fs::write(self.f.repo.join("api.rs"), content).unwrap();
    }

    /// Moves the branch to a new commit (no change to any file).
    fn move_branch(&self, message: &str) {
        let id = self
            .f
            .git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", message]);
        self.f
            .git(&["update-ref", &format!("refs/heads/{BRANCH}"), id.trim()]);
    }

    fn capture(&self, level: SnapshotLevel, mark: i64, cause: Option<&str>) -> String {
        let req = CaptureRequest {
            level,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: KEY.into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: Some(mark),
            cause_operation: cause.map(str::to_owned),
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.store.capture(&self.oplog, &req).unwrap().snapshot_id
    }

    fn point(&self, mark: i64) -> String {
        self.capture(SnapshotLevel::Observation, mark, None)
    }

    /// A finished operation of `who` at engine mark `mark` whose prior point is `prior`.
    fn operation(&self, who: Requester, mark: i64, prior: &str, refs: &[&str]) -> String {
        let mut log = self.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![self.root().to_string_lossy().into_owned()],
                        refs: refs.iter().map(|r| (*r).to_owned()).collect(),
                    },
                    requester: who,
                    channel: Channel::Mcp,
                    confirmed: false,
                    target: Target::None,
                    warnings: Vec::new(),
                    engine_mark: mark,
                },
                1,
            )
            .unwrap();
        for step in [
            OperationTransition::PriorSnapshot { snapshot_id: prior },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, 2).unwrap();
        }
        id
    }

    fn make_unavailable(&self, snapshot_id: &str) {
        let mut log = self.oplog.lock().unwrap();
        log.set_snapshot_state(snapshot_id, SnapshotState::PurgeIntent, 3)
            .unwrap();
        log.set_snapshot_state(snapshot_id, SnapshotState::Purged, 4)
            .unwrap();
    }

    fn check(
        &self,
        reference: &Requester,
        undone: &OpRef,
        target: &str,
        events: Vec<RawGitEvent>,
        deadline: Instant,
    ) -> OverlapReport {
        let ops = self
            .oplog
            .lock()
            .unwrap()
            .operations(&OperationFilter::default())
            .unwrap();
        let snaps = self
            .oplog
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap();
        let worktrees = [PlanWorktree {
            key: KEY.into(),
            root: self.root(),
            recreate_id: None,
            keep: BTreeSet::new(),
        }];
        let refs: BTreeSet<String> = [format!("refs/heads/{BRANCH}")].into();
        let raw: HashMap<PathBuf, RawSide> = [(self.root(), RawSide { events, floor: 0 })].into();
        let input = OverlapInput {
            reference: std::slice::from_ref(reference),
            undone: std::slice::from_ref(undone),
            target,
            worktrees: &worktrees,
            refs: &refs,
            ops: &ops,
            snaps: &snaps,
            raw: &raw,
            deadline,
        };
        check(&self.store, &input)
    }
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn branch_event(seq: i64, actor: Requester) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind: GitEventKind::BranchUpdate,
        branch: Some(BRANCH.into()),
        actor,
    }
}

/// Whether a set of ref names holds the branch, as a short name or as `refs/heads/<name>`.
fn has_branch(set: &BTreeSet<String>) -> bool {
    set.iter()
        .any(|n| n == BRANCH || n == &format!("refs/heads/{BRANCH}"))
}

fn conflicts_on_branch(report: &OverlapReport) -> usize {
    report
        .conflicts
        .iter()
        .filter(|c| match &c.subject {
            Subject::Ref(name) => name == BRANCH || name == &format!("refs/heads/{BRANCH}"),
            Subject::Path { .. } => false,
        })
        .count()
}

fn conflicts_on_path<'a>(report: &'a OverlapReport, wanted: &str) -> Vec<&'a Conflict> {
    report
        .conflicts
        .iter()
        .filter(|c| matches!(&c.subject, Subject::Path { path, .. } if path == wanted))
        .collect()
}

fn nothing_kept(report: &OverlapReport) -> bool {
    report.kept.values().all(BTreeSet::is_empty) && report.kept_refs.is_empty()
}

/// The scenario every branch test shares: the undone operation (of `s1`) moved the branch, the
/// base `B` (mark 10) shows it, and the target `T` is the point before.
struct Moved {
    w: World,
    target: String,
    op: String,
}

fn moved_by_the_undone_operation() -> Moved {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[&format!("refs/heads/{BRANCH}")]);
    w.move_branch("moved by the undone operation");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    Moved { w, target, op }
}

#[test]
fn mixed_attribution_counts_as_another_actor() {
    // Only the reference's own session moved the branch afterwards: nobody else.
    let own = moved_by_the_undone_operation();
    own.w.move_branch("later, same session");
    own.w.point(20);
    let report = own.w.check(
        &agent("s1"),
        &OpRef::Oplog(own.op.clone()),
        &own.target,
        vec![branch_event(15, agent("s1"))],
        soon(),
    );
    assert_eq!(
        conflicts_on_branch(&report),
        0,
        "a segment of only the reference's session is not another actor"
    );
    assert!(!report.unverified);

    // The same segment with one more session in it: mixed counts as another actor.
    let mixed = moved_by_the_undone_operation();
    mixed.w.move_branch("later, two sessions");
    mixed.w.point(20);
    let report = mixed.w.check(
        &agent("s1"),
        &OpRef::Oplog(mixed.op.clone()),
        &mixed.target,
        vec![branch_event(15, agent("s1")), branch_event(16, agent("s2"))],
        soon(),
    );
    assert_eq!(
        conflicts_on_branch(&report),
        1,
        "a segment with actors of the reference and of another one is a conflict on the branch: {:?}",
        report.conflicts
    );
}

#[test]
fn an_echo_counts_with_its_operations_requester() {
    // `op2` ran after the undone operation; its event is its echo: it runs from the operation's
    // mark (12) to the mark of its anchor (20) and counts with the operation's requester.
    let scenario = |op2_requester: Requester, event_actor: Requester| {
        let m = moved_by_the_undone_operation();
        let op2 = m.w.operation(
            op2_requester,
            12,
            &m.target,
            &[&format!("refs/heads/{BRANCH}")],
        );
        m.w.move_branch("moved by the second operation");
        m.w.capture(SnapshotLevel::Observation, 20, Some(&op2));
        m.w.check(
            &agent("s1"),
            &OpRef::Oplog(m.op.clone()),
            &m.target,
            vec![branch_event(15, event_actor)],
            soon(),
        )
    };

    // Same requester as the reference, though the engine saw the event as another session's.
    let own = scenario(agent("s1"), agent("s2"));
    assert_eq!(
        conflicts_on_branch(&own),
        0,
        "the echo counts with its operation's requester (the reference), not with the event's actor: {:?}",
        own.conflicts
    );

    // Another requester: the echo counts as that one, whoever the event says.
    let other = scenario(agent("s2"), agent("s1"));
    assert_eq!(
        conflicts_on_branch(&other),
        1,
        "the echo of another session's operation is another actor: {:?}",
        other.conflicts
    );
}

#[test]
fn a_path_back_to_its_target_content_is_not_written() {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    w.write_api("v1\n");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    // Another actor puts `api.rs` back to what the target had.
    w.write_api("v0\n");
    w.point(20);

    let report = w.check(&agent("s1"), &OpRef::Oplog(op), &target, Vec::new(), soon());

    assert!(!report.unverified);
    assert!(
        conflicts_on_path(&report, "api.rs").is_empty(),
        "api.rs holds the target's content again: it is neither a conflict nor kept: {:?}",
        report.conflicts
    );
    assert!(nothing_kept(&report), "{:?}", report.kept);
}

#[test]
fn an_index_only_change_overlaps() {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    // The undone operation changed the file in the worktree only.
    w.write_api("v1\n");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    // Later another actor staged other content and put the file back: only the index differs.
    w.write_api("v2\n");
    w.f.git(&["add", "api.rs"]);
    w.write_api("v1\n");
    w.point(20);

    let report = w.check(&agent("s1"), &OpRef::Oplog(op), &target, Vec::new(), soon());

    let found = conflicts_on_path(&report, "api.rs");
    assert_eq!(
        found.len(),
        1,
        "a path that changed only in the index overlaps: {:?}",
        report.conflicts
    );
    assert_eq!(
        found[0].actor,
        Requester::Unattributed,
        "a change of files or index without an event is unattributed (D3)"
    );
}

/// Builds the case of the merge tests. The undone operation did not touch the branch (`B` has it
/// on the same commit as `T`); a second session named the branch in `(10, 20]` without moving it
/// and the reference's session moved it in `(20, 30]`.
fn merged_case(first_point_unavailable: bool) -> OverlapReport {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    w.write_api("v1\n");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    let first = w.point(20);
    w.move_branch("moved in the second segment");
    w.point(30);
    if first_point_unavailable {
        w.make_unavailable(&first);
    }
    w.check(
        &agent("s1"),
        &OpRef::Oplog(op),
        &target,
        vec![branch_event(15, agent("s2")), branch_event(25, agent("s1"))],
        soon(),
    )
}

#[test]
fn an_unavailable_point_merges_two_segments() {
    let cut = merged_case(false);
    assert!(!cut.unverified);
    assert!(
        !has_branch(&cut.kept_refs) && conflicts_on_branch(&cut) == 0,
        "with the point available the branch only moved in a segment of the reference's session: {cut:?}"
    );

    let merged = merged_case(true);
    assert!(
        !merged.unverified,
        "an unavailable point is merged, not a reason to give up"
    );
    assert!(
        has_branch(&merged.kept_refs),
        "an unavailable point does not cut: both segments merge and the other session's event \
         counts, so the branch another actor changed is kept: {merged:?}"
    );
}

#[test]
fn a_shadowed_point_does_not_cut() {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    w.write_api("v1\n");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    // A prior just before a command of the reference's session (mark 20): its first event comes
    // at 30. A point recorded after the prior with mark 25 shows the command's effect under a
    // stale mark: the prior shadows it, so it is not a place to cut.
    w.capture(SnapshotLevel::GuaranteedPrior, 20, None);
    w.move_branch("the command's effect");
    w.point(25);
    w.point(40);

    let report = w.check(
        &agent("s1"),
        &OpRef::Oplog(op),
        &target,
        vec![branch_event(30, agent("s1"))],
        soon(),
    );

    assert!(!report.unverified);
    assert!(
        !has_branch(&report.kept_refs) && conflicts_on_branch(&report) == 0,
        "cutting at the shadowed point would leave the movement in a segment without events \
         (unattributed); it belongs to the command's own event: {report:?}"
    );
}

#[test]
fn without_a_base_an_agents_change_is_unverified_never_written() {
    // No capture names the operation as its cause: there is no base B.
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    w.write_api("v1\n");
    w.point(20);

    let report = w.check(&agent("s1"), &OpRef::Oplog(op), &target, Vec::new(), soon());

    assert!(
        report.unverified,
        "without B the undone change cannot be told from what came after: unverified"
    );

    // An unattributed reference needs no base: its edits do not count (D4, D7).
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(Requester::Unattributed, 5, &target, &[]);
    w.write_api("v1\n");
    w.point(20);
    let report = w.check(
        &Requester::Unattributed,
        &OpRef::Oplog(op),
        &target,
        Vec::new(),
        soon(),
    );
    assert!(
        !report.unverified,
        "an unattributed reference does not need a base"
    );
}

#[test]
fn the_deadline_gives_unverified() {
    let w = World::new();
    let target = w.point(1);
    let op = w.operation(agent("s1"), 5, &target, &[]);
    w.write_api("v1\n");
    w.capture(SnapshotLevel::Observation, 10, Some(&op));
    w.write_api("v2\n");
    w.point(20);
    let past = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);

    let report = w.check(&agent("s1"), &OpRef::Oplog(op), &target, Vec::new(), past);

    assert!(
        report.unverified,
        "an expired deadline fails closed: unverified, never 'no overlap'"
    );
}
