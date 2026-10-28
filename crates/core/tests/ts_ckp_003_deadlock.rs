//! TS-CKP-003 (D5, D15, ADR-GRD-003 Enmienda Cockpit): the hook of the executor's own `git` is
//! answered by `guard.evaluate` while the repo is locked by the operation, and a child of the
//! executor never asks for a `hook-prior` snapshot.
//!
//! In-process daemon with the executor wired to the real `GuardrailsEngine` (attached by the
//! test to a registry of its own) and a step double that launches the hook client as a marked
//! child: this very test binary run again (`hook_client_entry`), which calls `guard.evaluate`
//! the way a hook would. Temporary profile and repo only (NFR-01).
//!
//! The daemon runs in this process, so every client descends from it: the wiring fixes layer
//! `cockpit` with `test_layer_override` (see `channel_protected.rs`).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::{TempProfile, common_dir, git, init_repo};
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, OperationOutcome, PrepareResult};
use gitraptor_api::guard::{
    Decision, Effect, EvaluateParams, Hook, Operation, RefUpdate, RefValue,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, Untrusted, methods};
use gitraptor_core::channel::{ChannelConfig, ProtectedWiring};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{Affected, GuardrailsGate, OpPlan, PlanError, RepoFacts, StepPlan};
use gitraptor_core::guardrails::plan_gate::GuardrailsEngine;
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{
    CompleteInfo, NewSnapshot, Oplog, SnapshotFilter, SnapshotLevel,
};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedBackend, ProtectedStep,
    RepoHandle, ScopeError, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_policy::guard::plan::PlanShape;
use serde_json::{Value, json};

const REPO_A: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const RPC_ENV: &str = "RAPTOR_TEST_HOOK_CLIENT";
const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// The hook client: with `RAPTOR_TEST_HOOK_CLIENT` set to `{"root", "params", "out"}`, it
/// connects (a library client: it accepts every capability it knows, `guard.prior-snapshot`
/// included), asks `guard.evaluate` and writes the answer to `out`. As a normal test, nothing.
#[test]
fn hook_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Other, PROTOCOL_VERSION).unwrap();
    let answer = match client.call::<_, Value>(methods::GUARD_EVALUATE, spec["params"].clone()) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answer).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

struct Snap {
    oplog: Arc<Mutex<Oplog>>,
}

impl PriorSnapshotter for Snap {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        let mut log = self.oplog.lock().unwrap();
        let new = NewSnapshot {
            level: SnapshotLevel::GuaranteedPrior,
            worktrees: vec!["/w".into()],
            engine_mark: Some(1),
            cause_operation: Some(req.operation_id.clone()),
            cause_event_seq: None,
        };
        let id = log.begin_snapshot(&new, 1_000).unwrap();
        log.complete_snapshot(&id, &CompleteInfo::default(), 1_000)
            .unwrap();
        Ok(PriorSnapshot {
            snapshot_id: id,
            fast_path: true,
        })
    }
}

struct Allow;

impl McpAllowlist for Allow {
    fn allows(&self, _repo_id: &str) -> bool {
        true
    }
}

/// What the repo of the test looks like: `main` and `feat/x`, one commit ahead.
struct Oids {
    main: String,
    feat: String,
}

/// The step double: launches the hook client as a marked child, waits for it and for its answer.
struct Step {
    spec: Value,
    out: PathBuf,
    /// The repo's write lock, which this step is running under.
    lock_key: String,
    subtype: &'static str,
}

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        self.subtype
    }
    fn scope(&self) -> StepScope {
        StepScope::default()
    }
    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        // The executor holds the repo while this step runs.
        if gitraptor_core::repo_lock::try_lock(&self.lock_key).is_some() {
            return Err(StepError::new("the repo is not locked under the step"));
        }
        let exe = std::env::current_exe().unwrap();
        let mut cmd = Command::new(exe);
        cmd.args([
            "hook_client_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(RPC_ENV, self.spec.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
        let child = ctx
            .spawn(&mut cmd)
            .map_err(|e| StepError::new(e.to_string()))?;
        ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
        if !self.out.exists() {
            return Err(StepError::new("the hook client never answered"));
        }
        Ok(StepOutput::default())
    }
}

struct Backend {
    repo: RepoHandle,
    root: PathBuf,
    oids: Oids,
    out_dir: PathBuf,
    lock_prefix: String,
    allow: Allow,
}

impl Backend {
    fn out(&self, operation: OperationId) -> PathBuf {
        self.out_dir.join(format!("{}.json", operation.as_str()))
    }

    /// What the hook of this operation's own `git` would ask.
    fn hook_params(&self, operation: OperationId) -> EvaluateParams {
        let common = self.repo.common_dir.to_string_lossy().into_owned();
        let (hook, operation) = match operation {
            OperationId::RebaseOntoBase => (
                Hook::PreRebase,
                Operation::Rebase {
                    upstream: Some(Untrusted::new("main")),
                    branch: Some(Untrusted::new("feat/x")),
                },
            ),
            OperationId::DiscardWorktree => (
                Hook::ReferenceTransaction,
                Operation::RefTransaction {
                    updates: vec![RefUpdate {
                        refname: "refs/heads/feat/x".into(),
                        old: RefValue::Oid(self.oids.feat.clone()),
                        new: RefValue::Zero,
                    }],
                    orphan_head: None,
                },
            ),
            _ => (
                Hook::ReferenceTransaction,
                Operation::RefTransaction {
                    updates: vec![RefUpdate {
                        refname: "refs/heads/feat/new".into(),
                        old: RefValue::Zero,
                        new: RefValue::Oid(self.oids.main.clone()),
                    }],
                    orphan_head: None,
                },
            ),
        };
        EvaluateParams {
            exception_token: None,
            repo_id: REPO_A.into(),
            common_dir: common,
            hook,
            operation,
            authorship: None,
        }
    }
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        if folder.starts_with(&self.repo.worktree) {
            Ok(self.repo.clone())
        } else {
            Err(ScopeError::NotObserved)
        }
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("{}/{}", self.lock_prefix, repo.repo_id)
    }
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, gitraptor_api::catalog::RejectReason> {
        RepoFacts::read(&repo.worktree)
    }
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        let shape = match operation {
            OperationId::CreateWorktree => PlanShape::CreateWorktree {
                branch: "refs/heads/feat/new".into(),
                at: self.oids.main.clone(),
            },
            OperationId::RebaseOntoBase => PlanShape::Rebase {
                branch: "refs/heads/feat/x".into(),
                upstream: "refs/heads/main".into(),
                old_tip: self.oids.feat.clone(),
                onto: self.oids.main.clone(),
            },
            OperationId::DiscardWorktree => PlanShape::DeleteWorktree {
                branch: "refs/heads/feat/x".into(),
                tip: self.oids.feat.clone(),
            },
            _ => return Err(PlanError::NotImplemented),
        };
        Ok(OpPlan {
            expected: json!({ "start": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: Some(shape),
        })
    }
    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        let out = self.out(plan.operation);
        let spec = json!({
            "root": self.root,
            "params": self.hook_params(plan.operation),
            "out": out,
        });
        Ok(Box::new(Step {
            spec,
            out,
            lock_key: self.write_lock_key(&self.repo),
            subtype: plan.operation.as_str(),
        }))
    }
}

struct Running {
    tp: TempProfile,
    _repo_dir: tempfile::TempDir,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    oplog: Arc<Mutex<Oplog>>,
    backend: Arc<Backend>,
    worktree: PathBuf,
}

fn start() -> Running {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let repo_dir = tempfile::tempdir().unwrap();
    let worktree = init_repo(repo_dir.path(), "demo", true)
        .canonicalize()
        .unwrap();
    let main = git(&worktree, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&worktree, &["switch", "-q", "-c", "feat/x"]);
    git(&worktree, &["commit", "-q", "--allow-empty", "-m", "feat"]);
    let feat = git(&worktree, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&worktree, &["switch", "-q", "main"]);
    let oplog = Arc::new(Mutex::new(Oplog::open(&dirs, REPO_A, 1_000).unwrap().0));
    let backend = Arc::new(Backend {
        repo: RepoHandle {
            repo_id: REPO_A.into(),
            worktree: worktree.clone(),
            common_dir: common_dir(&worktree),
            oplog: Arc::clone(&oplog),
            snapshotter: Arc::new(Snap {
                oplog: Arc::clone(&oplog),
            }),
        },
        root: tp.root.path().join("profile"),
        oids: Oids { main, feat },
        out_dir: tp.root.path().join("answers"),
        lock_prefix: tp.root.path().display().to_string(),
        allow: Allow,
    });
    std::fs::create_dir_all(&backend.out_dir).unwrap();

    // The real engine, attached to a registry of the test: it decides with the same function
    // the hooks use and writes its entries where the test can read them.
    let engine = Arc::new(GuardrailsEngine::default());
    let registry = Arc::new(GuardRegistry::for_profile(dirs.clone()));
    engine.attach(&registry, Arc::new(|_entry| true));
    let gate: Arc<dyn GuardrailsGate> = engine;
    let mut wiring = ProtectedWiring::new(
        Arc::clone(&backend) as Arc<dyn ProtectedBackend>,
        gate,
        Duration::from_secs(10),
    );
    wiring.test_layer_override = Some(Layer::Cockpit);
    let config = DaemonConfig {
        dirs,
        env: DaemonEnv::from_vars(Vec::new()),
        git: ResolveConfig {
            configured_path: None,
            path_env: None,
            known_locations: Vec::new(),
            shim_paths: Vec::new(),
            toolchain_gits: Vec::new(),
        },
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: Some(wiring),
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        tp,
        _repo_dir: repo_dir,
        handle,
        join: Some(join),
        oplog,
        backend,
        worktree,
    }
}

impl Running {
    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// Prepares and runs `operation` from a client of its own, on a thread: the answer has a
    /// deadline, so a deadlock is a failure and not a hung test.
    fn run(&self, operation: &str, args: Value) -> Value {
        let (tx, rx) = channel();
        let mut c = self.client();
        let params = json!({
            "operation": operation, "worktree": self.worktree, "args": args, "surface": "tui",
        });
        std::thread::spawn(move || {
            let plan: Result<PrepareResult, ClientError> =
                c.call(methods::OPERATION_PREPARE, params);
            let ran = plan.and_then(|plan| {
                c.call::<_, Value>(
                    methods::OPERATION_RUN,
                    json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
                )
            });
            let _ = tx.send(ran);
        });
        match rx.recv_timeout(ANSWER_WITHIN) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => panic!("{operation}: {e:?}"),
            Err(_) => panic!("{operation}: no answer within {ANSWER_WITHIN:?}: deadlock"),
        }
    }

    /// What the hook client of `operation` was answered.
    fn hook_answer(&self, operation: OperationId) -> Value {
        let bytes = std::fs::read(self.backend.out(operation)).expect("the hook client's answer");
        serde_json::from_slice(&bytes).unwrap()
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

/// ADR-GRD-003 Enmienda Cockpit: a hook under the executor gets its answer while the operation
/// holds the repo's write lock (the step checks the lock is held), within 10 s. The decision
/// is the product's own: a branch creation nothing forbids is allowed.
#[test]
fn a_hook_under_the_executor_is_answered_while_the_repo_is_locked() {
    let r = start();
    let started = Instant::now();
    let done = r.run(
        "create-worktree",
        json!({ "branch": "feat/new", "path": r.tp.root.path().join("wt-new") }),
    );
    assert_eq!(done["outcome"], json!(OperationOutcome::Done), "{done}");
    let answer = r.hook_answer(OperationId::CreateWorktree);
    let decision: Decision = serde_json::from_value(answer["ok"].clone())
        .unwrap_or_else(|e| panic!("no decision ({e}): {answer}"));
    assert_eq!(decision.applied_effect, Effect::Allow, "{decision:?}");
    // The latency against NFR-GRD-04 (< 100 ms p95) is noted by the one who measures it.
    assert!(started.elapsed() < ANSWER_WITHIN);
}

/// D15: a child of the executor never produces a `hook-prior` snapshot, though its hook announces
/// `guard.prior-snapshot` and the operation (a rebase, a branch deletion) is one that asks for
/// it: the answer carries no prior and the oplog has no `hook-prior` row; the executor has its
/// own prior snapshot.
#[test]
fn the_executor_child_never_asks_for_a_hook_prior() {
    let r = start();
    for operation in [OperationId::RebaseOntoBase, OperationId::DiscardWorktree] {
        let done = r.run(operation.as_str(), json!({}));
        assert_eq!(done["outcome"], json!(OperationOutcome::Done), "{done}");
        let answer = r.hook_answer(operation);
        let decision: Decision = serde_json::from_value(answer["ok"].clone())
            .unwrap_or_else(|e| panic!("{operation:?}: no decision ({e}): {answer}"));
        assert_eq!(decision.applied_effect, Effect::Allow, "{decision:?}");
        assert!(
            decision.prior_snapshot.is_none(),
            "{operation:?}: the child of the executor was given a hook prior"
        );
    }
    let log = r.oplog.lock().unwrap();
    let hook_prior = log
        .snapshots(&SnapshotFilter {
            level: Some(SnapshotLevel::HookPrior),
            ..SnapshotFilter::default()
        })
        .unwrap();
    assert!(
        hook_prior.is_empty(),
        "{} hook-prior rows",
        hook_prior.len()
    );
    let own = log
        .snapshots(&SnapshotFilter {
            level: Some(SnapshotLevel::GuaranteedPrior),
            ..SnapshotFilter::default()
        })
        .unwrap();
    assert_eq!(own.len(), 2, "the executor's own prior snapshots");
}
