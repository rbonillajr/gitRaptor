//! The overlap check fails closed where it cannot tell actors or generations apart: a base of
//! another engine generation is never B, missing events with an unattributed reference are
//! `unverified`, and a branch move nothing explains is another actor's (or `unverified` when the
//! reference is unattributed). Real captures of a temporary repo and synthetic raw events;
//! nothing here touches this repo or the real profile. Child module of `timemachine::overlap`.

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
    Requester, RequesterOrigin, Scope, SnapshotFilter, SnapshotLevel, Target,
};
use crate::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use crate::timemachine::undo::RawSide;
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

    fn move_branch(&self, message: &str) {
        let id = self
            .f
            .git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", message]);
        self.f
            .git(&["update-ref", &format!("refs/heads/{BRANCH}"), id.trim()]);
    }

    fn capture(&self, mark: i64, cause_op: Option<&str>, cause_event: Option<i64>) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::Observation,
            echo_settled: None,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: KEY.into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: Some(mark),
            cause_operation: cause_op.map(str::to_owned),
            cause_event_seq: cause_event,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.store.capture(&self.oplog, &req).unwrap().snapshot_id
    }

    /// The oplog sequence of a snapshot: where a generation that starts at it begins.
    fn seq_of(&self, snapshot_id: &str) -> i64 {
        self.oplog
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap()
            .into_iter()
            .find(|s| s.record.snapshot_id == snapshot_id)
            .map(|s| s.record.seq)
            .unwrap()
    }

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

    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        reference: &Requester,
        undone: &OpRef,
        target: &str,
        events: Vec<RawGitEvent>,
        floor: i64,
        events_complete: bool,
        tail_prior: Option<&str>,
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
        let raw: HashMap<PathBuf, RawSide> = [(self.root(), RawSide { events, floor })].into();
        let input = OverlapInput {
            reference: std::slice::from_ref(reference),
            undone: std::slice::from_ref(undone),
            target,
            worktrees: &worktrees,
            refs: &refs,
            ops: &ops,
            snaps: &snaps,
            raw: &raw,
            deadline: Instant::now() + Duration::from_secs(60),
        };
        let phase1 = check_with(&self.store, &input, events_complete);
        match tail_prior {
            Some(prior) => check_tail_with(
                &self.store,
                &input,
                &phase1,
                prior,
                i64::MAX / 2,
                events_complete,
            ),
            None => phase1,
        }
    }
}

fn conflicts_on_branch(report: &OverlapReport) -> Vec<&Conflict> {
    report
        .conflicts
        .iter()
        .filter(|c| matches!(&c.subject, Subject::Ref(full) if full.ends_with(BRANCH)))
        .collect()
}

/// A snapshot of an older engine generation caused by an event with the same seq as the undone
/// one: under the current generation's floor it is never B, so an agent's change without a B of
/// its generation is `unverified`.
#[test]
fn an_older_generation_snapshot_is_never_the_base() {
    let w = World::new();
    // Older generation: its marks are of another numbering; seq 5 named another event there.
    w.write_api("older generation\n");
    let old = w.capture(7, None, Some(5));
    // Current generation, starting at the target.
    w.write_api("v0\n");
    let target = w.capture(1, None, None);
    let floor = w.seq_of(&target);
    assert!(
        w.seq_of(&old) < floor,
        "the old snapshot is under the floor"
    );
    w.write_api("later edit\n");
    w.capture(10, None, None);

    let report = w.run(
        &agent("s1"),
        &OpRef::GitEvent(5),
        &target,
        Vec::new(),
        floor,
        true,
        None,
    );
    assert!(
        report.unverified,
        "a base of another generation is never chosen: unverified, not a check against it: {report:?}"
    );

    // The same snapshot within the current generation is a valid B: the floor decides.
    let report = w.run(
        &agent("s1"),
        &OpRef::GitEvent(5),
        &target,
        Vec::new(),
        0,
        true,
        None,
    );
    assert!(
        !report.unverified,
        "inside the generation the caused snapshot is B: {report:?}"
    );
}

/// Without every worktree's events an unattributed reference cannot tell another agent's unseen
/// work from its own: both phases are `unverified`. An agent reference already counts what it
/// cannot see as another actor's.
#[test]
fn missing_events_with_an_unattributed_reference_are_unverified() {
    let w = World::new();
    let target = w.capture(1, None, None);
    let op = w.operation(Requester::Unattributed, 5, &target, &[]);
    w.write_api("by the undone operation\n");
    w.capture(10, Some(&op), None);
    w.write_api("later\n");
    let last = w.capture(20, None, None);
    let undone = OpRef::Oplog(op.clone());

    let phase1 = w.run(
        &Requester::Unattributed,
        &undone,
        &target,
        Vec::new(),
        0,
        false,
        None,
    );
    assert!(phase1.unverified, "phase 1 without all events: {phase1:?}");

    let tail = w.run(
        &Requester::Unattributed,
        &undone,
        &target,
        Vec::new(),
        0,
        false,
        Some(&last),
    );
    assert!(tail.unverified, "phase 2 without all events: {tail:?}");

    let complete = w.run(
        &Requester::Unattributed,
        &undone,
        &target,
        Vec::new(),
        0,
        true,
        None,
    );
    assert!(
        !complete.unverified,
        "with every event the same check runs: {complete:?}"
    );

    let agent_op = w.operation(agent("s1"), 25, &last, &[]);
    w.write_api("by the agent\n");
    w.capture(30, Some(&agent_op), None);
    let by_agent = w.run(
        &agent("s1"),
        &OpRef::Oplog(agent_op),
        &last,
        Vec::new(),
        0,
        false,
        None,
    );
    assert!(
        !by_agent.unverified,
        "an agent reference is not blind without events: {by_agent:?}"
    );
}

/// A branch moved after the undone change with no event or operation that explains it: for an
/// agent reference it is another actor's (`Unattributed`), never the reference's own.
#[test]
fn an_unexplained_branch_move_is_another_actor_for_an_agent() {
    let w = World::new();
    let target = w.capture(1, None, None);
    let full = format!("refs/heads/{BRANCH}");
    let op = w.operation(agent("s1"), 5, &target, &[&full]);
    w.move_branch("moved by the undone operation");
    w.capture(10, Some(&op), None);
    w.move_branch("moved by nobody the engine saw");
    w.capture(20, None, None);

    let report = w.run(
        &agent("s1"),
        &OpRef::Oplog(op),
        &target,
        Vec::new(),
        0,
        true,
        None,
    );

    assert!(!report.unverified, "{report:?}");
    let on_branch = conflicts_on_branch(&report);
    assert_eq!(on_branch.len(), 1, "{report:?}");
    assert_eq!(on_branch[0].actor, Requester::Unattributed);
}

/// The same unexplained move with an unattributed reference: `Unattributed` is the reference's
/// own class, so who moved it cannot be told: `unverified`, never "no overlap".
#[test]
fn an_unexplained_branch_move_is_unverified_for_an_unattributed_reference() {
    let w = World::new();
    let target = w.capture(1, None, None);
    let full = format!("refs/heads/{BRANCH}");
    let op = w.operation(Requester::Unattributed, 5, &target, &[&full]);
    w.move_branch("moved by the undone operation");
    w.capture(10, Some(&op), None);
    w.move_branch("moved by nobody the engine saw");
    w.capture(20, None, None);

    let report = w.run(
        &Requester::Unattributed,
        &OpRef::Oplog(op),
        &target,
        Vec::new(),
        0,
        true,
        None,
    );

    assert!(
        report.unverified,
        "an unexplained move cannot be told from the reference's own: {report:?}"
    );
    assert!(report.stops());
}
