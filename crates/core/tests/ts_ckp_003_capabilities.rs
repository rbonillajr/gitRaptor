//! TS-CKP-003 (D10): `operation.guardrails` in both directions, through the channel. A governed
//! plan the real `GuardrailsEngine` denies (a merge into `main`, which nothing may merge into
//! without a confirmation) is prepared and run from two connections of the same daemon: one
//! that accepted `operation.guardrails` and one that did not. With it, `PrepareResult` and the
//! `data` of `-32014` carry a `GuardrailsDenial` whose `decision_id` is the one of the log entry
//! the plan leaves; without it, the shape of today and no `guardrails` field at all.
//!
//! In-process daemon, step double that never runs (the plan is denied before any effect),
//! temporary profile and repo (NFR-01). The wiring fixes layer `cockpit` with
//! `test_layer_override`, as `channel_protected.rs` does.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::{TempProfile, common_dir, git, init_repo};
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, RejectReason};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{ChannelConfig, ProtectedWiring};
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{Affected, GuardrailsGate, OpPlan, PlanError, RepoFacts, StepPlan};
use gitraptor_core::guardrails::log::LogEntry;
use gitraptor_core::guardrails::plan_gate::GuardrailsEngine;
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::timemachine::oplog::{CompleteInfo, NewSnapshot, Oplog, SnapshotLevel};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedBackend, ProtectedStep,
    RepoHandle, ScopeError, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_policy::guard::plan::PlanShape;
use serde_json::{Value, json};

const REPO_A: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";

struct Snap(Arc<Mutex<Oplog>>);

impl PriorSnapshotter for Snap {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        let mut log = self.0.lock().unwrap();
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

struct NeverRuns;

impl ProtectedStep for NeverRuns {
    fn subtype(&self) -> &str {
        "merge-into-base"
    }
    fn scope(&self) -> StepScope {
        StepScope::default()
    }
    fn run(&mut self, _ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        Err(StepError::new("a denied plan must never reach its step"))
    }
}

struct Backend {
    repo: RepoHandle,
    main: String,
    feat: String,
    lock_prefix: String,
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
        &Allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("{}/{}", self.lock_prefix, repo.repo_id)
    }
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        RepoFacts::read(&repo.worktree)
    }
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::MergeIntoBase {
            return Err(PlanError::NotImplemented);
        }
        Ok(OpPlan {
            expected: json!({ "start": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: Some(PlanShape::Merge {
                base: "refs/heads/main".into(),
                old: self.main.clone(),
                integrated: self.feat.clone(),
            }),
        })
    }
    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Ok(Box::new(NeverRuns))
    }
}

struct Running {
    tp: TempProfile,
    _repo_dir: tempfile::TempDir,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    worktree: PathBuf,
    /// What the engine wrote to the log: one entry per plan.
    entries: Arc<Mutex<Vec<LogEntry>>>,
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
            snapshotter: Arc::new(Snap(Arc::clone(&oplog))),
        },
        main,
        feat,
        lock_prefix: tp.root.path().display().to_string(),
    });
    // The real engine, attached to a registry of the test and to a record that keeps the entries.
    let entries = Arc::new(Mutex::new(Vec::new()));
    let engine = Arc::new(GuardrailsEngine::default());
    let registry = Arc::new(GuardRegistry::for_profile(dirs.clone()));
    let kept = Arc::clone(&entries);
    engine.attach(
        &registry,
        Arc::new(move |entry| {
            kept.lock().unwrap().push(entry);
            true
        }),
    );
    let gate: Arc<dyn GuardrailsGate> = engine;
    let mut wiring = ProtectedWiring::new(
        backend as Arc<dyn ProtectedBackend>,
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
        predict: Default::default(),
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
        worktree,
        entries,
    }
}

impl Running {
    /// A library client: it accepts every capability it knows, `operation.guardrails` included.
    fn with_capability(&self) -> Client {
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

    /// A raw connection of protocol 9 that accepts nothing.
    fn without_capability(&self) -> Raw {
        let stream = UnixStream::connect(socket_path(&self.tp.dirs()).unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Raw {
            stream,
            reader,
            next: 1,
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello["result"].is_object(), "{hello}");
        raw
    }

    fn params(&self) -> Value {
        json!({
            "operation": "merge-into-base", "worktree": self.worktree,
            "args": {}, "surface": "tui",
        })
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

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Raw {
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let mut answer = String::new();
            self.reader.read_line(&mut answer).unwrap();
            let answer: Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    }
}

/// D10 in both directions.
#[test]
fn operation_guardrails_only_with_its_capability() {
    let r = start();

    // With the capability: the plan says why it was denied, and so does `-32014`.
    let mut with = r.with_capability();
    let prepared: Value = with
        .call(methods::OPERATION_PREPARE, r.params())
        .expect("prepare");
    assert_eq!(prepared["decision"], "deny", "{prepared:#}");
    assert_eq!(prepared["guardrails"]["effect"], "deny", "{prepared:#}");
    assert_eq!(prepared["guardrails"]["appliedEffect"], "deny");
    assert_eq!(
        prepared["guardrails"]["reasons"][0]["rule"], "policy.protected-branch",
        "{prepared:#}"
    );
    assert!(
        prepared["guardrails"]["decisionId"]
            .as_str()
            .is_some_and(|d| !d.is_empty())
    );
    let ran = with.call::<_, Value>(
        methods::OPERATION_RUN,
        json!({ "plan_id": prepared["plan_id"], "accepted_warnings": prepared["warnings"] }),
    );
    let Err(ClientError::Rpc(denied)) = ran else {
        panic!("a denied plan ran: {ran:?}");
    };
    assert_eq!(denied.code, code::OPERATION_REJECTED);
    let data = denied.data.expect("data of the rejection");
    assert_eq!(data["reason"], "guardrails-denied", "{data}");
    assert_eq!(
        data["guardrails"]["reasons"][0]["rule"], "policy.protected-branch",
        "{data}"
    );
    // The response and the entry the plan leaves share the decision.
    let entries = r.entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "one entry per plan");
    assert_eq!(
        data["guardrails"]["decisionId"].as_str(),
        Some(entries[0].decision_id.as_str())
    );
    drop(entries);

    // Without it: the shape of today, and no `guardrails` anywhere.
    let mut without = r.without_capability();
    let answer = without.call(methods::OPERATION_PREPARE, r.params());
    let prepared = &answer["result"];
    assert_eq!(prepared["decision"], "deny", "{answer:#}");
    assert!(prepared.get("guardrails").is_none(), "{answer:#}");
    let ran = without.call(
        methods::OPERATION_RUN,
        json!({ "plan_id": prepared["plan_id"], "accepted_warnings": prepared["warnings"] }),
    );
    assert_eq!(ran["error"]["code"], code::OPERATION_REJECTED, "{ran}");
    assert_eq!(ran["error"]["data"]["reason"], "guardrails-denied", "{ran}");
    assert!(ran["error"]["data"].get("guardrails").is_none(), "{ran}");
}
