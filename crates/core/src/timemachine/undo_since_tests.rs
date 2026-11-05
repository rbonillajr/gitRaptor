//! US-TMC-010 (T003): which operations an undo by period takes back, and that it never takes
//! back less than it says (D2, D4, NFR-01). Child module of `timemachine::undo`.
//!
//! Temporary repo and profile only. Every undo here is refused after the plan (no Git given to
//! the undo: `git-unavailable`), so nothing is applied; what is read is the rejection the undo
//! records, whose `Target::Undo` is the set the undo chose.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::channel::peer::{ProcError, ProcInfo};
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::EngineLink;
use crate::timemachine::oplog::{
    NewOperation, OperationState, OperationTransition, SnapshotLevel, SnapshotState,
};
use crate::timemachine::protected::{McpRepos, TimeMachineBackend, TmRepos};
use crate::timemachine::store::{CaptureRequest, WorktreeScope};
use gitraptor_api::messages::GitEventKind;
use gitraptor_testkit::Fixture;

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

struct Engine {
    events: Vec<RawGitEvent>,
}

impl EngineLink for Engine {
    fn mark(&self, _repo_id: &str) -> i64 {
        1_000
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        Some(1_000)
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(self.events.clone())
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct NoProcs;

impl ProcSource for NoProcs {
    fn read(&self, _pid: u32) -> Result<ProcInfo, ProcError> {
        Err(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

struct Rig {
    f: Fixture,
    dirs: ProfileDirs,
    repo: TmRepoHandle,
}

fn rig() -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let dirs = ProfileDirs::under_root(&f.profile);
    let common = crate::observe::locate(&f.repo).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &common, oplog);
    let backend = TimeMachineBackend::new(repos, None, Arc::new(McpRepos::new([])));
    let repo = backend
        .repo_of(&f.repo)
        .expect("the temporary repo is observed");
    Rig { f, dirs, repo }
}

impl Rig {
    fn key(&self) -> String {
        self.repo.repo.worktree.to_string_lossy().into_owned()
    }

    /// An unattributed operation of this worktree recorded at `recorded_ms`, in the stack at
    /// engine mark `mark`, finished with a real prior snapshot of the worktree (its content is
    /// `name`, so every prior differs).
    fn operation(&self, name: &str, recorded_ms: i64, mark: i64) -> (String, String) {
        std::fs::write(self.f.repo.join("since.txt"), name).unwrap();
        let oplog = &self.repo.repo.oplog;
        let id = oplog
            .lock()
            .unwrap()
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![self.key()],
                        refs: Vec::new(),
                    },
                    requester: Requester::Unattributed,
                    channel: Channel::Cli,
                    confirmed: true,
                    target: Target::None,
                    warnings: Vec::new(),
                    engine_mark: mark,
                },
                recorded_ms,
            )
            .unwrap();
        let store = self.repo.store.as_deref().expect("the store opens");
        let prior = store
            .capture(
                oplog,
                &CaptureRequest {
                    level: SnapshotLevel::GuaranteedPrior,
                    echo_settled: None,
                    common_dir: self.repo.repo.common_dir.clone(),
                    worktrees: vec![WorktreeScope {
                        key: "main".into(),
                        path: self.f.repo.clone(),
                        hint: None,
                    }],
                    engine_mark: Some(mark),
                    cause_operation: Some(id.clone()),
                    cause_event_seq: None,
                    include_credentials: false,
                    still_valid: None,
                    give_way: None,
                },
            )
            .unwrap()
            .snapshot_id;
        let mut log = oplog.lock().unwrap();
        for step in [
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, recorded_ms).unwrap();
        }
        (id, prior)
    }

    /// Asks for `selection` as an unattributed person with no Git to run: the rejection and the
    /// `Target::Undo` the oplog kept for it.
    fn attempt(&self, events: Vec<RawGitEvent>, selection: Selection) -> (UndoError, Vec<OpRef>) {
        let deps = CaptureDeps {
            repos: Arc::new(TmRepos::new(self.dirs.clone())),
            engine: Arc::new(Engine { events }),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: None,
            after_mark: None,
        };
        let marks = Arc::new(ExecutorMarks::default());
        let stopping = AtomicBool::new(false);
        let invoker = Invoker::default();
        let env = UndoEnv {
            marks: &marks,
            procs: &NoProcs,
            stopping: &stopping,
            prior_deadline: Duration::from_secs(5),
            git: None,
            invoker: &invoker,
            engine: Some(&deps),
            fallback_mark: 1_000,
        };
        let err = undo_selected(
            &self.repo,
            &Who::unattributed(),
            Channel::Cli,
            &Confirmation::unavailable(),
            &env,
            selection,
        )
        .expect_err("no Git was given: the undo cannot finish");
        let operation_id = match &err {
            UndoError::Rejected { operation_id, .. } => operation_id.clone().expect("recorded"),
            other => panic!("expected a rejection, got {other:?}"),
        };
        let view = self
            .repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .operation(&operation_id)
            .unwrap()
            .expect("the rejection is in the oplog");
        assert_eq!(view.record.kind, OperationKind::Undo);
        assert_eq!(view.state, OperationState::Rejected);
        let Target::Undo(set) = view.record.target else {
            panic!("a rejected undo records Target::Undo");
        };
        (err, set)
    }

    fn since(&self, secs_ago: i64) -> Selection {
        Selection::Since {
            since_ms: now_ms() - secs_ago * 1_000,
        }
    }
}

fn reason(err: &UndoError) -> TmRejectReason {
    match err {
        UndoError::Rejected { reason, .. } => *reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn oplog_op(id: &str) -> OpRef {
    OpRef::Oplog(id.to_owned())
}

#[test]
fn since_takes_the_suffix_of_the_stack_inside_the_period() {
    let r = rig();
    let now = now_ms();
    let (_a, _) = r.operation("a", now - 60_000, 1);
    let (b, _) = r.operation("b", now - 3_000, 2);
    let (c, _) = r.operation("c", now - 1_000, 3);

    let (err, set) = r.attempt(Vec::new(), r.since(10));
    assert_eq!(
        reason(&err),
        TmRejectReason::GitUnavailable,
        "TMC010-SELECT: the plan goes through and only the missing Git stops it"
    );
    assert_eq!(
        set,
        vec![oplog_op(&b), oplog_op(&c)],
        "TMC010-SELECT: the set is the suffix of the stack inside the period, in stack order"
    );

    let (_, last) = r.attempt(Vec::new(), Selection::Last);
    assert_eq!(
        last,
        vec![oplog_op(&c)],
        "TMC010-SELECT: Selection::Last is a set of one, today's behaviour"
    );
}

#[test]
fn since_stops_at_the_first_older_operation() {
    let r = rig();
    let now = now_ms();
    // A is inside the period but sits below B, which is older than the threshold.
    let (_a, _) = r.operation("a", now - 1_000, 1);
    let (_b, _) = r.operation("b", now - 60_000, 2);
    let (c, _) = r.operation("c", now - 1_000, 3);

    let (_, set) = r.attempt(Vec::new(), r.since(10));
    assert_eq!(
        set,
        vec![oplog_op(&c)],
        "TMC010-SELECT: the walk stops at the first older operation (D2): undoing less, never more"
    );
}

#[test]
fn a_raw_event_not_covered_rejects_the_whole_period() {
    let r = rig();
    let now = now_ms();
    let (a, _) = r.operation("a", now - 60_000, 1);
    // As in production, the operation leaves an anchor capture (an Observation of the state it
    // left, at its own mark): without it a's echo window would run on to b's prior and swallow
    // the push below as a's echo.
    let store = r.repo.store.as_deref().expect("the store opens");
    store
        .capture(
            &r.repo.repo.oplog,
            &CaptureRequest {
                level: SnapshotLevel::Observation,
                echo_settled: None,
                common_dir: r.repo.repo.common_dir.clone(),
                worktrees: vec![WorktreeScope {
                    key: "main".into(),
                    path: r.f.repo.clone(),
                    hint: None,
                }],
                engine_mark: Some(1),
                cause_operation: Some(a),
                cause_event_seq: None,
                include_credentials: false,
                still_valid: None,
                give_way: None,
            },
        )
        .unwrap();
    // A push inside the period: the Time Machine does not undo it, and does not skip it.
    let push = RawGitEvent {
        seq: 2,
        kind: GitEventKind::Push,
        branch: Some("main".into()),
        actor: Requester::Unattributed,
        observed_ms: now - 2_000,
    };
    let (b, _) = r.operation("b", now - 1_000, 3);
    let oplog_before = r
        .repo
        .repo
        .oplog
        .lock()
        .unwrap()
        .snapshots(&Default::default())
        .unwrap()
        .len();

    let (err, set) = r.attempt(vec![push], r.since(10));
    assert_eq!(
        reason(&err),
        TmRejectReason::RawGitNotCovered,
        "TMC010-NFR01-WHOLE: a raw event of the period that is not undoable rejects the request"
    );
    assert_eq!(
        set,
        vec![OpRef::GitEvent(2), oplog_op(&b)],
        "TMC010-NFR01-WHOLE: the rejection records the whole set as Target::Undo"
    );
    let after = r
        .repo
        .repo
        .oplog
        .lock()
        .unwrap()
        .snapshots(&Default::default())
        .unwrap()
        .len();
    assert_eq!(
        after, oplog_before,
        "TMC010-NFR01-WHOLE: a rejection takes no prior snapshot"
    );
}

#[test]
fn the_target_is_the_prior_of_the_oldest() {
    let mark_unavailable = |r: &Rig, snapshot_id: &str| {
        r.repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .set_snapshot_state(snapshot_id, SnapshotState::PurgeIntent, now_ms())
            .unwrap();
    };

    // The prior of the newest is gone: the oldest's prior is the target, so the plan goes on.
    let r = rig();
    let now = now_ms();
    let (_b, _) = r.operation("b", now - 3_000, 1);
    let (_c, prior_c) = r.operation("c", now - 1_000, 2);
    mark_unavailable(&r, &prior_c);
    let (err, _) = r.attempt(Vec::new(), r.since(10));
    assert_eq!(
        reason(&err),
        TmRejectReason::GitUnavailable,
        "TMC010-TARGET: the target of the set is the prior of its oldest operation, not the newest's"
    );

    // The prior of the oldest is gone: there is no target and the whole request is refused.
    let r = rig();
    let now = now_ms();
    let (_b, prior_b) = r.operation("b", now - 3_000, 1);
    let (_c, _) = r.operation("c", now - 1_000, 2);
    mark_unavailable(&r, &prior_b);
    let (err, _) = r.attempt(Vec::new(), r.since(10));
    assert_eq!(
        reason(&err),
        TmRejectReason::TargetUnavailable,
        "TMC010-TARGET: the oldest operation's prior must be available and verified"
    );
}
