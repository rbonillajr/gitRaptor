//! `Executor::run_any` for a `snapshot` plan when the daemon is stopping: the run is refused and
//! never reaches the capture. The rig is a copy of the one in `us_mcp_008_executor.rs`.
//!
//! The executor runs against doubles of the repo layer (a backend with a toggleable allowlist
//! and toggleable facts, a capture that counts its calls) and a real oplog in a temporary
//! profile, never a real repo or profile (NFR-01). Nothing here waits.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::catalog::{
    Layer, OperationArgs, OperationId, PrepareParams, RejectReason, RunParams,
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
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, _folder: &Path) -> Result<RepoHandle, ScopeError> {
        Ok(self.repo.clone())
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("us-mcp-008-executor/{}", repo.repo_id)
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
    let tmp = tempfile::tempdir().unwrap();
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
        },
        executor: Executor::new(Arc::new(NoGuardrails)),
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
    Caller {
        connection: CONNECTION,
        pid: 4_242,
        start_us: 1,
        mcp: true,
    }
}

impl Rig {
    /// Prepares a `snapshot` plan over MCP; its id.
    fn prepare(&self) -> String {
        let mut args = Map::new();
        args.insert("label".into(), json!("before the migration"));
        let params = PrepareParams {
            operation: OperationId::Snapshot,
            worktree: None,
            args,
            surface: None,
            session_env: Vec::new(),
        };
        let prepared = self
            .executor
            .prepare(
                &self.backend,
                PrepareInput {
                    caller: caller(),
                    resolution: &self.resolution,
                    layer: Layer::Mcp,
                    channel: RequestChannel::Mcp,
                    params: &params,
                    repo: self.backend.repo.clone(),
                    confirm_refusal: None,
                },
            )
            .unwrap_or_else(|e| panic!("prepare of a snapshot plan: {e:?}"));
        prepared.plan_id
    }

    fn run(&self, plan_id: &str) -> Result<RunDone, ExecError> {
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
                caller: caller(),
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &RepoHandle| Ok(()),
            },
            &env,
        )
    }

    fn captures(&self) -> usize {
        self.captures.load(Ordering::SeqCst)
    }

    /// Nothing was recorded in the repo's oplog.
    fn oplog_is_untouched(&self) -> bool {
        self.oplog.lock().unwrap().last_seq().unwrap() == 0
    }
}

// ----- Tests -------------------------------------------------------------------------------

/// A daemon that is stopping runs no snapshot: refused with its own reason, nothing captured,
/// nothing recorded.
#[test]
fn a_stopping_daemon_runs_no_snapshot() {
    let rig = rig();
    let plan = rig.prepare();
    rig.stopping.store(true, Ordering::SeqCst);

    let err = rig.run(&plan).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::DaemonStopping),
        "{err:?}"
    );
    assert_eq!(rig.captures(), 0, "the capture was never reached");
    assert!(rig.oplog_is_untouched(), "no point, no row");
}
