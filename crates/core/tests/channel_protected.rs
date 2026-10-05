//! TS-TMC-004 and TS-CKP-002 through the channel: a real daemon (in-process) with the executor
//! of catalog operations, the two-phase flow (`operation.prepare`, then `operation.run` of the
//! plan) and a double of the repo layer, the snapshotter and the operation's step. Real oplogs
//! in a temporary profile; never a real repo or profile (NFR-01). The last tests use real
//! temporary Git repos and run `git` through the executor.
//!
//! The daemon runs in this process, so every client here descends from it and resolves
//! "unattributed" without passing the reserved checks: the wiring fixes layer `cockpit` with
//! `test_layer_override` (never applied to a descendant of the executor). The layer the daemon
//! derives itself is covered by unit tests of `executor` and by
//! `layer_comes_from_the_daemon_without_the_override`.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{
    DescribeResult, Layer, OperationArgs, OperationId, OperationOutcome, PrepareResult,
    RejectReason, RejectedData, WarningCode,
};
use gitraptor_api::messages::{ClientKind, RefusalReason, RefusedData};
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    OperationRunResult, PriorFailedData, PriorFailure, RequestChannel, RequesterView, ResolvedVia,
};
use gitraptor_api::{Actor, PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{ChannelConfig, ProtectedWiring};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause,
};
use gitraptor_core::executor::{
    Affected, Executor, GateDecision, GateRequest, GuardrailsGate, NoGuardrails, OpPlan, PlanClose,
    PlanError, RepoFacts, StepPlan, run_git,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{
    CompleteInfo, NewSnapshot, OperationState, Oplog, SnapshotLevel,
};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedBackend, ProtectedStep,
    RepoHandle, ScopeError, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_git::user_ops::{FALLBACK_NO_EDITOR, RepoTarget, UserGitCommand, UserOp};
use gitraptor_git::{Oid, RefName, SystemGit};
use serde_json::{Value, json};

const REPO_A: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const REPO_B: &str = "0a1b2c3d-0000-4000-8000-0000000000bb";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";

// ----- A client in another process (DEP-MCP-3) ------------------------------

/// Entry point of a client process: with `RAPTOR_TEST_RPC` set to
/// `{"root", "calls": [[method, params]], "out"}`, it connects, makes each
/// call and writes the answers to `out`. As a normal test it does nothing.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let mut answers = Vec::new();
    for call in spec["calls"].as_array().unwrap() {
        let answer = match client.call::<_, Value>(call[0].as_str().unwrap(), call[1].clone()) {
            Ok(v) => json!({ "ok": v }),
            Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
            Err(e) => json!({ "other": e.to_string() }),
        };
        answers.push(answer);
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

// ----- Doubles of the repo layer --------------------------------------------

struct Snap {
    oplog: Arc<Mutex<Oplog>>,
    fail: Option<PriorError>,
}

impl PriorSnapshotter for Snap {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        Ok(PriorSnapshot {
            snapshot_id: complete_snapshot(&self.oplog, Some(&req.operation_id)),
            fast_path: true,
        })
    }
}

fn complete_snapshot(oplog: &Mutex<Oplog>, op: Option<&str>) -> String {
    let mut log = oplog.lock().unwrap();
    let new = NewSnapshot {
        level: SnapshotLevel::GuaranteedPrior,
        worktrees: vec!["/w".into()],
        engine_mark: Some(1),
        cause_operation: op.map(str::to_owned),
        cause_event_seq: None,
    };
    let id = log.begin_snapshot(&new, 1_000).unwrap();
    log.complete_snapshot(&id, &CompleteInfo::default(), 1_000)
        .unwrap();
    id
}

/// What the step double does.
#[derive(Clone)]
enum Behavior {
    /// Reports a ref with terminal escapes.
    MoveEvilRef,
    /// Starts a detached client process (double fork through `sh -c '… &'`)
    /// and waits for its answers.
    SpawnClient { spec: Value, out: PathBuf },
    /// Sleeps, counting how many steps run at once.
    Slow(Duration),
    /// Waits until the plan is cancelled.
    UntilCancelled,
    /// Real Git: `worktree add` of the plan (create-worktree).
    RealGit,
}

struct Step {
    behavior: Behavior,
    runs: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    cancel: gitraptor_core::executor::CancelToken,
    real: Option<(SystemGit, RepoTarget, UserOp)>,
}

const EVIL_REF: &str = "refs/heads/feat/\u{1b}]52;c;cm0gLXJmIH4=\u{7}x";

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        "create-worktree"
    }
    fn scope(&self) -> StepScope {
        StepScope::default()
    }
    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        let out = self.behave(ctx);
        self.active.fetch_sub(1, Ordering::SeqCst);
        out
    }
}

impl Step {
    fn behave(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        match &self.behavior {
            Behavior::MoveEvilRef => Ok(StepOutput {
                changed_refs: vec![EVIL_REF.into()],
                ..StepOutput::default()
            }),
            Behavior::Slow(d) => {
                std::thread::sleep(*d);
                Ok(StepOutput::default())
            }
            Behavior::UntilCancelled => {
                let deadline = Instant::now() + Duration::from_secs(20);
                while self.cancel.reason().is_none() {
                    if Instant::now() > deadline {
                        return Err(StepError::new("never cancelled"));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(StepOutput::default())
            }
            Behavior::SpawnClient { spec, out } => {
                let exe = std::env::current_exe().unwrap();
                let script = format!(
                    "'{}' rpc_client_entry --exact --nocapture --test-threads=1 >/dev/null 2>&1 &",
                    exe.display()
                );
                let mut cmd = Command::new("/bin/sh");
                cmd.args(["-c", &script]).env(RPC_ENV, spec.to_string());
                let child = ctx
                    .spawn(&mut cmd)
                    .map_err(|e| StepError::new(e.to_string()))?;
                ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
                let deadline = Instant::now() + Duration::from_secs(20);
                while !out.exists() {
                    if Instant::now() > deadline {
                        return Err(StepError::new("the client never answered"));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(StepOutput::default())
            }
            Behavior::RealGit => {
                let (git, target, op) = self.real.clone().expect("real git");
                let cmd = UserGitCommand::new(
                    &git,
                    &target,
                    &op,
                    None,
                    &Default::default(),
                    Path::new(FALLBACK_NO_EDITOR),
                    None,
                )
                .map_err(StepError::new)?;
                let run =
                    run_git(ctx, cmd, &self.cancel).map_err(|e| StepError::new(e.to_string()))?;
                let output = String::from_utf8_lossy(&run.stderr).into_owned();
                if run.interrupted.is_some() || !run.status.success() {
                    return Ok(StepOutput {
                        outcome: Some(OperationOutcome::FailedChanged),
                        git_output: Some(output),
                        ..StepOutput::default()
                    });
                }
                Ok(StepOutput {
                    git_output: Some(output),
                    ..StepOutput::default()
                })
            }
        }
    }
}

struct Allow(Vec<String>);

impl McpAllowlist for Allow {
    fn allows(&self, repo_id: &str) -> bool {
        self.0.iter().any(|r| r == repo_id)
    }
}

/// Guardrails double: a fixed decision and the closes it saw.
struct Gate {
    decision: Mutex<GateDecision>,
    closes: Mutex<Vec<PlanClose>>,
}

impl GuardrailsGate for Gate {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        *self.decision.lock().unwrap()
    }
    fn record_close(&self, _req: &GateRequest, close: PlanClose) {
        self.closes.lock().unwrap().push(close);
    }
}

fn double_facts(root: &str) -> RepoFacts {
    RepoFacts {
        root: root.into(),
        git_dir: format!("{root}/.git").into(),
        common_dir: format!("{root}/.git").into(),
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
    repos: Vec<(PathBuf, RepoHandle)>,
    behavior: Mutex<Behavior>,
    runs: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    allow: Allow,
    /// Facts of the double repos; `None`: read the real worktree.
    facts: Mutex<Option<RepoFacts>>,
    warnings: Mutex<Vec<WarningCode>>,
    /// The repo write lock is process-wide: each daemon of a test has its own
    /// keys, as production keys it by the store path inside each profile.
    lock_prefix: String,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        self.repos
            .iter()
            .find(|(root, _)| folder.starts_with(root))
            .map(|(_, h)| h.clone())
            .ok_or(ScopeError::NotObserved)
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("{}/{}", self.lock_prefix, repo.repo_id)
    }
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        match &*self.facts.lock().unwrap() {
            Some(f) => Ok(f.clone()),
            None => RepoFacts::read(&repo.worktree),
        }
    }
    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::CreateWorktree {
            return Err(PlanError::NotImplemented);
        }
        let OperationArgs::CreateWorktree(a) = args else {
            return Err(PlanError::NotImplemented);
        };
        if facts.branches_elsewhere.contains(&a.branch) {
            return Err(PlanError::Rejected(RejectReason::BranchCheckedOutElsewhere));
        }
        Ok(OpPlan {
            expected: json!({ "start": facts.head_commit }),
            warnings: self.warnings.lock().unwrap().clone(),
            affected: Affected::Nobody,
            other_session: false,
        })
    }
    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        let behavior = self.behavior.lock().unwrap().clone();
        let real = match (&behavior, plan.args) {
            (Behavior::RealGit, OperationArgs::CreateWorktree(a)) => {
                let path = a.path.clone().ok_or_else(|| StepError::new("path"))?;
                Some((
                    system_git(),
                    RepoTarget {
                        git_dir: plan.facts.common_dir.clone(),
                        work_tree: Some(plan.facts.root.clone()),
                    },
                    UserOp::WorktreeAdd {
                        branch: RefName::new(&a.branch).unwrap(),
                        path: PathBuf::from(path),
                        start: Oid::from_hex(plan.facts.head_commit.as_deref().unwrap()).unwrap(),
                    },
                ))
            }
            _ => None,
        };
        Ok(Box::new(Step {
            behavior,
            runs: Arc::clone(&self.runs),
            active: Arc::clone(&self.active),
            max_active: Arc::clone(&self.max_active),
            cancel: plan.cancel.clone(),
            real,
        }))
    }
}

// ----- Fixture ---------------------------------------------------------------

/// The system Git, as the daemon would resolve it.
fn system_git() -> SystemGit {
    match gitraptor_git::resolve::resolve(
        &ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        gitraptor_git::resolve::Resolution::Found { git, .. } => git,
        other => panic!("no Git: {other:?}"),
    }
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
    oplog_a: Arc<Mutex<Oplog>>,
    oplog_b: Arc<Mutex<Oplog>>,
    backend: Arc<Backend>,
    gate: Arc<Gate>,
}

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

/// How the daemon is wired for a test.
struct Setup {
    fail: Option<PriorError>,
    behavior: Behavior,
    wired: bool,
    layer: Option<Layer>,
    guardrails: bool,
    /// Root of a real repo for REPO_A; `None`: the double `/repos/a`.
    real_a: Option<PathBuf>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            fail: None,
            behavior: Behavior::MoveEvilRef,
            wired: true,
            layer: Some(Layer::Cockpit),
            guardrails: true,
            real_a: None,
        }
    }
}

fn start(fail: Option<PriorError>, behavior: Behavior, wired: bool) -> Running {
    start_with(Setup {
        fail,
        behavior,
        wired,
        ..Setup::default()
    })
}

fn start_with(setup: Setup) -> Running {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let open = |id| Arc::new(Mutex::new(Oplog::open(&dirs, id, 1_000).unwrap().0));
    let (oplog_a, oplog_b) = (open(REPO_A), open(REPO_B));
    let handle = |id: &str, oplog: &Arc<Mutex<Oplog>>, wt: &Path| RepoHandle {
        repo_id: id.into(),
        worktree: wt.to_owned(),
        oplog: Arc::clone(oplog),
        snapshotter: Arc::new(Snap {
            oplog: Arc::clone(oplog),
            fail: setup.fail.clone(),
        }),
    };
    let root_a = setup
        .real_a
        .clone()
        .unwrap_or_else(|| PathBuf::from("/repos/a"));
    let backend = Arc::new(Backend {
        repos: vec![
            (root_a.clone(), handle(REPO_A, &oplog_a, &root_a)),
            (
                PathBuf::from("/repos/b"),
                handle(REPO_B, &oplog_b, Path::new("/repos/b")),
            ),
        ],
        behavior: Mutex::new(setup.behavior),
        runs: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        max_active: Arc::new(AtomicUsize::new(0)),
        allow: Allow(vec![REPO_A.into()]),
        facts: Mutex::new(setup.real_a.is_none().then(|| double_facts("/repos/a"))),
        warnings: Mutex::new(Vec::new()),
        lock_prefix: tp.root.path().display().to_string(),
    });
    let gate = Arc::new(Gate {
        decision: Mutex::new(GateDecision::Allow),
        closes: Mutex::new(Vec::new()),
    });
    let gate_dyn: Arc<dyn GuardrailsGate> = if setup.guardrails {
        Arc::clone(&gate) as Arc<dyn GuardrailsGate>
    } else {
        Arc::new(NoGuardrails)
    };
    let protected = setup.wired.then(|| {
        let mut w = ProtectedWiring::new(
            Arc::clone(&backend) as Arc<dyn ProtectedBackend>,
            gate_dyn,
            Duration::from_secs(10),
        );
        w.test_layer_override = setup.layer;
        w
    });
    let config = DaemonConfig {
        dirs: dirs.clone(),
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected,
        operations: None,
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        tp,
        handle,
        join: Some(join),
        oplog_a,
        oplog_b,
        backend,
        gate,
    }
}

impl Running {
    fn client(&self, kind: ClientKind) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), kind, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    fn operations(&self) -> usize {
        self.oplog_a
            .lock()
            .unwrap()
            .operations(&Default::default())
            .unwrap()
            .len()
    }

    fn set_facts(&self, edit: impl FnOnce(&mut RepoFacts)) {
        edit(self.backend.facts.lock().unwrap().as_mut().unwrap());
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

fn rpc_err(result: Result<Value, ClientError>) -> (i64, Option<Value>) {
    match result {
        Err(ClientError::Rpc(e)) => (e.code, e.data),
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn rejected(result: Result<Value, ClientError>) -> RejectReason {
    let (code, data) = rpc_err(result);
    assert_eq!(code, code::OPERATION_REJECTED, "{data:?}");
    serde_json::from_value::<RejectedData>(data.unwrap())
        .unwrap()
        .reason
}

fn create_worktree(worktree: &str) -> Value {
    json!({ "operation": "create-worktree", "worktree": worktree, "args": { "branch": "feat/x" } })
}

fn prepare(c: &mut Client, params: Value) -> Result<PrepareResult, ClientError> {
    c.call(methods::OPERATION_PREPARE, params)
}

fn run_plan(c: &mut Client, plan: &PrepareResult) -> Result<Value, ClientError> {
    c.call(
        methods::OPERATION_RUN,
        json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
    )
}

/// Prepare, then run the plan, from the same connection.
fn prepare_and_run(c: &mut Client, params: Value) -> Result<Value, ClientError> {
    let plan = prepare(c, params)?;
    run_plan(c, &plan)
}

// ----- TS-TMC-004 through the two-phase flow ----------------------------------

#[test]
fn operation_run_takes_the_prior_snapshot_then_runs() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let mut params = create_worktree("/repos/a");
    params["surface"] = json!("tui");
    let plan = prepare(&mut c, params).unwrap();
    // Prepare touches nothing and writes no oplog entry.
    assert_eq!(r.operations(), 0);
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
    assert_eq!(plan.layer, Layer::Cockpit);
    assert_eq!(plan.requester.actor, Actor::Unattributed);
    assert_eq!(plan.fingerprint.len(), 64);
    let out: OperationRunResult = serde_json::from_value(run_plan(&mut c, &plan).unwrap()).unwrap();
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 1);
    assert_eq!(out.outcome, OperationOutcome::Done);
    assert_eq!(out.requester.channel, RequestChannel::Tui);
    assert_eq!(out.requester.actor, Actor::Unattributed);
    let log = r.oplog_a.lock().unwrap();
    let view = log.operation(&out.operation_id).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Finished);
    assert_eq!(view.prior_snapshot.as_ref(), Some(&out.prior_snapshot_id));
    assert_eq!(
        view.record.channel,
        gitraptor_core::timemachine::oplog::Channel::Tui
    );
    // SEC-12: the ref travels marked; the CLI prints it clean.
    let shown = out.changed_refs[0].sanitized();
    assert!(
        !shown.contains('\u{1b}') && !shown.contains('\u{7}'),
        "{shown:?}"
    );
    // One record per governed plan, written when it closes (ADR-GRD-006).
    assert_eq!(
        *r.gate.closes.lock().unwrap(),
        [PlanClose::Ran(OperationOutcome::Done)]
    );
}

/// US-TMC-001 scenario 4 through the channel: the client gets the reason,
/// the step never runs (Validación 1).
#[test]
fn without_a_prior_snapshot_nothing_runs_and_the_client_gets_why() {
    let r = start(Some(PriorError::NoSpace), Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let (code, data) = rpc_err(prepare_and_run(&mut c, create_worktree("/repos/a")));
    assert_eq!(code, code::PRIOR_SNAPSHOT_FAILED);
    let data: PriorFailedData = serde_json::from_value(data.unwrap()).unwrap();
    assert_eq!(data.reason, PriorFailure::NoSpace);
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
    let log = r.oplog_a.lock().unwrap();
    let view = log.operation(&data.operation_id.unwrap()).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Aborted);
    drop(log);
    assert_eq!(
        *r.gate.closes.lock().unwrap(),
        [PlanClose::Ran(OperationOutcome::Aborted)]
    );
}

/// BR-TMC-VAL-001 and SEC-02: values out of format and unknown operations are
/// refused before anything is touched, and a client cannot declare who it is
/// nor its layer.
#[test]
fn invalid_parameters_touch_nothing() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let calls = [
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "Checkout; rm -rf", "worktree": "/repos/a" }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "push", "worktree": "/repos/a" }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "args": { "branch": "x" } }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "worktree": "relative", "args": { "branch": "x" } }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "worktree": "/repos/a", "args": { "branch": "x" },
                    "actor": { "actor": "agent", "kind": "claude-code", "origin": "detected" } }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "worktree": "/repos/a", "args": { "branch": "x" }, "layer": "cockpit" }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "worktree": "/repos/a", "args": { "branch": "HEAD" } }),
        ),
        (
            methods::OPERATION_PREPARE,
            json!({ "operation": "create-worktree", "worktree": "/repos/a", "args": { "branch": "x", "force": true } }),
        ),
        (
            methods::OPERATION_RUN,
            json!({ "operation": "create-worktree", "worktree": "/repos/a" }),
        ),
        (
            methods::OPERATION_CANCEL,
            json!({ "operation_id": "latest" }),
        ),
        (
            methods::TM_UNDO,
            json!({ "worktree": "/repos/a", "since": "2 hours" }),
        ),
        (
            methods::TM_UNDO,
            json!({ "worktree": "/repos/a", "agent": "claude 1" }),
        ),
        (
            methods::TM_RESTORE,
            json!({ "worktree": "/repos/a", "snapshot_id": "../../HEAD" }),
        ),
        (
            methods::TM_TIMELINE,
            json!({ "worktree": "/repos/a", "since": "0m" }),
        ),
    ];
    for (method, params) in calls {
        let (code, _) = rpc_err(c.call(method, params.clone()));
        assert_eq!(code, code::INVALID_PARAMS, "{method} {params}");
    }
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
    assert_eq!(r.operations(), 0);
    // The declared identity changed nothing: the daemon's answer stands.
    let view: RequesterView = c.call(methods::REQUESTER_RESOLVE, json!({})).unwrap();
    assert_eq!(view.actor, Actor::Unattributed);
}

/// SEC-TMC-07: a snapshot id of another repo is "not found", like an
/// unknown one; a valid one reaches its story ("not implemented").
#[test]
fn an_id_of_another_repo_does_not_exist() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let in_b = complete_snapshot(&r.oplog_b, None);
    let in_a = complete_snapshot(&r.oplog_a, None);
    let mut c = r.client(ClientKind::Cli);
    let restore = |c: &mut Client, id: &str| {
        rpc_err(c.call(
            methods::TM_RESTORE,
            json!({ "worktree": "/repos/a", "snapshot_id": id }),
        ))
        .0
    };
    assert_eq!(restore(&mut c, &in_b), code::NOT_FOUND);
    assert_eq!(
        restore(&mut c, "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5b"),
        code::NOT_FOUND
    );
    assert_eq!(restore(&mut c, &in_a), code::NOT_IMPLEMENTED);
    // Nothing was recorded by a command its story has not implemented.
    assert_eq!(r.operations(), 0);
}

/// Over MCP the worktree comes from the caller's working folder, never from
/// a parameter; redo, restore, the timeline and Cancel are not offered.
#[test]
fn mcp_scope_comes_from_the_caller() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut m = r.client(ClientKind::Mcp);
    let (code, _) = rpc_err(m.call(methods::OPERATION_PREPARE, create_worktree("/repos/a")));
    assert_eq!(code, code::INVALID_PARAMS);
    let (code, _) = rpc_err(m.call(
        methods::OPERATION_PREPARE,
        json!({ "operation": "create-worktree", "surface": "cli", "args": { "branch": "x" } }),
    ));
    assert_eq!(code, code::INVALID_PARAMS);
    // macOS cannot read another process's working folder safely yet: the
    // scope fails closed (Pendiente: US-GRP-009).
    let (code, _) = rpc_err(m.call(
        methods::OPERATION_PREPARE,
        json!({ "operation": "create-worktree", "args": { "branch": "x" } }),
    ));
    assert_eq!(code, code::SCOPE_REFUSED);
    for method in [
        methods::TM_REDO,
        methods::TM_RESTORE,
        methods::TM_TIMELINE,
        methods::OPERATION_CANCEL,
    ] {
        let (code, _) = rpc_err(m.call(method, json!({})));
        assert_eq!(code, code::METHOD_NOT_FOUND, "{method}");
    }
    // An unattributed requester cannot undo over MCP (TQ-7 → a).
    let (code, _) = rpc_err(m.call(methods::TM_UNDO, json!({})));
    assert_eq!(code, code::SCOPE_REFUSED);
    // Over MCP the requester is only the actor.
    let view: Value = m.call(methods::REQUESTER_RESOLVE, json!({})).unwrap();
    let mut keys: Vec<_> = view.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["actor", "channel"]);
    // The catalog over MCP: only the operations with the MCP mark, and
    // `create-worktree` without a path (H-02).
    let catalog: DescribeResult = m.call(methods::OPERATION_DESCRIBE, json!({})).unwrap();
    let ids: Vec<_> = catalog.operations.iter().map(|o| o.id).collect();
    assert_eq!(
        ids,
        [
            OperationId::RebaseOntoBase,
            OperationId::CreateWorktree,
            OperationId::Commit,
            OperationId::Snapshot
        ]
    );
    let schema = catalog.operations[1].args_schema.to_string();
    assert!(!schema.contains("\"path\""), "{schema}");
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
}

#[test]
fn without_an_executor_the_catalog_is_described_but_nothing_runs() {
    let r = start(None, Behavior::MoveEvilRef, false);
    let mut c = r.client(ClientKind::Cli);
    let catalog: DescribeResult = c.call(methods::OPERATION_DESCRIBE, json!({})).unwrap();
    assert_eq!(catalog.operations.len(), 8);
    assert_eq!(catalog.catalog_version, 1);
    let (code, data) = rpc_err(c.call(methods::OPERATION_PREPARE, create_worktree("/repos/a")));
    assert_eq!(code, code::NOT_IMPLEMENTED);
    assert_eq!(data.unwrap()["implemented_by"], "F-001-02");
    let (code, _) = rpc_err(c.call(
        methods::OPERATION_RUN,
        json!({ "plan_id": "00000000000000000000000000000000" }),
    ));
    assert_eq!(code, code::NOT_IMPLEMENTED);
    let (code, _) = rpc_err(c.call(methods::TM_UNDO, json!({ "worktree": "/repos/a" })));
    assert_eq!(code, code::NOT_IMPLEMENTED);
}

/// DEP-MCP-3 and H-01 (Validación 17): a process the operation started,
/// detached from the daemon's tree by a double fork, acts for the plan's
/// requester and is refused a reserved command (`daemon-descendant`) and,
/// by the executor and without waiting for the lock its plan holds, the
/// catalog operations and Cancel (`executor-descendant`).
#[test]
fn a_child_of_the_operation_cannot_use_a_reserved_command() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let out = r.tp.root.path().join("child-answers.json");
    let spec = json!({
        "root": r.tp.root.path().join("profile"),
        "calls": [
            [methods::REQUESTER_RESOLVE, {}],
            [methods::DAEMON_STOP, {}],
            [methods::OPERATION_PREPARE, create_worktree("/repos/a")],
            [methods::OPERATION_RUN, { "plan_id": "00000000000000000000000000000000" }],
            [methods::OPERATION_CANCEL, { "operation_id": "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5b" }],
        ],
        "out": out,
    });
    *r.backend.behavior.lock().unwrap() = Behavior::SpawnClient {
        spec,
        out: out.clone(),
    };
    let mut c = r.client(ClientKind::Cli);
    let result: OperationRunResult =
        serde_json::from_value(prepare_and_run(&mut c, create_worktree("/repos/a")).unwrap())
            .unwrap();
    let answers: Vec<Value> = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let view: RequesterView = serde_json::from_value(answers[0]["ok"].clone()).unwrap();
    assert_eq!(view.via, ResolvedVia::Executor);
    assert_eq!(view.actor, result.requester.actor);
    assert!(!view.confirmable);
    assert_eq!(answers[1]["code"], code::RESERVED_REFUSED);
    let refused: RefusedData = serde_json::from_value(answers[1]["data"].clone()).unwrap();
    assert_eq!(refused.reason, RefusalReason::DaemonDescendant);
    for answer in &answers[2..] {
        assert_eq!(answer["code"], code::OPERATION_REJECTED, "{answer}");
        assert_eq!(answer["data"]["reason"], "executor-descendant", "{answer}");
    }
    // The daemon is still running: the stop was refused.
    let mut again = r.client(ClientKind::Cli);
    let _: String = again.call(methods::PING, json!({})).unwrap();
}

// ----- TS-CKP-002: the plan ----------------------------------------------------

/// M-04 (Validación 22): a plan runs only from its own connection and only
/// once; an unknown one is the same answer.
#[test]
fn a_plan_belongs_to_its_connection_and_runs_once() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut a = r.client(ClientKind::Cli);
    let mut b = r.client(ClientKind::Cli);
    let plan = prepare(&mut a, create_worktree("/repos/a")).unwrap();
    assert_eq!(rejected(run_plan(&mut b, &plan)), RejectReason::PlanUnknown);
    run_plan(&mut a, &plan).unwrap();
    assert_eq!(rejected(run_plan(&mut a, &plan)), RejectReason::PlanUnknown);
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 1);
    // Closing a connection drops its plans, each with its record.
    let _left = prepare(&mut b, create_worktree("/repos/a")).unwrap();
    drop(b);
    let deadline = Instant::now() + Duration::from_secs(5);
    while r.gate.closes.lock().unwrap().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        r.gate.closes.lock().unwrap().last(),
        Some(&PlanClose::Dropped)
    );
}

/// BR-CKP-CONS-004 (Validación 3): the state changed between prepare and
/// run: rejected, nothing ran and no oplog entry.
#[test]
fn a_changed_plan_is_rejected_without_effects() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    // A commit lands on the worktree in between.
    r.set_facts(|f| f.head_commit = Some("2".repeat(40)));
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::StateChanged
    );
    // A precondition that now fails also reads as "the state changed".
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    r.set_facts(|f| f.git_locks = vec!["index.lock".into()]);
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::StateChanged
    );
    assert_eq!(
        rejected(prepare_and_run(&mut c, create_worktree("/repos/a"))),
        RejectReason::GitBusy
    );
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
    assert_eq!(r.operations(), 0);
    assert!(
        r.gate
            .closes
            .lock()
            .unwrap()
            .iter()
            .all(|c| *c == PlanClose::Rejected)
    );
}

/// M-05 (Validación 23): the `.git` of the worktree replaced between
/// prepare and run.
#[test]
fn a_replaced_repo_is_rejected_under_the_lock() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    r.set_facts(|f| f.dot_git_id = Some((1, 99)));
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::RepoIdentityChanged
    );
    assert_eq!(r.operations(), 0);
}

/// ADR-CKP-002 § 2, step 3: the run enumerates exactly the plan's warnings.
#[test]
fn the_warnings_must_be_accepted() {
    let r = start(None, Behavior::MoveEvilRef, true);
    *r.backend.warnings.lock().unwrap() = vec![WarningCode::PredictedConflict];
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    assert_eq!(plan.warnings, [WarningCode::PredictedConflict]);
    let bare = c.call::<_, Value>(methods::OPERATION_RUN, json!({ "plan_id": plan.plan_id }));
    assert_eq!(rejected(bare), RejectReason::WarningsMismatch);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    run_plan(&mut c, &plan).unwrap();
}

/// Q-CKP-19 (Validación 3, serialization): two clients on one repo never
/// run at once; the second waits in the queue, seen by every client.
#[test]
fn operations_on_one_repo_are_serialized() {
    let r = start(None, Behavior::Slow(Duration::from_millis(400)), true);
    let mut watcher = r.client(ClientKind::Cli);
    let _: Value = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    let mut a = r.client(ClientKind::Cli);
    let mut b = r.client(ClientKind::Cli);
    let plan_a = prepare(&mut a, create_worktree("/repos/a")).unwrap();
    let plan_b = prepare(&mut b, create_worktree("/repos/a")).unwrap();
    let ta = std::thread::spawn(move || run_plan(&mut a, &plan_a).unwrap());
    std::thread::sleep(Duration::from_millis(100));
    let tb = std::thread::spawn(move || run_plan(&mut b, &plan_b).unwrap());
    ta.join().unwrap();
    tb.join().unwrap();
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 2);
    assert_eq!(r.backend.max_active.load(Ordering::SeqCst), 1);
    let mut kinds = Vec::new();
    while let Some(n) = watcher
        .next_notification(Duration::from_millis(300))
        .unwrap()
    {
        let kind = n.params["event"]["kind"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if kind.starts_with("operation.") {
            kinds.push(kind);
        }
    }
    assert!(kinds.contains(&"operation.queued".to_owned()), "{kinds:?}");
    assert_eq!(
        kinds.iter().filter(|k| *k == "operation.finished").count(),
        2,
        "{kinds:?}"
    );
}

/// ADR-CKP-002 § 4: the decision that counts denies before any effect and
/// leaves one record; without a decision engine nothing governed runs.
#[test]
fn guardrails_decide_before_any_effect() {
    let r = start(None, Behavior::MoveEvilRef, true);
    *r.gate.decision.lock().unwrap() = GateDecision::Deny;
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    assert_eq!(plan.decision, gitraptor_api::catalog::DecisionView::Deny);
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::GuardrailsDenied
    );
    assert_eq!(r.operations(), 0);
    assert_eq!(*r.gate.closes.lock().unwrap(), [PlanClose::Denied]);

    let r = start_with(Setup {
        guardrails: false,
        ..Setup::default()
    });
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(&mut c, create_worktree("/repos/a")).unwrap();
    assert_eq!(
        plan.decision,
        gitraptor_api::catalog::DecisionView::NotEvaluated
    );
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::GuardrailsDenied
    );
    assert_eq!(r.operations(), 0);
}

/// An operation whose story is not built yet says which story builds it,
/// at prepare, before any lock or oplog entry.
#[test]
fn an_operation_without_its_story_names_it() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let (code, data) = rpc_err(c.call(
        methods::OPERATION_PREPARE,
        json!({ "operation": "discard-worktree", "worktree": "/repos/a" }),
    ));
    assert_eq!(code, code::NOT_IMPLEMENTED);
    assert_eq!(data.unwrap()["implemented_by"], "US-CKP-017");
    // A cockpit-only plan is refused over the operations of MCP (M-03).
    let (code, _) = rpc_err(c.call(
        methods::OPERATION_PREPARE,
        json!({ "operation": "commit", "worktree": "/repos/a", "args": { "message": "m" } }),
    ));
    assert_eq!(code, code::OPERATION_REJECTED);
    assert_eq!(r.operations(), 0);
}

/// M-03: without the test override the daemon fixes the layer; an in-process
/// client does not pass the reserved checks, so it gets `mcp` and, being
/// unattributed, nothing.
#[test]
fn layer_comes_from_the_daemon_without_the_override() {
    let r = start_with(Setup {
        layer: None,
        ..Setup::default()
    });
    let mut c = r.client(ClientKind::Cli);
    assert_eq!(
        rejected(prepare_and_run(&mut c, create_worktree("/repos/a"))),
        RejectReason::UnattributedWithoutCockpit
    );
    let (code, _) = rpc_err(c.call(
        methods::OPERATION_CANCEL,
        json!({ "operation_id": "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5b" }),
    ));
    assert_eq!(code, code::OPERATION_REJECTED);
    // The production constructor never fixes a layer.
    let w = ProtectedWiring::new(
        Arc::clone(&r.backend) as Arc<dyn ProtectedBackend>,
        Arc::new(NoGuardrails),
        Duration::from_secs(1),
    );
    assert_eq!(w.test_layer_override, None);
    let _ = Executor::new(Arc::new(NoGuardrails));
}

/// ADR-CKP-002 § 2, D8: a connection keeps at most four live plans.
#[test]
fn live_plans_per_connection_are_bounded() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    for _ in 0..4 {
        prepare(&mut c, create_worktree("/repos/a")).unwrap();
    }
    let (code, _) = rpc_err(c.call(methods::OPERATION_PREPARE, create_worktree("/repos/a")));
    assert_eq!(code, code::LIMIT_REACHED);
}

/// BR-CKP-WF-008: Cancel from another client with layer `cockpit`, by the
/// operation id the stream published.
#[test]
fn cancel_reaches_the_running_operation() {
    let r = start(None, Behavior::UntilCancelled, true);
    let mut watcher = r.client(ClientKind::Cli);
    let _: Value = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    let mut a = r.client(ClientKind::Cli);
    let plan = prepare(&mut a, create_worktree("/repos/a")).unwrap();
    let ta = std::thread::spawn(move || run_plan(&mut a, &plan));
    let deadline = Instant::now() + Duration::from_secs(10);
    let operation_id = loop {
        assert!(Instant::now() < deadline, "no operation.started");
        if let Some(n) = watcher
            .next_notification(Duration::from_millis(200))
            .unwrap()
            && n.params["event"]["kind"] == "operation.started"
        {
            break n.params["event"]["data"]["operation_id"]
                .as_str()
                .unwrap()
                .to_owned();
        }
    };
    let mut b = r.client(ClientKind::Cli);
    let answer: Value = b
        .call(
            methods::OPERATION_CANCEL,
            json!({ "operation_id": operation_id }),
        )
        .unwrap();
    assert_eq!(answer["requested"], true);
    let out: OperationRunResult = serde_json::from_value(ta.join().unwrap().unwrap()).unwrap();
    assert_eq!(out.outcome, OperationOutcome::Cancelled);
    // Nothing runs under that id any more.
    let answer: Value = b
        .call(
            methods::OPERATION_CANCEL,
            json!({ "operation_id": operation_id }),
        )
        .unwrap();
    assert_eq!(answer["requested"], false);
}

// ----- TS-CKP-002 with real Git ------------------------------------------------

fn real_repo(tp: &tempfile::TempDir) -> PathBuf {
    let parent = std::fs::canonicalize(tp.path()).unwrap();
    common::init_repo(&parent, "repo", true)
}

/// The whole flow on a real repo: the executor launches `git worktree add`
/// as a marked child, with the explicit repo, and the requester sees Git's
/// output (layer `cockpit`).
#[test]
fn a_real_worktree_is_created_through_the_executor() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = real_repo(&tmp);
    let r = start_with(Setup {
        behavior: Behavior::RealGit,
        real_a: Some(repo.clone()),
        ..Setup::default()
    });
    let new = repo.parent().unwrap().join("repo-feat-x");
    let mut c = r.client(ClientKind::Cli);
    let params = json!({ "operation": "create-worktree", "worktree": repo,
                         "args": { "branch": "feat/x", "path": new } });
    let out: OperationRunResult =
        serde_json::from_value(prepare_and_run(&mut c, params).unwrap()).unwrap();
    assert_eq!(out.outcome, OperationOutcome::Done);
    assert!(new.join(".git").is_file());
    assert_eq!(
        common::git(&new, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feat/x"
    );
    assert!(out.git_output.is_some());
}

/// Validación 3 on a real repo: a commit by someone else between prepare
/// and run; the plan is rejected and no worktree appears.
#[test]
fn an_external_commit_between_prepare_and_run_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = real_repo(&tmp);
    let r = start_with(Setup {
        behavior: Behavior::RealGit,
        real_a: Some(repo.clone()),
        ..Setup::default()
    });
    let new = repo.parent().unwrap().join("repo-feat-x");
    let mut c = r.client(ClientKind::Cli);
    let plan = prepare(
        &mut c,
        json!({ "operation": "create-worktree", "worktree": repo,
                "args": { "branch": "feat/x", "path": new } }),
    )
    .unwrap();
    std::fs::write(repo.join("other.txt"), "x\n").unwrap();
    common::git(&repo, &["add", "other.txt"]);
    common::git(&repo, &["commit", "-q", "-m", "someone else"]);
    assert_eq!(
        rejected(run_plan(&mut c, &plan)),
        RejectReason::StateChanged
    );
    assert!(!new.exists());
    assert_eq!(r.operations(), 0);
}

/// ADR-CKP-002 § 6 on a real repo: Cancel interrupts a hook that never ends,
/// like a Ctrl-C to the group of `git`.
#[test]
fn cancel_interrupts_a_hanging_hook() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = real_repo(&tmp);
    let hook = repo.join(".git").join("hooks").join("post-checkout");
    std::fs::write(&hook, "#!/bin/sh\nsleep 60\n").unwrap();
    let _ = Command::new("chmod").arg("0755").arg(&hook).status();
    let r = start_with(Setup {
        behavior: Behavior::RealGit,
        real_a: Some(repo.clone()),
        ..Setup::default()
    });
    let mut watcher = r.client(ClientKind::Cli);
    let _: Value = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    let new = repo.parent().unwrap().join("repo-feat-x");
    let mut a = r.client(ClientKind::Cli);
    let plan = prepare(
        &mut a,
        json!({ "operation": "create-worktree", "worktree": repo,
                "args": { "branch": "feat/x", "path": new } }),
    )
    .unwrap();
    let started = Instant::now();
    let ta = std::thread::spawn(move || run_plan(&mut a, &plan));
    let operation_id = loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no operation.started"
        );
        if let Some(n) = watcher
            .next_notification(Duration::from_millis(200))
            .unwrap()
            && n.params["event"]["kind"] == "operation.started"
        {
            break n.params["event"]["data"]["operation_id"]
                .as_str()
                .unwrap()
                .to_owned();
        }
    };
    std::thread::sleep(Duration::from_millis(300));
    let mut b = r.client(ClientKind::Cli);
    let _: Value = b
        .call(
            methods::OPERATION_CANCEL,
            json!({ "operation_id": operation_id }),
        )
        .unwrap();
    let out: OperationRunResult = serde_json::from_value(ta.join().unwrap().unwrap()).unwrap();
    assert_eq!(out.outcome, OperationOutcome::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the hook was not interrupted"
    );
}
