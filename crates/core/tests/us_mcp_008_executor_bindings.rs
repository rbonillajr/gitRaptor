//! The bindings of a `snapshot` plan (DS-US-MCP-008 T1 and the allowlist by requester): a plan
//! belongs to the connection that prepared it, expires, is capped per connection, and a direct
//! client running under an agent is held to the allowlist like the MCP. A manual snapshot also
//! never waits for the repo's write lock.
//!
//! The executor runs against doubles of the repo layer and a real oplog in a temporary profile
//! (NFR-01). Waits are bounded by an explicit signal, never a sleep.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::catalog::{
    Layer, MAX_PLANS_PER_CONNECTION, OperationArgs, OperationId, PrepareParams, RejectReason,
    RunParams,
};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::{
    Affected, Caller, ExecError, Executor, NoGuardrails, OpPlan, PlanError, PrepareInput,
    RepoFacts, RunDone, RunEnv, RunInput, StepPlan,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::repo_lock;
use gitraptor_core::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedBackend, ProtectedStep,
    RepoHandle, ScopeError, StepError,
};
use serde_json::{Map, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const CONNECTION: u64 = 7;

// ----- Doubles ---------------------------------------------------------------------------

/// The repo layer's snapshotter is never used by a manual snapshot.
struct NoPrior;

impl PriorSnapshotter for NoPrior {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        Err(PriorError::StoreUnavailable)
    }
}

struct Allow(AtomicBool);

impl McpAllowlist for Allow {
    fn allows(&self, _repo_id: &str) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn facts(root_inode: u64) -> RepoFacts {
    RepoFacts {
        root: "/w/shop".into(),
        git_dir: "/w/shop/.git".into(),
        common_dir: "/w/shop/.git".into(),
        linked: false,
        root_id: Some((1, root_inode)),
        dot_git_id: Some((1, 3)),
        gitdir_linked_back: true,
        head_branch: Some("main".into()),
        head_commit: Some("1".repeat(40)),
        detached: false,
        in_progress: None,
        git_locks: Vec::new(),
        locked: false,
        grafts: false,
        branches_elsewhere: Vec::new(),
    }
}

struct Backend {
    repo: RepoHandle,
    allow: Allow,
    facts: Mutex<RepoFacts>,
    /// Unique per rig: tests of this file run in parallel in one process.
    key: String,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, _folder: &Path) -> Result<RepoHandle, ScopeError> {
        Ok(self.repo.clone())
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("us-mcp-008-bindings/{}/{}", repo.repo_id, self.key)
    }
    fn facts(&self, _repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        Ok(self.facts.lock().unwrap().clone())
    }
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        _facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::Snapshot {
            return Err(PlanError::NotImplemented);
        }
        Ok(OpPlan {
            expected: json!({}),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
        })
    }
    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        // A snapshot never runs as a protected operation.
        Err(StepError::new("a snapshot has no protected step"))
    }
}

/// No process is read: a manual snapshot does not need the process table.
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
    _tmp: tempfile::TempDir,
    oplog: Arc<Mutex<Oplog>>,
    backend: Backend,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
    captures: AtomicUsize,
}

fn rig() -> Rig {
    rig_with(Executor::new(Arc::new(NoGuardrails)))
}

fn rig_with(executor: Executor) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_key = tmp.path().to_string_lossy().into_owned();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let oplog = Arc::new(Mutex::new(oplog));
    let repo = RepoHandle {
        repo_id: REPO.into(),
        worktree: "/w/shop".into(),
        oplog: Arc::clone(&oplog),
        snapshotter: Arc::new(NoPrior),
    };
    let who = Who {
        actor: Actor::Agent {
            kind: actor::AgentKind::ClaudeCode,
            name: None,
            origin: actor::AgentOrigin::Detected,
        },
        requester: Requester::Agent {
            name: "claude".into(),
            origin: RequesterOrigin::Detected,
            session_id: "session-1".into(),
        },
    };
    Rig {
        _tmp: tmp,
        oplog,
        backend: Backend {
            repo,
            allow: Allow(AtomicBool::new(true)),
            facts: Mutex::new(facts(2)),
            key: tmp_key,
        },
        executor,
        resolution: Resolution {
            who,
            via: ResolvedVia::Ancestry,
            executor_operation: None,
            confirmable: false,
        },
        marks: Arc::new(ExecutorMarks::default()),
        stopping: AtomicBool::new(false),
        captures: AtomicUsize::new(0),
    }
}

fn caller() -> Caller {
    caller_of(CONNECTION)
}

fn caller_of(connection: u64) -> Caller {
    Caller {
        connection,
        pid: 4_242,
        start_us: 1,
        mcp: true,
    }
}

impl Rig {
    /// Prepares a `snapshot` plan as `caller` over `channel`.
    fn prepare_as(&self, caller: Caller, channel: RequestChannel) -> Result<String, ExecError> {
        let mut args = Map::new();
        args.insert("label".into(), json!("before the migration"));
        let params = PrepareParams {
            operation: OperationId::Snapshot,
            worktree: None,
            args,
            surface: None,
            session_env: Vec::new(),
        };
        self.executor
            .prepare(
                &self.backend,
                PrepareInput {
                    caller,
                    resolution: &self.resolution,
                    layer: Layer::Mcp,
                    channel,
                    params: &params,
                    repo: self.backend.repo.clone(),
                    confirm_refusal: None,
                },
            )
            .map(|p| p.plan_id)
    }

    fn prepare(&self) -> String {
        self.prepare_as(caller(), RequestChannel::Mcp)
            .unwrap_or_else(|e| panic!("prepare of a snapshot plan: {e:?}"))
    }

    fn run_as(&self, caller: Caller, plan_id: &str) -> Result<RunDone, ExecError> {
        let params = RunParams {
            plan_id: plan_id.to_owned(),
            accepted_warnings: Vec::new(),
            confirmation: None,
        };
        let again = || Ok((self.resolution.clone(), Layer::Mcp, None));
        let capture = |ask: &ManualAsk| {
            self.captures.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ManualError>(ManualCaptured {
                snapshot_id: "00000000-0000-4000-8000-000000000001".into(),
                worktree: ask.worktree.clone(),
            })
        };
        let engine_mark = |_repo: &str, _worktrees: &[PathBuf]| Some(1);
        let after_step = |_repo: &str, _worktrees: &[PathBuf], _operation: &str| {};
        let publish = |_kind: &str, _data| {};
        let env = RunEnv {
            marks: &self.marks,
            procs: &NoProcs,
            stopping: &self.stopping,
            prior_deadline: Duration::from_secs(10),
            engine_mark: &engine_mark,
            after_step: &after_step,
            publish: &publish,
            capture: &capture,
        };
        self.executor.run_any(
            &self.backend,
            RunInput {
                caller,
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &RepoHandle| Ok(()),
            },
            &env,
        )
    }

    fn run(&self, plan_id: &str) -> Result<RunDone, ExecError> {
        self.run_as(caller(), plan_id)
    }

    fn captures(&self) -> usize {
        self.captures.load(Ordering::SeqCst)
    }

    fn oplog_is_untouched(&self) -> bool {
        self.oplog.lock().unwrap().last_seq().unwrap() == 0
    }
}

/// A direct client (declares the CLI) that runs under an agent.
fn direct() -> Caller {
    Caller {
        mcp: false,
        ..caller()
    }
}

// ----- Tests -------------------------------------------------------------------------------

/// T1: a plan prepared by one connection is unknown to another.
#[test]
fn a_plan_run_by_another_connection_is_plan_unknown() {
    let rig = rig();
    let plan = rig.prepare();

    let err = rig.run_as(caller_of(CONNECTION + 1), &plan).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::PlanUnknown),
        "{err:?}"
    );
    assert_eq!(rig.captures(), 0);
    assert!(rig.oplog_is_untouched());
}

/// T1: an expired plan is unknown (the lifetime is injected: zero expires at once).
#[test]
fn an_expired_snapshot_plan_is_plan_unknown() {
    let rig = rig_with(
        Executor::new(Arc::new(NoGuardrails)).with_limits(Duration::ZERO, Duration::from_secs(10)),
    );
    let plan = rig.prepare();

    let err = rig.run(&plan).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::PlanUnknown),
        "{err:?}"
    );
    assert_eq!(rig.captures(), 0);
    assert!(rig.oplog_is_untouched());
}

/// T1: more pending plans than the per-connection ceiling are refused.
#[test]
fn more_pending_plans_than_the_ceiling_are_refused() {
    let rig = rig();
    for _ in 0..MAX_PLANS_PER_CONNECTION {
        rig.prepare();
    }

    let err = rig.prepare_as(caller(), RequestChannel::Mcp).unwrap_err();

    assert_eq!(err, ExecError::TooManyPlans, "{err:?}");
}

/// The allowlist follows the requester, not the declared channel: a direct client under an
/// agent cannot snapshot a repo that is off it.
#[test]
fn a_direct_client_under_the_agent_cannot_snapshot_a_repo_off_the_allowlist() {
    let rig = rig();
    rig.backend.allow.0.store(false, Ordering::SeqCst);

    let err = rig.prepare_as(direct(), RequestChannel::Cli).unwrap_err();

    assert_eq!(err, ExecError::Scope(ScopeError::NotAllowlisted), "{err:?}");
    assert_eq!(rig.captures(), 0);
    assert!(rig.oplog_is_untouched(), "no plan, no row");
}

/// The same at `run`: disabled after a direct client prepared it.
#[test]
fn a_direct_client_under_the_agent_is_held_to_the_allowlist_at_run() {
    let rig = rig();
    let plan = rig.prepare_as(direct(), RequestChannel::Cli).unwrap();
    rig.backend.allow.0.store(false, Ordering::SeqCst);

    let err = rig.run_as(direct(), &plan).unwrap_err();

    assert_eq!(err, ExecError::Scope(ScopeError::NotAllowlisted), "{err:?}");
    assert_eq!(rig.captures(), 0);
    assert!(rig.oplog_is_untouched());
}

/// A manual snapshot does not take the repo's write lock: with another holder of it, it
/// captures without waiting.
#[test]
fn snapshot_runs_without_the_repo_write_lock() {
    let rig = rig();
    let plan = rig.prepare();
    let held = repo_lock::try_lock(&rig.backend.write_lock_key(&rig.backend.repo))
        .expect("the key is unique to this rig, so it is free");
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = tx.send(rig.run(&plan));
        });
        let done = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the snapshot must not wait for the write lock");
        assert!(matches!(done, Ok(RunDone::Captured(_))), "{done:?}");
    });

    assert_eq!(rig.captures(), 1);
    drop(held);
}
