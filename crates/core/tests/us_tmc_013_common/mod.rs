//! Shared arnés of the US-TMC-013 channel tests: a real daemon in this process with its own repo
//! layer and snapshot store, a test catalog whose operation is a real `git reset --hard`, and the
//! work of an agent written into the oplog as the executor would record it. Temporary repos and
//! profile only (NFR-01). Nothing waits on time: every step waits for the daemon's answer.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{OperationRunResult, TmConfirmData};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{ChannelConfig, TestConfirmation};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationFilter, OperationKind, OperationState, OperationTransition,
    OperationView, Oplog, Requester, RequesterOrigin, Scope, Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

pub fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

pub fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

/// The session of the agent whose work the scenarios undo (`claude-1`).
pub const AGENT_SESSION: &str = "4242:1";

// ----- Test catalog: a real `git reset --hard` as the operation to undo ---------------------

struct Catalog {
    fx: Arc<Fixture>,
}

struct Reset {
    fx: Arc<Fixture>,
    cwd: PathBuf,
}

impl OperationCatalog for Catalog {
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::AbortInProgress {
            return Err(PlanError::NotImplemented);
        }
        Ok(OpPlan {
            expected: json!({ "head": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
        })
    }

    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Ok(Box::new(Reset {
            fx: Arc::clone(&self.fx),
            cwd: plan.repo.worktree.clone(),
        }))
    }
}

impl ProtectedStep for Reset {
    fn subtype(&self) -> &str {
        "reset-hard"
    }

    fn scope(&self) -> StepScope {
        StepScope::default()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        let mut cmd = self.fx.git_command(&self.cwd, &["reset", "-q", "--hard"]);
        cmd.stdout(std::process::Stdio::null());
        let child = ctx
            .spawn(&mut cmd)
            .map_err(|e| StepError::new(e.to_string()))?;
        let status = ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
        if status.success() {
            Ok(StepOutput::default())
        } else {
            Err(StepError::new("git reset failed".to_owned()))
        }
    }
}

struct AllowAll;

impl GuardrailsGate for AllowAll {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

// ----- Running daemon --------------------------------------------------------------------------

pub struct Running {
    pub fx: Arc<Fixture>,
    pub tp: TempProfile,
    pub worktree: PathBuf,
    pub oplog: Arc<Mutex<Oplog>>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

/// A repo with `a.rs` committed and a linked worktree `feat-login`.
pub fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("a.rs", "fn a() {}\n");
    fx.git(&["add", "a.rs"]);
    fx.git(&["commit", "-q", "-m", "a"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

/// Starts the daemon over `fx`, with the confirmation seam and capabilities of the case.
pub fn start(
    fx: Fixture,
    worktree: PathBuf,
    test_confirmation: Option<TestConfirmation>,
    capabilities: Option<Vec<&'static str>>,
) -> Running {
    let fx = Arc::new(fx);
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    let wiring = OperationsWiring {
        catalog: Arc::new(Catalog {
            fx: Arc::clone(&fx),
        }),
        gate: Arc::new(AllowAll),
        test_layer_override: Some(Layer::Cockpit),
        prior_deadline: Duration::from_secs(30),
        prior_layer: None,
    };
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let mut channel = ChannelConfig {
        test_confirmation,
        ..ChannelConfig::default()
    };
    if let Some(capabilities) = capabilities {
        channel.capabilities = capabilities;
    }
    let daemon = Daemon::start(DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel,
        protected: None,
        operations: Some(wiring),
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        // A capture of its own would add points the repo-intact checks do not expect.
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: false,
                ..CaptureConfig::default()
            },
            ..Default::default()
        },
    })
    .unwrap();
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        fx,
        tp,
        worktree,
        oplog,
        handle,
        join: Some(join),
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

pub fn connect(tp: &TempProfile, kind: ClientKind) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), kind, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// What an operation of the oplog that finished or was refused shows.
pub struct Recorded {
    pub view: OperationView,
    /// The reason of a rejection, as written in the journal.
    pub reason: Option<String>,
}

impl Running {
    pub fn client(&self) -> Client {
        connect(&self.tp, ClientKind::Cli)
    }

    pub fn mcp_client(&self) -> Client {
        connect(&self.tp, ClientKind::Mcp)
    }

    /// Runs a real `git reset --hard` as an unattributed operation (prepare, then run).
    pub fn reset_hard(&self) -> OperationRunResult {
        let mut c = self.client();
        let plan: PrepareResult = c
            .call(
                methods::OPERATION_PREPARE,
                json!({
                    "operation": "abort-in-progress",
                    "worktree": self.worktree.to_str().unwrap(),
                    "surface": "tui",
                }),
            )
            .unwrap();
        let out: Value = c
            .call(
                methods::OPERATION_RUN,
                json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
            )
            .unwrap();
        serde_json::from_value(out).unwrap()
    }

    /// Writes the operation of an agent into the oplog on top of the stack, with the prior
    /// snapshot of a real one (as `apps/cli/tests/undo_process.rs` does). Returns the dirty
    /// content the agent's reset threw away and the operation id.
    pub fn agent_work(&self, session: &str) -> AgentWork {
        let dirty = format!(
            "fn a() {{ trabajo_del_agente_{}(); }}\n",
            session.replace(':', "_")
        );
        std::fs::write(self.worktree.join("a.rs"), &dirty).unwrap();
        let real = self.reset_hard();
        let mut log = self.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![self.worktree.to_string_lossy().into_owned()],
                        refs: Vec::new(),
                    },
                    requester: Requester::Agent {
                        name: "claude-code".into(),
                        origin: RequesterOrigin::Detected,
                        session_id: session.into(),
                    },
                    channel: Channel::Mcp,
                    confirmed: false,
                    target: Target::None,
                    warnings: Vec::new(),
                    engine_mark: i64::MAX / 2,
                },
                1,
            )
            .unwrap();
        for step in [
            OperationTransition::PriorSnapshot {
                snapshot_id: &real.prior_snapshot_id,
            },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, 2).unwrap();
        }
        AgentWork {
            operation_id: id,
            prior_snapshot_id: real.prior_snapshot_id,
            dirty,
        }
    }

    /// Every operation of the oplog, oldest first, with the reason of the rejected ones.
    pub fn recorded(&self) -> Vec<Recorded> {
        let log = self.oplog.lock().unwrap();
        let mut views = log.operations(&OperationFilter::default()).unwrap();
        views.sort_by_key(|v| v.record.seq);
        views
            .into_iter()
            .map(|view| {
                let reason = (view.state == OperationState::Rejected)
                    .then(|| {
                        log.journal(&view.record.operation_id)
                            .unwrap()
                            .into_iter()
                            .rev()
                            .find(|j| j.state.as_deref() == Some("rejected"))
                            .and_then(|j| j.detail)
                    })
                    .flatten();
                Recorded { view, reason }
            })
            .collect()
    }

    /// The requests of the Time Machine's own commands (`kind` undo or restore), oldest first.
    pub fn requests(&self, kind: OperationKind) -> Vec<Recorded> {
        self.recorded()
            .into_iter()
            .filter(|r| r.view.record.kind == kind)
            .collect()
    }

    pub fn a_rs(&self) -> String {
        std::fs::read_to_string(self.worktree.join("a.rs")).unwrap()
    }
}

pub struct AgentWork {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    /// What `a.rs` held before the agent's reset: what taking the work back restores.
    pub dirty: String,
}

// ----- Calls -------------------------------------------------------------------------------------

pub fn params(worktree: &Path, token: Option<&str>) -> Value {
    let mut p = json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" });
    if let Some(token) = token {
        p["confirmation"] = json!(token);
    }
    p
}

pub fn restore_params(worktree: &Path, snapshot_id: &str, token: Option<&str>) -> Value {
    let mut p = params(worktree, token);
    p["snapshot_id"] = json!(snapshot_id);
    p
}

/// The code and `data` of an RPC error.
pub fn rejected(result: Result<Value, ClientError>) -> (i64, Option<Value>) {
    match result {
        Err(ClientError::Rpc(e)) => (e.code, e.data),
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

/// An `OPERATION_REJECTED` of a connection with the capability.
pub fn confirm_data(result: Result<Value, ClientError>) -> TmConfirmData {
    let (error, data) = rejected(result);
    assert_eq!(error, code::OPERATION_REJECTED);
    serde_json::from_value(data.expect("data")).expect("TmConfirmData")
}

/// The token of a challenge, as the daemon issued it.
pub fn token_of(data: &TmConfirmData) -> String {
    data.challenge.as_ref().expect("a challenge").token.clone()
}
