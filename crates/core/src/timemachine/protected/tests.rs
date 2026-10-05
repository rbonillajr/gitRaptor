//! Acceptance tests of the protected operation (TS-TMC-004). A real oplog in
//! a temporary profile; doubles for the snapshotter and the step (the
//! applier of TS-TMC-003 and the executor of F-001-02/05 are built
//! elsewhere). Never a real repo or profile (NFR-01).

use std::sync::atomic::AtomicUsize;

use super::*;
use crate::channel::peer::SystemProcs;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::{OperationState, Requester};

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";

struct Fixture {
    _tmp: tempfile::TempDir,
    oplog: Arc<Mutex<Oplog>>,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    Fixture {
        _tmp: tmp,
        oplog: Arc::new(Mutex::new(oplog)),
        marks: Arc::new(ExecutorMarks::default()),
        stopping: AtomicBool::new(false),
    }
}

/// A snapshotter double: answers `result` after `delay`, optionally asking
/// the daemon to stop while it runs.
struct Snap {
    result: Result<PriorSnapshot, PriorError>,
    delay: Duration,
    calls: AtomicUsize,
    stop_flag: Option<Arc<AtomicBool>>,
    /// Where a successful answer records its complete snapshot row, as the
    /// real store does.
    oplog: Option<Arc<Mutex<Oplog>>>,
}

impl Snap {
    fn ok() -> Self {
        Self::answer(Ok(PriorSnapshot {
            snapshot_id: "snap-1".into(),
            fast_path: true,
        }))
    }
    fn answer(result: Result<PriorSnapshot, PriorError>) -> Self {
        Self {
            result,
            delay: Duration::ZERO,
            calls: AtomicUsize::new(0),
            stop_flag: None,
            oplog: None,
        }
    }
}

impl PriorSnapshotter for Snap {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        if let Some(flag) = &self.stop_flag {
            flag.store(true, Ordering::SeqCst);
        }
        let mut answer = self.result.clone()?;
        if let Some(oplog) = &self.oplog {
            let mut log = oplog.lock().unwrap();
            let new = crate::timemachine::oplog::NewSnapshot {
                level: crate::timemachine::oplog::SnapshotLevel::GuaranteedPrior,
                worktrees: vec!["/repo/main".into()],
                engine_mark: req.engine_mark,
                cause_operation: Some(req.operation_id.clone()),
                cause_event_seq: None,
            };
            let id = log.begin_snapshot(&new, 2_000).unwrap();
            log.complete_snapshot(&id, &Default::default(), 2_000)
                .unwrap();
            answer.snapshot_id = id;
        }
        Ok(answer)
    }
}

/// A step double that records whether it ran.
#[derive(Default)]
struct Step {
    ran: bool,
    fail: bool,
    refs: Vec<String>,
}

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        "checkout"
    }
    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.ran = true;
        assert!(!ctx.prior_snapshot_id().is_empty());
        ctx.applying()?;
        if self.fail {
            return Err(StepError::new("checkout failed"));
        }
        Ok(StepOutput {
            changed_refs: self.refs.clone(),
            ..StepOutput::default()
        })
    }
}

fn request() -> ProtectedRequest {
    ProtectedRequest {
        kind: OperationKind::Protected,
        scope: Scope {
            worktrees: vec!["/repo/main".into()],
            refs: vec![],
        },
        worktree_paths: vec![PathBuf::from("/repo/main")],
        who: Who::unattributed(),
        channel: Channel::Cli,
        confirmed: false,
        target: Target::None,
        warnings: vec![],
        engine_mark: 7,
    }
}

impl Fixture {
    fn snap(&self) -> Snap {
        let mut s = Snap::ok();
        s.oplog = Some(Arc::clone(&self.oplog));
        s
    }
}

fn run_with(
    f: &Fixture,
    snap: Arc<dyn PriorSnapshotter>,
    stopping: &AtomicBool,
    deadline: Duration,
    step: &mut Step,
) -> Result<ProtectedOutcome, ProtectedError> {
    ProtectedOperation {
        oplog: &f.oplog,
        snapshotter: snap,
        marks: &f.marks,
        procs: &SystemProcs,
        stopping,
        deadline,
    }
    .run(&request(), step)
}

fn state(f: &Fixture, id: &str) -> OperationState {
    f.oplog
        .lock()
        .unwrap()
        .operation(id)
        .unwrap()
        .unwrap()
        .state
}

fn reason_of(err: &ProtectedError) -> (PriorFailure, String) {
    match err {
        ProtectedError::Prior {
            reason,
            operation_id,
            ..
        } => (*reason, operation_id.clone().unwrap()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn intent_prior_execution_and_record_in_order() {
    let f = fixture();
    let mut step = Step {
        refs: vec!["refs/heads/main".into()],
        ..Step::default()
    };
    let out = run_with(
        &f,
        Arc::new(f.snap()),
        &f.stopping,
        DEFAULT_PRIOR_DEADLINE,
        &mut step,
    )
    .unwrap();
    assert!(step.ran);
    assert!(out.prior.fast_path);
    assert_eq!(out.output.changed_refs, ["refs/heads/main"]);
    let log = f.oplog.lock().unwrap();
    let view = log.operation(&out.operation_id).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Finished);
    assert_eq!(view.prior_snapshot.as_ref(), Some(&out.prior.snapshot_id));
    assert_eq!(view.record.subtype.as_deref(), Some("checkout"));
    assert_eq!(view.record.requester, Requester::Unattributed);
    assert_eq!(view.record.channel, Channel::Cli);
    let states: Vec<_> = log
        .journal(&out.operation_id)
        .unwrap()
        .into_iter()
        .filter_map(|e| e.state)
        .collect();
    assert_eq!(
        states,
        [
            "intent",
            "prior-snapshot",
            "ready",
            "applying",
            "applying",
            "finished"
        ]
    );
    // The marks close with the operation.
    assert!(f.marks.is_empty());
}

/// US-TMC-001 scenario 4: the store has no space, the step never runs.
#[test]
fn no_space_aborts_without_running_the_step() {
    let f = fixture();
    let mut step = Step::default();
    let err = run_with(
        &f,
        Arc::new(Snap::answer(Err(PriorError::NoSpace))),
        &f.stopping,
        DEFAULT_PRIOR_DEADLINE,
        &mut step,
    )
    .unwrap_err();
    assert!(!step.ran);
    let (reason, id) = reason_of(&err);
    assert_eq!(reason, PriorFailure::NoSpace);
    assert_eq!(state(&f, &id), OperationState::Aborted);
}

#[test]
fn a_full_disk_from_the_store_reads_as_no_space() {
    let enospc = std::io::Error::from_raw_os_error(28);
    assert_eq!(
        PriorError::from(CaptureError::Io(enospc)),
        PriorError::NoSpace
    );
    let store = gitraptor_git::tm_write::store::StoreError::Untrusted("owner".into());
    assert_eq!(
        PriorError::from(CaptureError::Store(store)),
        PriorError::StoreUnavailable
    );
    assert!(matches!(
        PriorError::from(CaptureError::InvalidInput("x".into())),
        PriorError::Failed(_)
    ));
}

#[test]
fn unavailable_store_and_capture_failures_abort() {
    for (answer, want) in [
        (PriorError::StoreUnavailable, PriorFailure::StoreUnavailable),
        (
            PriorError::Failed("boom".into()),
            PriorFailure::CaptureFailed,
        ),
    ] {
        let f = fixture();
        let mut step = Step::default();
        let err = run_with(
            &f,
            Arc::new(Snap::answer(Err(answer))),
            &f.stopping,
            DEFAULT_PRIOR_DEADLINE,
            &mut step,
        )
        .unwrap_err();
        assert!(!step.ran);
        let (reason, id) = reason_of(&err);
        assert_eq!(reason, want);
        assert_eq!(state(&f, &id), OperationState::Aborted);
    }
}

#[test]
fn timeout_aborts() {
    let f = fixture();
    let mut snap = f.snap();
    snap.delay = Duration::from_millis(400);
    let mut step = Step::default();
    let err = run_with(
        &f,
        Arc::new(snap),
        &f.stopping,
        Duration::from_millis(30),
        &mut step,
    )
    .unwrap_err();
    assert!(!step.ran);
    let (reason, id) = reason_of(&err);
    assert_eq!(reason, PriorFailure::Timeout);
    assert_eq!(state(&f, &id), OperationState::Aborted);
}

#[test]
fn stopping_daemon_aborts() {
    // Already stopping: recorded and aborted, no snapshot taken.
    let f = fixture();
    f.stopping.store(true, Ordering::SeqCst);
    let snap = Arc::new(f.snap());
    let mut step = Step::default();
    let err = run_with(
        &f,
        snap.clone(),
        &f.stopping,
        DEFAULT_PRIOR_DEADLINE,
        &mut step,
    )
    .unwrap_err();
    assert!(!step.ran);
    assert_eq!(snap.calls.load(Ordering::SeqCst), 0);
    let (reason, id) = reason_of(&err);
    assert_eq!(reason, PriorFailure::DaemonStopping);
    assert_eq!(state(&f, &id), OperationState::Aborted);

    // Starts stopping during the snapshot: the step still does not run.
    let f = fixture();
    let flag = Arc::new(AtomicBool::new(false));
    let mut snap = f.snap();
    snap.stop_flag = Some(Arc::clone(&flag));
    let mut step = Step::default();
    let err = run_with(&f, Arc::new(snap), &flag, DEFAULT_PRIOR_DEADLINE, &mut step).unwrap_err();
    assert!(!step.ran);
    let (reason, id) = reason_of(&err);
    assert_eq!(reason, PriorFailure::DaemonStopping);
    let log = f.oplog.lock().unwrap();
    let view = log.operation(&id).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Aborted);
    assert!(view.prior_snapshot.is_some());
}

#[test]
fn a_failed_step_is_interrupted_with_its_prior_snapshot() {
    let f = fixture();
    let mut step = Step {
        fail: true,
        ..Step::default()
    };
    let err = run_with(
        &f,
        Arc::new(f.snap()),
        &f.stopping,
        DEFAULT_PRIOR_DEADLINE,
        &mut step,
    )
    .unwrap_err();
    let ProtectedError::Step {
        operation_id,
        message,
    } = err
    else {
        panic!("{err:?}")
    };
    assert_eq!(message, "checkout failed");
    let log = f.oplog.lock().unwrap();
    let view = log.operation(&operation_id).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Interrupted);
    assert!(view.prior_snapshot.is_some());
}

/// DEP-MCP-3: a child the step starts is marked with the operation's
/// requester while the operation runs, and annotated in the journal.
#[cfg(unix)]
#[test]
fn children_of_the_step_are_marked() {
    struct Spawner {
        seen: Option<crate::channel::marks::MarkedBy>,
    }
    impl ProtectedStep for Spawner {
        fn subtype(&self) -> &str {
            "fetch-free"
        }
        fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
            let child = ctx
                .spawn(Command::new("/bin/sleep").arg("0.3"))
                .map_err(|e| StepError::new(e.to_string()))?;
            let info = SystemProcs.read(child.pid).unwrap();
            self.seen = ctx.marks.lookup(&info);
            ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
            Ok(StepOutput::default())
        }
    }
    let f = fixture();
    let mut who = Who::unattributed();
    who.requester = Requester::Agent {
        name: "claude-code".into(),
        origin: crate::timemachine::oplog::RequesterOrigin::Detected,
        session_id: "25:250".into(),
    };
    let mut req = request();
    req.who = who.clone();
    let mut step = Spawner { seen: None };
    let out = ProtectedOperation {
        oplog: &f.oplog,
        snapshotter: Arc::new(f.snap()),
        marks: &f.marks,
        procs: &SystemProcs,
        stopping: &f.stopping,
        deadline: DEFAULT_PRIOR_DEADLINE,
    }
    .run(&req, &mut step)
    .unwrap();
    let seen = step.seen.expect("the child is marked");
    assert_eq!(seen.operation_id, out.operation_id);
    assert_eq!(seen.who, who);
    assert!(f.marks.is_empty());
    let entries: Vec<_> = f
        .oplog
        .lock()
        .unwrap()
        .journal(&out.operation_id)
        .unwrap()
        .into_iter()
        .map(|e| e.entry)
        .collect();
    assert!(entries.iter().any(|e| e == "child-started"));
    assert!(entries.iter().any(|e| e == "child-ended"));
}
