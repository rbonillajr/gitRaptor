//! US-MCP-013 T002: a gate that answers "ask" is applied as deny with its own reason and its own
//! single close, before any effect; `Deny` and `NotEvaluated` stay `guardrails-denied`.
//!
//! The executor is the real one, running against doubles of the repo layer (a backend that plans
//! a `commit` and counts its `step` calls, a snapshotter that counts its calls, a gate with a
//! fixed decision) and a real oplog in a temporary profile, never a real repo or profile
//! (NFR-01). Nothing here waits.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::catalog::{
    DecisionView, GovernedAs, Layer, OperationArgs, OperationId, PrepareParams, RejectReason,
    RunParams,
};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::{
    Affected, Caller, ExecError, Executor, GateDecision, GateRequest, GuardrailsGate, OpPlan,
    PlanClose, PlanError, PrepareInput, RepoFacts, RunDone, RunEnv, RunInput, StepPlan,
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

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000bb";
const CONNECTION: u64 = 7;

// ----- Doubles ---------------------------------------------------------------------------

/// A snapshotter that counts its calls and never answers.
struct CountingPrior(Arc<AtomicUsize>);

impl PriorSnapshotter for CountingPrior {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(PriorError::StoreUnavailable)
    }
}

struct Allow;

impl McpAllowlist for Allow {
    fn allows(&self, _repo_id: &str) -> bool {
        true
    }
}

fn facts() -> RepoFacts {
    RepoFacts {
        root: "/w/shop".into(),
        git_dir: "/w/shop/.git".into(),
        common_dir: "/w/shop/.git".into(),
        linked: false,
        root_id: Some((1, 2)),
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
    steps: AtomicUsize,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, _folder: &Path) -> Result<RepoHandle, ScopeError> {
        Ok(self.repo.clone())
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("us-mcp-013/{}", repo.repo_id)
    }
    fn facts(&self, _repo: &RepoHandle) -> Result<RepoFacts, gitraptor_api::catalog::RejectReason> {
        Ok(facts())
    }
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        _facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::Commit {
            return Err(PlanError::NotImplemented);
        }
        Ok(OpPlan {
            expected: json!({}),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: None,
        })
    }
    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.steps.fetch_add(1, Ordering::SeqCst);
        Err(StepError::new("the step must not be reached"))
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

/// A gate with a fixed decision, a count of its `evaluate` calls and the closes it saw.
struct Gate {
    decision: GateDecision,
    evaluations: AtomicUsize,
    closes: Mutex<Vec<(Layer, GovernedAs, PlanClose)>>,
}

impl Gate {
    fn new(decision: GateDecision) -> Arc<Self> {
        Arc::new(Self {
            decision,
            evaluations: AtomicUsize::new(0),
            closes: Mutex::new(Vec::new()),
        })
    }
}

impl GuardrailsGate for Gate {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        self.decision
    }
    fn record_close(&self, req: &GateRequest, close: PlanClose) {
        self.closes
            .lock()
            .unwrap()
            .push((req.layer, req.operation, close));
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    oplog: Arc<Mutex<Oplog>>,
    backend: Backend,
    gate: Arc<Gate>,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
    priors: Arc<AtomicUsize>,
}

fn rig(decision: GateDecision) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let oplog = Arc::new(Mutex::new(oplog));
    let priors = Arc::new(AtomicUsize::new(0));
    let repo = RepoHandle {
        repo_id: REPO.into(),
        worktree: "/w/shop".into(),
        common_dir: "/w/shop/.git".into(),
        oplog: Arc::clone(&oplog),
        snapshotter: Arc::new(CountingPrior(Arc::clone(&priors))),
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
    let gate = Gate::new(decision);
    Rig {
        _tmp: tmp,
        oplog,
        backend: Backend {
            repo,
            allow: Allow,
            steps: AtomicUsize::new(0),
        },
        executor: Executor::new(gate.clone()),
        gate,
        resolution: Resolution {
            who,
            via: ResolvedVia::Ancestry,
            executor_operation: None,
            confirmable: false,
        },
        marks: Arc::new(ExecutorMarks::default()),
        stopping: AtomicBool::new(false),
        priors,
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
    /// Prepares a `commit` plan over MCP; its id and the decision the preview showed.
    fn prepare(&self) -> (String, DecisionView) {
        let mut args = Map::new();
        args.insert("message".into(), json!("fix: a"));
        args.insert("paths".into(), json!(["src/a.rs"]));
        let params = PrepareParams {
            operation: OperationId::Commit,
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
            .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"));
        (prepared.plan_id, prepared.decision)
    }

    fn run(&self, plan_id: &str) -> Result<RunDone, ExecError> {
        let params = RunParams {
            plan_id: plan_id.to_owned(),
            accepted_warnings: Vec::new(),
            confirmation: None,
        };
        let again = || Ok((self.resolution.clone(), Layer::Mcp, None));
        let capture = |ask: &ManualAsk| {
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
            engine: None,
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
                rescope: &|_repo: &gitraptor_core::timemachine::protected::RepoHandle| Ok(()),
            },
            &env,
        )
    }

    fn steps(&self) -> usize {
        self.backend.steps.load(Ordering::SeqCst)
    }

    fn priors(&self) -> usize {
        self.priors.load(Ordering::SeqCst)
    }

    fn evaluations(&self) -> usize {
        self.gate.evaluations.load(Ordering::SeqCst)
    }

    fn closes(&self) -> Vec<(Layer, GovernedAs, PlanClose)> {
        self.gate.closes.lock().unwrap().clone()
    }

    /// Nothing was recorded in the repo's oplog.
    fn oplog_is_untouched(&self) -> bool {
        self.oplog.lock().unwrap().last_seq().unwrap() == 0
    }
}

fn asked() -> ExecError {
    ExecError::Rejected(RejectReason::GuardrailsConfirmationRequired)
}

// ----- Tests -------------------------------------------------------------------------------

/// MCP013-EXEC-REFUSED: an "ask" is refused with its own reason before any effect.
#[test]
fn an_ask_is_refused_with_its_own_reason_before_any_effect() {
    let rig = rig(GateDecision::Ask);
    let (plan, _) = rig.prepare();

    let err = rig.run(&plan).unwrap_err();

    assert_eq!(err, asked(), "{err:?}");
    assert_eq!(rig.steps(), 0, "the step was never reached");
    assert_eq!(rig.priors(), 0, "no prior snapshot was taken");
    assert!(rig.oplog_is_untouched(), "no point, no row");
}

/// MCP013-EXEC-CLOSE-ONCE: the plan closes once, with the MCP layer and its own close.
#[test]
fn an_ask_closes_the_plan_once_with_the_mcp_layer() {
    let rig = rig(GateDecision::Ask);
    let (plan, _) = rig.prepare();

    let _ = rig.run(&plan);

    assert_eq!(
        rig.closes(),
        [(Layer::Mcp, GovernedAs::Commit, PlanClose::AskedWithoutQueue)]
    );
}

/// MCP013-EXEC-RETRY: retrying a plan that was asked gets the same refusal and changes nothing.
#[test]
fn retrying_an_ask_gets_the_same_refusal_and_changes_nothing() {
    let rig = rig(GateDecision::Ask);
    let (first, _) = rig.prepare();
    assert_eq!(rig.run(&first).unwrap_err(), asked());

    // The agent tries again: a fresh plan, the same answer.
    let (second, _) = rig.prepare();
    let err = rig.run(&second).unwrap_err();

    assert_eq!(err, asked(), "{err:?}");
    assert_eq!(rig.steps(), 0);
    assert_eq!(rig.priors(), 0);
    assert!(rig.oplog_is_untouched());
}

/// MCP013-EXEC-PREVIEW: the preview shows the applied effect, deny.
#[test]
fn the_preview_of_an_ask_is_the_applied_deny() {
    let rig = rig(GateDecision::Ask);
    let (_, decision) = rig.prepare();
    assert_eq!(decision, DecisionView::Deny);
}

/// MCP013-EXEC-ONE-EVALUATE: `run` asks the gate once.
#[test]
fn the_gate_is_asked_once_at_run() {
    let rig = rig(GateDecision::Ask);
    let (plan, _) = rig.prepare();
    let at_prepare = rig.evaluations();

    let _ = rig.run(&plan);

    assert_eq!(rig.evaluations() - at_prepare, 1, "one evaluation at run");
}

/// MCP013-EXEC-DENY-KEPT: a deny and no engine stay `guardrails-denied`, closed as denied.
#[test]
fn a_deny_and_no_engine_keep_guardrails_denied() {
    for decision in [GateDecision::Deny, GateDecision::NotEvaluated] {
        let rig = rig(decision);
        let (plan, _) = rig.prepare();

        let err = rig.run(&plan).unwrap_err();

        assert!(
            matches!(err, ExecError::GuardrailsDenied(_)),
            "{decision:?}: {err:?}"
        );
        assert_eq!(
            rig.closes(),
            [(Layer::Mcp, GovernedAs::Commit, PlanClose::Denied)],
            "{decision:?}"
        );
        assert_eq!(rig.steps(), 0, "{decision:?}");
    }
}
