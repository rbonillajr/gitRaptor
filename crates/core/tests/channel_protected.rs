//! TS-TMC-004 through the channel: a real daemon (in-process) with a
//! double of the repo layer, the snapshotter and the step (the executor of
//! F-001-02/05 and the applier of TS-TMC-003 are built elsewhere). Real
//! oplogs in a temporary profile; never a real repo or profile (NFR-01).
//!
//! The daemon runs in this process, so every client here descends from it
//! and resolves "unattributed". Attribution to a simulated agent by CLI and
//! MCP is tested at process level in `apps/cli/tests/protected_process.rs`.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
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
}

struct Step {
    behavior: Behavior,
    runs: Arc<AtomicUsize>,
}

const EVIL_REF: &str = "refs/heads/feat/\u{1b}]52;c;cm0gLXJmIH4=\u{7}x";

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        "checkout"
    }
    fn scope(&self) -> StepScope {
        StepScope::default()
    }
    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        match &self.behavior {
            Behavior::MoveEvilRef => Ok(StepOutput {
                changed_refs: vec![EVIL_REF.into()],
            }),
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
        }
    }
}

struct Allow(Vec<String>);

impl McpAllowlist for Allow {
    fn allows(&self, repo_id: &str) -> bool {
        self.0.iter().any(|r| r == repo_id)
    }
}

struct Backend {
    repos: Vec<(PathBuf, RepoHandle)>,
    behavior: Mutex<Behavior>,
    runs: Arc<AtomicUsize>,
    allow: Allow,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        self.repos
            .iter()
            .find(|(root, _)| folder.starts_with(root))
            .map(|(_, h)| h.clone())
            .ok_or(ScopeError::NotObserved)
    }
    fn step(
        &self,
        operation: &str,
        _args: &serde_json::Map<String, Value>,
        _repo: &RepoHandle,
    ) -> Result<Box<dyn ProtectedStep>, StepError> {
        if operation != "checkout" {
            return Err(StepError::new("unknown operation"));
        }
        Ok(Box::new(Step {
            behavior: self.behavior.lock().unwrap().clone(),
            runs: Arc::clone(&self.runs),
        }))
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
}

// ----- Fixture ---------------------------------------------------------------

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
    oplog_a: Arc<Mutex<Oplog>>,
    oplog_b: Arc<Mutex<Oplog>>,
    backend: Arc<Backend>,
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

fn start(fail: Option<PriorError>, behavior: Behavior, wired: bool) -> Running {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let open = |id| Arc::new(Mutex::new(Oplog::open(&dirs, id, 1_000).unwrap().0));
    let (oplog_a, oplog_b) = (open(REPO_A), open(REPO_B));
    let handle = |id: &str, oplog: &Arc<Mutex<Oplog>>, wt: &str| RepoHandle {
        repo_id: id.into(),
        worktree: PathBuf::from(wt),
        oplog: Arc::clone(oplog),
        snapshotter: Arc::new(Snap {
            oplog: Arc::clone(oplog),
            fail: fail.clone(),
        }),
    };
    let backend = Arc::new(Backend {
        repos: vec![
            (
                PathBuf::from("/repos/a"),
                handle(REPO_A, &oplog_a, "/repos/a"),
            ),
            (
                PathBuf::from("/repos/b"),
                handle(REPO_B, &oplog_b, "/repos/b"),
            ),
        ],
        behavior: Mutex::new(behavior),
        runs: Arc::new(AtomicUsize::new(0)),
        allow: Allow(vec![REPO_A.into()]),
    });
    let config = DaemonConfig {
        dirs: dirs.clone(),
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: wired.then(|| ProtectedWiring {
            backend: Arc::clone(&backend) as Arc<dyn ProtectedBackend>,
            prior_deadline: Duration::from_secs(10),
        }),
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

// ----- Tests -----------------------------------------------------------------

#[test]
fn operation_run_takes_the_prior_snapshot_then_runs() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let out: OperationRunResult = c
        .call(
            methods::OPERATION_RUN,
            json!({ "operation": "checkout", "worktree": "/repos/a", "surface": "tui" }),
        )
        .unwrap();
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 1);
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
}

/// US-TMC-001 scenario 4 through the channel: the client gets the reason,
/// the step never runs.
#[test]
fn without_a_prior_snapshot_nothing_runs_and_the_client_gets_why() {
    let r = start(Some(PriorError::NoSpace), Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let (code, data) = rpc_err(c.call(
        methods::OPERATION_RUN,
        json!({ "operation": "checkout", "worktree": "/repos/a" }),
    ));
    assert_eq!(code, code::PRIOR_SNAPSHOT_FAILED);
    let data: PriorFailedData = serde_json::from_value(data.unwrap()).unwrap();
    assert_eq!(data.reason, PriorFailure::NoSpace);
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
    let log = r.oplog_a.lock().unwrap();
    let view = log.operation(&data.operation_id.unwrap()).unwrap().unwrap();
    assert_eq!(view.state, OperationState::Aborted);
}

/// BR-TMC-VAL-001: values out of format are refused before anything is
/// touched, and a client cannot declare who it is.
#[test]
fn invalid_parameters_touch_nothing() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut c = r.client(ClientKind::Cli);
    let calls = [
        (
            methods::OPERATION_RUN,
            json!({ "operation": "Checkout; rm -rf", "worktree": "/repos/a" }),
        ),
        (methods::OPERATION_RUN, json!({ "operation": "checkout" })),
        (
            methods::OPERATION_RUN,
            json!({ "operation": "checkout", "worktree": "relative" }),
        ),
        (
            methods::OPERATION_RUN,
            json!({ "operation": "checkout", "worktree": "/repos/a",
                    "actor": { "actor": "agent", "kind": "claude-code", "origin": "detected" } }),
        ),
        (
            methods::OPERATION_RUN,
            json!({ "operation": "checkout", "worktree": "/repos/a", "human": true }),
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
/// a parameter; redo, restore and the timeline are not offered (Q-MCP-11).
#[test]
fn mcp_scope_comes_from_the_caller() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let mut m = r.client(ClientKind::Mcp);
    let (code, _) = rpc_err(m.call(
        methods::OPERATION_RUN,
        json!({ "operation": "checkout", "worktree": "/repos/a" }),
    ));
    assert_eq!(code, code::INVALID_PARAMS);
    let (code, _) = rpc_err(m.call(
        methods::OPERATION_RUN,
        json!({ "operation": "checkout", "surface": "cli" }),
    ));
    assert_eq!(code, code::INVALID_PARAMS);
    // macOS cannot read another process's working folder safely yet: the
    // scope fails closed (Pendiente: US-GRP-009).
    let (code, _) = rpc_err(m.call(methods::OPERATION_RUN, json!({ "operation": "checkout" })));
    assert_eq!(code, code::SCOPE_REFUSED);
    for method in [methods::TM_REDO, methods::TM_RESTORE, methods::TM_TIMELINE] {
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
    assert_eq!(r.backend.runs.load(Ordering::SeqCst), 0);
}

#[test]
fn without_an_executor_operation_run_is_not_implemented() {
    let r = start(None, Behavior::MoveEvilRef, false);
    let mut c = r.client(ClientKind::Cli);
    let (code, data) = rpc_err(c.call(
        methods::OPERATION_RUN,
        json!({ "operation": "checkout", "worktree": "/repos/a" }),
    ));
    assert_eq!(code, code::NOT_IMPLEMENTED);
    assert_eq!(data.unwrap()["implemented_by"], "F-001-02");
    let (code, _) = rpc_err(c.call(methods::TM_UNDO, json!({ "worktree": "/repos/a" })));
    assert_eq!(code, code::NOT_IMPLEMENTED);
}

/// DEP-MCP-3: a process the operation started, detached from the daemon's
/// tree by a double fork, still acts for the operation's requester and is
/// refused a reserved command as a descendant of the executor.
#[test]
fn a_child_of_the_operation_cannot_use_a_reserved_command() {
    let r = start(None, Behavior::MoveEvilRef, true);
    let out = r.tp.root.path().join("child-answers.json");
    let spec = json!({
        "root": r.tp.root.path().join("profile"),
        "calls": [
            [methods::REQUESTER_RESOLVE, {}],
            [methods::DAEMON_STOP, {}],
        ],
        "out": out,
    });
    *r.backend.behavior.lock().unwrap() = Behavior::SpawnClient {
        spec,
        out: out.clone(),
    };
    let mut c = r.client(ClientKind::Cli);
    let result: OperationRunResult = c
        .call(
            methods::OPERATION_RUN,
            json!({ "operation": "checkout", "worktree": "/repos/a" }),
        )
        .unwrap();
    let answers: Vec<Value> = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let view: RequesterView = serde_json::from_value(answers[0]["ok"].clone()).unwrap();
    assert_eq!(view.via, ResolvedVia::Executor);
    assert_eq!(view.actor, result.requester.actor);
    assert!(!view.confirmable);
    assert_eq!(answers[1]["code"], code::RESERVED_REFUSED);
    let refused: RefusedData = serde_json::from_value(answers[1]["data"].clone()).unwrap();
    assert_eq!(refused.reason, RefusalReason::DaemonDescendant);
    // The daemon is still running: the stop was refused.
    let mut again = r.client(ClientKind::Cli);
    let _: String = again.call(methods::PING, json!({})).unwrap();
}
