//! US-TMC-009, review round: what a restore refuses when its plan cannot be
//! trusted, and what the undo of a restore says when it keeps a branch.
//!
//! A real daemon (in-process) with its own repo layer and snapshot store, a
//! real client over the channel, real Git, testkit fixtures (temporary repo,
//! worktrees and home) and a separate temporary profile; never this repo or
//! the real profile (NFR-01). The work done after the point is real Git run
//! by a test catalog through `operation.prepare` and `operation.run`, as in
//! `us_tmc_009.rs`. No test waits on time: each step waits for the daemon's
//! answer.
//!
//! macOS and Linux, like the other channel tests.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{OperationRunResult, TmRejectReason, TmRejectedData};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::oplog::{
    OPLOG_FILE, OperationKind, OperationState, OperationView, Oplog, Target, repo_dir,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

// ----- Test catalog: real Git commands as the work done after the point -----------

#[derive(Clone, Default)]
struct Script {
    subtype: &'static str,
    commands: Vec<Vec<String>>,
    scope: StepScope,
}

fn script(subtype: &'static str, commands: &[&[&str]]) -> Script {
    Script {
        subtype,
        commands: commands
            .iter()
            .map(|c| c.iter().map(|a| (*a).to_owned()).collect())
            .collect(),
        scope: StepScope::default(),
    }
}

fn commit(message: &str) -> Script {
    script(
        "commit",
        &[&["add", "-A"], &["commit", "-q", "-m", message]],
    )
}

struct Catalog {
    fx: Arc<Fixture>,
    next: Mutex<Script>,
}

struct Step {
    fx: Arc<Fixture>,
    cwd: PathBuf,
    script: Script,
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
        Ok(Box::new(Step {
            fx: Arc::clone(&self.fx),
            cwd: plan.repo.worktree.clone(),
            script: self.next.lock().unwrap().clone(),
        }))
    }
}

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        self.script.subtype
    }

    fn scope(&self) -> StepScope {
        self.script.scope.clone()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        for args in &self.script.commands {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let mut cmd = self.fx.git_command(&self.cwd, &args);
            cmd.stdout(std::process::Stdio::null());
            let child = ctx
                .spawn(&mut cmd)
                .map_err(|e| StepError::new(e.to_string()))?;
            let status = ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
            if !status.success() {
                return Err(StepError::new(format!("git {args:?} failed")));
            }
        }
        Ok(StepOutput::default())
    }
}

struct AllowAll;

impl GuardrailsGate for AllowAll {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

// ----- Running daemon --------------------------------------------------------------

struct Running {
    fx: Arc<Fixture>,
    tp: TempProfile,
    repo_id: String,
    catalog: Arc<Catalog>,
    oplog: Arc<Mutex<Oplog>>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Observes `fx`'s repo in a fresh profile and starts the daemon.
fn start(fx: Fixture) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    run_daemon(Arc::new(fx), tp, entry.repo_id)
}

/// Starts the daemon over a profile that already observes `repo_id`.
fn run_daemon(fx: Arc<Fixture>, tp: TempProfile, repo_id: String) -> Running {
    let catalog = Arc::new(Catalog {
        fx: Arc::clone(&fx),
        next: Mutex::new(Script::default()),
    });
    let wiring = OperationsWiring {
        catalog: Arc::clone(&catalog) as Arc<dyn OperationCatalog>,
        gate: Arc::new(AllowAll),
        test_layer_override: Some(Layer::Cockpit),
        prior_deadline: Duration::from_secs(30),
        prior_layer: None,
    };
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: Some(wiring),
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: TmCapture::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let oplog = daemon.oplog(&repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        fx,
        tp,
        repo_id,
        catalog,
        oplog,
        handle,
        join: Some(join),
    }
}

impl Running {
    fn operation(&self, worktree: &Path, script: Script) -> OperationRunResult {
        *self.catalog.next.lock().unwrap() = script;
        let mut c = connect(&self.tp);
        let plan: PrepareResult = c
            .call(
                methods::OPERATION_PREPARE,
                json!({
                    "operation": "abort-in-progress",
                    "worktree": worktree.to_str().unwrap(),
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

    fn restore(&self, worktree: &Path, snapshot_id: &str) -> Result<Value, ClientError> {
        connect(&self.tp).call(
            methods::TM_RESTORE,
            json!({
                "worktree": worktree.to_str().unwrap(),
                "snapshot_id": snapshot_id,
                "surface": "cli",
            }),
        )
    }

    fn op(&self, id: &str) -> OperationView {
        self.oplog.lock().unwrap().operation(id).unwrap().unwrap()
    }

    /// Stops the daemon and hands back what a restart needs.
    fn stop(mut self) -> (Arc<Fixture>, TempProfile, String) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
        let fx = Arc::clone(&self.fx);
        let repo_id = self.repo_id.clone();
        // `Running` owns the profile; swap in an empty one so it can move out.
        let tp = std::mem::replace(&mut self.tp, TempProfile::new());
        (fx, tp, repo_id)
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

fn reject_reason(result: Result<Value, ClientError>) -> TmRejectedData {
    match result {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, code::OPERATION_REJECTED, "{:?}", e.data);
            serde_json::from_value(e.data.unwrap()).unwrap()
        }
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

/// The restore was recorded rejected, of kind `restore`, with no prior.
fn assert_recorded_rejected(r: &Running, refusal: &TmRejectedData, target: &str) {
    let rec = r.op(refusal.operation_id.as_deref().unwrap());
    assert_eq!(rec.state, OperationState::Rejected);
    assert_eq!(rec.record.kind, OperationKind::Restore);
    assert_eq!(rec.record.target, Target::Snapshot(target.to_owned()));
    assert_eq!(rec.prior_snapshot, None);
}

/// A repo with `a.rs` committed and a linked worktree `feat-login` on its
/// own branch.
fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("a.rs", "fn a() {}\n");
    fx.git(&["add", "a.rs"]);
    fx.git(&["commit", "-q", "-m", "a"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

// ----- Tests -----------------------------------------------------------------------

/// An operation after the point whose oplog row was edited behind the
/// daemon's back hides whose work it is: the restore refuses the point
/// (`target-unavailable`, recorded) instead of leaving that work out of the
/// permission rule, and nothing changes.
#[test]
fn a_tampered_operation_after_the_point_refuses_the_restore() {
    let (fx, wt) = repo_with_login();
    let r = start(fx);
    std::fs::write(wt.join("a.rs"), "fn a() { uno(); }\n").unwrap();
    let first = r.operation(&wt, commit("uno"));
    let point = first.prior_snapshot_id.clone();
    std::fs::write(wt.join("a.rs"), "fn a() { dos(); }\n").unwrap();
    let second = r.operation(&wt, commit("dos"));
    let (fx, tp, repo_id) = r.stop();

    // The user edits the second operation's requester with sqlite3,
    // bypassing the oplog's triggers.
    let file = repo_dir(&tp.dirs(), &repo_id).unwrap().join(OPLOG_FILE);
    let conn = rusqlite::Connection::open(file).unwrap();
    for t in ["snapshots", "operations", "journal", "notices", "chain"] {
        for k in ["update", "delete"] {
            conn.execute_batch(&["DROP TRIGGER ", t, "_no_", k].concat())
                .unwrap();
        }
    }
    let edited = conn
        .execute(
            "UPDATE operations SET subtype = 'edited' WHERE operation_id = ?1",
            [&second.operation_id],
        )
        .unwrap();
    assert_eq!(edited, 1);
    drop(conn);

    let r = run_daemon(fx, tp, repo_id);
    assert!(r.op(&second.operation_id).tampered);
    let before = r.fx.fingerprint();

    let refusal = reject_reason(r.restore(&wt, &point));

    assert_eq!(refusal.reason, TmRejectReason::TargetUnavailable);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    assert_recorded_rejected(&r, &refusal, &point);
}
