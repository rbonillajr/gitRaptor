//! Review of the undo by period (US-TMC-010): the walk stops at a moment ahead of the clock and
//! at an older raw event. Each operation here has its anchor, so a raw event after it is the
//! stack's own item, not its echo. Child module of `timemachine::undo`; the rig is the one of
//! `since_tests` plus the anchor. Temporary repo and profile only.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::channel::peer::{ProcError, ProcInfo};
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::EngineLink;
use crate::timemachine::oplog::{NewOperation, OperationState, OperationTransition, SnapshotLevel};
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
        drop(log);
        // Its anchor, at its own mark: the operation's echo ends there, so a raw event after it
        // is the stack's own item.
        store
            .capture(
                oplog,
                &CaptureRequest {
                    level: SnapshotLevel::Observation,
                    echo_settled: Some(true),
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
            .unwrap();
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

    /// The period starts `secs_ago` before `now`, the instant the test anchored its operations at:
    /// never the wall clock at call time, which drifts by seconds on a loaded runner (the real
    /// snapshots of `operation` are slow on Windows) and would push them out of the period.
    fn since(&self, now: i64, secs_ago: i64) -> Selection {
        Selection::Since {
            since_ms: now - secs_ago * 1_000,
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

fn commit_event(seq: i64, observed_ms: i64, kind: GitEventKind) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind,
        branch: Some("main".into()),
        actor: Requester::Unattributed,
        observed_ms,
        in_gap: false,
        inferred: false,
        old_commit: None,
        new_commit: None,
    }
}

#[test]
fn a_moment_ahead_of_the_clock_stops_the_walk() {
    let r = rig();
    let now = now_ms();
    let (_a, _) = r.operation("a", now - 1_000, 1);
    // Recorded an hour ahead: the clock stepped back since. It is never undone silently.
    let (_future, _) = r.operation("future", now + 3_600_000, 2);

    let (err, set) = r.attempt(Vec::new(), r.since(now, 10));
    assert_eq!(reason(&err), TmRejectReason::NothingToUndo);
    assert!(
        set.is_empty(),
        "nothing above the future one is taken: {set:?}"
    );

    let (c, _) = r.operation("c", now - 500, 3);
    let (_, set) = r.attempt(Vec::new(), r.since(now, 10));
    assert_eq!(
        set,
        vec![oplog_op(&c)],
        "the walk stops at the future one, and nothing below it is undone"
    );
}

#[test]
fn an_older_raw_commit_between_two_operations_stops_the_walk() {
    let r = rig();
    let now = now_ms();
    let (_a, _) = r.operation("a", now - 60_000, 1);
    let commit = commit_event(2, now - 30_000, GitEventKind::Commit);
    let (b, _) = r.operation("b", now - 1_000, 3);

    let (_, set) = r.attempt(vec![commit.clone()], r.since(now, 10));
    assert_eq!(set, vec![oplog_op(&b)], "only the recent operation");

    // The commit is the stack's own item: a period that holds it takes it too.
    let (_, set) = r.attempt(vec![commit], r.since(now, 45));
    assert_eq!(set, vec![OpRef::GitEvent(2), oplog_op(&b)]);
}
