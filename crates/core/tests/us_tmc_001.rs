//! US-TMC-001 end to end: every operation GitRaptor launches is preceded by
//! a recoverable point. A real daemon (in-process) with its own repo layer
//! and snapshot store, a real client over the channel, testkit fixtures
//! (temporary repo, worktrees and home) and a separate temporary profile;
//! never this repo or the real profile (NFR-01).
//!
//! The executor and the catalog are TS-CKP-002: here a test catalog plugs
//! the operations' own parts into the daemon's hook (`OperationCatalog`),
//! run through `StepCtx::spawn`, and every call goes through the two-phase
//! flow (`operation.prepare`, then `operation.run`). In-process clients
//! descend from the daemon, so the wiring fixes layer `cockpit`
//! (`test_layer_override`) and a Guardrails double allows everything.
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{OperationRunResult, PriorFailedData, PriorFailure};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::oplog::{OperationState, Oplog};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter,
    ProtectedStep, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_core::timemachine::store::{CaptureError, Meta, SnapshotStore};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

/// Content of the fake credential files: not a real key.
const FAKE_KEY: &str = "fake key material\n";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

// ----- Test catalog ------------------------------------------------------------

/// What the stand-in operation does. The test catalog plugs these into ids
/// of the closed catalog: `abort-in-progress` (not governed, layer
/// `cockpit`) runs the stand-in; `discard-worktree` really removes the
/// requested linked worktree and deletes its branch.
#[derive(Clone)]
enum StandIn {
    /// Reads, changes nothing.
    Inspect,
    /// `git checkout -- .` and `git clean -fd` in the requested worktree.
    DiscardChanges,
    /// Declares a worktree of another repo in its scope.
    ForeignScope(PathBuf),
}

struct Catalog {
    fx: Arc<Fixture>,
    runs: Arc<AtomicUsize>,
    stand_in: Mutex<StandIn>,
}

struct Step {
    op: OperationId,
    stand_in: StandIn,
    fx: Arc<Fixture>,
    /// Where `git` runs.
    cwd: PathBuf,
    /// `discard-worktree`: the worktree and its branch.
    target: Option<(PathBuf, String)>,
    runs: Arc<AtomicUsize>,
}

impl OperationCatalog for Catalog {
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &gitraptor_core::timemachine::protected::RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if !matches!(
            operation,
            OperationId::AbortInProgress | OperationId::DiscardWorktree
        ) {
            return Err(PlanError::NotImplemented);
        }
        // A detached worktree has no branch to delete: refused, never an
        // empty ref in the scope.
        if operation == OperationId::DiscardWorktree && facts.head_branch.is_none() {
            return Err(PlanError::Rejected(
                gitraptor_api::catalog::RejectReason::DetachedHead,
            ));
        }
        Ok(OpPlan {
            expected: json!({ "head": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
        })
    }

    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        let target = (plan.operation == OperationId::DiscardWorktree).then(|| {
            (
                plan.facts.root.clone(),
                plan.facts.head_branch.clone().unwrap_or_default(),
            )
        });
        Ok(Box::new(Step {
            op: plan.operation,
            stand_in: self.stand_in.lock().unwrap().clone(),
            fx: Arc::clone(&self.fx),
            // A worktree is removed from the main one.
            cwd: if target.is_some() {
                canonical(&self.fx.repo)
            } else {
                plan.repo.worktree.clone()
            },
            target,
            runs: Arc::clone(&self.runs),
        }))
    }
}

impl Step {
    fn git(&self, ctx: &mut StepCtx<'_>, args: &[&str]) -> Result<(), StepError> {
        let mut cmd = self.fx.git_command(&self.cwd, args);
        cmd.stdout(std::process::Stdio::null());
        let child = ctx
            .spawn(&mut cmd)
            .map_err(|e| StepError::new(e.to_string()))?;
        let status = ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(StepError::new(format!("git {args:?} failed")))
        }
    }
}

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        match self.op {
            OperationId::DiscardWorktree => "discard-worktree",
            _ => "abort-in-progress",
        }
    }

    fn scope(&self) -> StepScope {
        if let Some((_, branch)) = &self.target {
            // Launched from the main worktree, which it declares.
            return StepScope {
                worktrees: vec![self.cwd.clone()],
                refs: vec![format!("refs/heads/{branch}")],
            };
        }
        match &self.stand_in {
            StandIn::ForeignScope(other) => StepScope {
                worktrees: vec![other.clone()],
                refs: Vec::new(),
            },
            _ => StepScope::default(),
        }
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if let Some((path, branch)) = self.target.clone() {
            self.git(
                ctx,
                &["worktree", "remove", "--force", path.to_str().unwrap()],
            )?;
            self.git(ctx, &["branch", "-D", "-q", &branch])?;
            return Ok(StepOutput {
                changed_refs: vec![format!("refs/heads/{branch}")],
                ..StepOutput::default()
            });
        }
        match self.stand_in {
            StandIn::DiscardChanges => {
                self.git(ctx, &["checkout", "--", "."])?;
                self.git(ctx, &["clean", "-fdq"])?;
            }
            _ => self.git(ctx, &["rev-parse", "-q", "--verify", "HEAD"])?,
        }
        Ok(StepOutput::default())
    }
}

/// Guardrails double: allows everything (TS-CKP-003 is not built yet).
struct AllowAll;

impl GuardrailsGate for AllowAll {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

/// Fails every prior with a real `ENOSPC` through the store's error path.
struct NoSpace;

impl PriorSnapshotter for NoSpace {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        Err(CaptureError::Io(std::io::Error::from_raw_os_error(28)).into())
    }
}

// ----- Running daemon ------------------------------------------------------------

struct Running {
    fx: Arc<Fixture>,
    tp: TempProfile,
    repo_id: String,
    runs: Arc<AtomicUsize>,
    catalog: Arc<Catalog>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

fn config(tp: &TempProfile, operations: Option<OperationsWiring>) -> DaemonConfig {
    DaemonConfig {
        dirs: tp.dirs(),
        env: DaemonEnv::from_vars(std::env::vars_os()),
        git: DaemonEnv::from_vars(std::env::vars_os()).git_resolve_config(None),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: Default::default(),
    }
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

/// Observes `fx`'s repo in a fresh profile, with the profile settings
/// `settings` if given, and starts the daemon with the test catalog.
fn start(fx: Fixture, settings: Option<&str>, no_space: bool) -> Running {
    let fx = Arc::new(fx);
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    if let Some(text) = settings {
        std::fs::create_dir_all(&dirs.config).unwrap();
        std::fs::write(dirs.config.join("settings.json"), text).unwrap();
    }
    let runs = Arc::new(AtomicUsize::new(0));
    let layer: Option<gitraptor_core::timemachine::protected::SnapshotterLayer> =
        no_space.then(|| {
            Arc::new(|_: Arc<dyn PriorSnapshotter>| Arc::new(NoSpace) as Arc<dyn PriorSnapshotter>)
                as _
        });
    let catalog = Arc::new(Catalog {
        fx: Arc::clone(&fx),
        runs: Arc::clone(&runs),
        stand_in: Mutex::new(StandIn::Inspect),
    });
    let wiring = OperationsWiring {
        catalog: Arc::clone(&catalog) as Arc<dyn OperationCatalog>,
        gate: Arc::new(AllowAll),
        test_layer_override: Some(Layer::Cockpit),
        prior_deadline: Duration::from_secs(30),
        prior_layer: layer,
    };
    let daemon = Daemon::start(config(&tp, Some(wiring))).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        runs,
        catalog,
        handle,
        join: Some(join),
    }
}

impl Running {
    /// Prepares `operation` on `worktree`, then runs the plan, from one
    /// connection.
    fn run(&self, operation: &str, worktree: &Path) -> Result<Value, ClientError> {
        let mut c = connect(&self.tp);
        let plan: PrepareResult = c.call(
            methods::OPERATION_PREPARE,
            json!({
                "operation": operation,
                "worktree": worktree.to_str().unwrap(),
                "surface": "tui",
            }),
        )?;
        c.call(
            methods::OPERATION_RUN,
            json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
        )
    }

    fn run_ok(&self, operation: &str, worktree: &Path) -> OperationRunResult {
        serde_json::from_value(self.run(operation, worktree).unwrap()).unwrap()
    }

    fn stand_in(&self, what: StandIn) {
        *self.catalog.stand_in.lock().unwrap() = what;
    }

    fn store(&self) -> SnapshotStore {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .unwrap()
            .unwrap()
    }

    /// Stops the daemon and opens the repo's oplog.
    fn stop_and_oplog(mut self) -> Oplog {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap();
        Oplog::open(&self.tp.dirs(), &self.repo_id, 2).unwrap().0
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

/// The key under `wt/` of the worktree at canonical `path` in a snapshot.
fn key_of(meta: &Meta, path: &Path) -> String {
    meta.worktrees
        .iter()
        .find(|w| Path::new(&w.path) == path)
        .unwrap_or_else(|| panic!("{path:?} not in the snapshot: {meta:?}"))
        .key
        .clone()
}

/// `path → bytes` of the worktree at `path` in snapshot `id`.
fn files(store: &SnapshotStore, id: &str, path: &Path) -> Vec<(String, Vec<u8>)> {
    let key = key_of(&store.meta(id).unwrap(), path);
    store
        .files(id, &key)
        .unwrap()
        .into_iter()
        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
        .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
        .collect()
}

fn file<'a>(files: &'a [(String, Vec<u8>)], path: &str) -> Option<&'a [u8]> {
    files
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, b)| b.as_slice())
}

fn exclusions(store: &SnapshotStore, id: &str) -> Vec<(String, String)> {
    store
        .meta(id)
        .unwrap()
        .exclusions
        .into_iter()
        .map(|e| (e.path, e.reason))
        .collect()
}

/// A repo with `lib.rs` committed and a linked worktree `name` on its own
/// branch `name`.
fn repo_with_worktree(name: &str) -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("lib.rs", "fn v1() {}\n");
    fx.git(&["add", "lib.rs"]);
    fx.git(&["commit", "-q", "-m", "lib"]);
    fx.git(&["branch", name]);
    let wt = fx.add_worktree(name, name);
    (fx, canonical(&wt))
}

fn rpc_error(result: Result<Value, ClientError>) -> (i64, Option<Value>) {
    match result {
        Err(ClientError::Rpc(e)) => (e.code, e.data),
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

// ----- Scenarios -------------------------------------------------------------------

/// Escenario: Una operación de GitRaptor guarda antes el trabajo sin commitear.
#[test]
fn a_gitraptor_operation_saves_uncommitted_work_first() {
    let (fx, wt) = repo_with_worktree("feat-login");
    std::fs::write(wt.join("lib.rs"), "fn v2_uncommitted() {}\n").unwrap();
    std::fs::write(wt.join("nuevo.rs"), "fn nuevo() {}\n").unwrap();
    let r = start(fx, None, false);
    r.stand_in(StandIn::DiscardChanges);

    let out = r.run_ok("abort-in-progress", &wt);

    // The recoverable point holds the work as it was.
    let store = r.store();
    store.verify(&out.prior_snapshot_id).unwrap();
    let prior = files(&store, &out.prior_snapshot_id, &wt);
    assert_eq!(
        file(&prior, "lib.rs"),
        Some(&b"fn v2_uncommitted() {}\n"[..])
    );
    assert_eq!(file(&prior, "nuevo.rs"), Some(&b"fn nuevo() {}\n"[..]));
    // And the operation ran.
    assert_eq!(r.runs.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(wt.join("lib.rs")).unwrap(), b"fn v1() {}\n");
    assert!(!wt.join("nuevo.rs").exists());

    let oplog = r.stop_and_oplog();
    let op = oplog.operation(&out.operation_id).unwrap().unwrap();
    assert_eq!(op.state, OperationState::Finished);
    assert_eq!(
        op.prior_snapshot.as_deref(),
        Some(out.prior_snapshot_id.as_str())
    );
}

/// Escenario: Descartar un worktree y su rama queda cubierto.
#[test]
fn discarding_a_worktree_and_its_branch_is_covered() {
    let (fx, wt) = repo_with_worktree("feat-pagos");
    std::fs::write(wt.join("lib.rs"), "fn pagos() {}\n").unwrap();
    std::fs::write(wt.join("cobro.rs"), "fn cobro() {}\n").unwrap();
    let tip = fx.git_in(&wt, &["rev-parse", "HEAD"]).trim().to_owned();
    let main = canonical(&fx.repo);
    let r = start(fx, None, false);

    // Asked for feat-pagos and launched from the main worktree: the main
    // worktree is covered only because the operation declares it.
    let out = r.run_ok("discard-worktree", &wt);

    let store = r.store();
    let meta = store.meta(&out.prior_snapshot_id).unwrap();
    assert_eq!(meta.branches.get("feat-pagos"), Some(&tip));
    let registered = meta
        .registered
        .iter()
        .find(|w| Path::new(&w.path) == wt)
        .expect("the worktree is registered in the prior");
    assert_eq!(registered.branch.as_deref(), Some("feat-pagos"));
    let prior = files(&store, &out.prior_snapshot_id, &wt);
    assert_eq!(file(&prior, "lib.rs"), Some(&b"fn pagos() {}\n"[..]));
    assert_eq!(file(&prior, "cobro.rs"), Some(&b"fn cobro() {}\n"[..]));
    // The operation ran: the worktree and the branch are gone.
    assert!(!wt.exists());
    let branches = r.fx.git(&["branch", "--list", "feat-pagos"]);
    assert!(branches.trim().is_empty(), "{branches}");

    let oplog = r.stop_and_oplog();
    let op = oplog.operation(&out.operation_id).unwrap().unwrap();
    assert_eq!(op.state, OperationState::Finished);
    let scope = &op.record.scope;
    for path in [&main, &wt] {
        assert!(
            scope
                .worktrees
                .contains(&path.to_string_lossy().into_owned()),
            "{path:?} not in {scope:?}"
        );
    }
    assert_eq!(scope.refs, ["refs/heads/feat-pagos"]);
}

/// Escenario: Los archivos ignorados no entran en el punto.
#[test]
fn ignored_files_are_not_in_the_prior() {
    let fx = Fixture::with_commit(&git());
    fx.write(".gitignore", ".env\nnode_modules/\n");
    fx.git(&["add", ".gitignore"]);
    fx.git(&["commit", "-q", "-m", "ignore"]);
    fx.write(".env", "TOKEN=fake\n");
    fx.write("node_modules/pkg/index.js", "module.exports = 1;\n");
    let main = canonical(&fx.repo);
    let before = fx.fingerprint();
    let r = start(fx, None, false);

    let out = r.run_ok("abort-in-progress", &main);

    let store = r.store();
    let prior = files(&store, &out.prior_snapshot_id, &main);
    assert!(file(&prior, ".env").is_none());
    assert!(!prior.iter().any(|(p, _)| p.starts_with("node_modules")));
    // Ignored files are not declared one by one.
    let excl = exclusions(&store, &out.prior_snapshot_id);
    assert!(
        !excl
            .iter()
            .any(|(p, _)| p.ends_with(":.env") || p.contains("node_modules")),
        "{excl:?}"
    );
    // They stay as they were: nothing in the repo or the machine changed.
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// Escenario: Las credenciales sin seguimiento se excluyen y se declaran.
#[test]
fn untracked_credentials_are_excluded_and_declared() {
    let fx = Fixture::with_commit(&git());
    fx.write("deploy.pem", FAKE_KEY);
    fx.write(".env.local", "TOKEN=fake\n");
    fx.write("nuevo.rs", "fn nuevo() {}\n");
    let main = canonical(&fx.repo);
    let before = fx.fingerprint();
    // A profile without the option.
    let r = start(fx, Some(r#"{"timeMachine": {"retentionDays": 30}}"#), false);

    let out = r.run_ok("abort-in-progress", &main);

    let store = r.store();
    let prior = files(&store, &out.prior_snapshot_id, &main);
    assert_eq!(file(&prior, "nuevo.rs"), Some(&b"fn nuevo() {}\n"[..]));
    assert!(file(&prior, "deploy.pem").is_none());
    assert!(file(&prior, ".env.local").is_none());
    let key = key_of(&store.meta(&out.prior_snapshot_id).unwrap(), &main);
    let excl = exclusions(&store, &out.prior_snapshot_id);
    for path in ["deploy.pem", ".env.local"] {
        assert!(
            excl.contains(&(format!("{key}:{path}"), "credential".into())),
            "{path} not declared: {excl:?}"
        );
    }
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// Escenario: El perfil permite incluir las credenciales.
#[test]
fn the_profile_can_include_credentials() {
    let fx = Fixture::with_commit(&git());
    fx.write("deploy.pem", FAKE_KEY);
    let main = canonical(&fx.repo);
    let r = start(
        fx,
        Some(r#"{"timeMachine": {"includeCredentialFiles": true}}"#),
        false,
    );

    let out = r.run_ok("abort-in-progress", &main);

    let store = r.store();
    let prior = files(&store, &out.prior_snapshot_id, &main);
    assert_eq!(file(&prior, "deploy.pem"), Some(FAKE_KEY.as_bytes()));
    let excl = exclusions(&store, &out.prior_snapshot_id);
    assert!(
        !excl.iter().any(|(p, _)| p.ends_with("deploy.pem")),
        "{excl:?}"
    );

    // Changing the option takes effect on the next prior, without a restart.
    std::fs::write(r.tp.dirs().config.join("settings.json"), "{}").unwrap();
    let again = r.run_ok("abort-in-progress", &main);
    let prior = files(&store, &again.prior_snapshot_id, &main);
    assert!(file(&prior, "deploy.pem").is_none());
}

/// Escenario: Si el punto previo no se puede guardar, la operación no se ejecuta.
#[test]
fn no_prior_snapshot_means_no_operation() {
    let (fx, wt) = repo_with_worktree("feat-pagos");
    std::fs::write(wt.join("lib.rs"), "fn pagos() {}\n").unwrap();
    std::fs::write(wt.join("cobro.rs"), "fn cobro() {}\n").unwrap();
    let before = fx.fingerprint();
    let r = start(fx, None, true);

    let (code, data) = rpc_error(r.run("discard-worktree", &wt));

    // The requester gets the reason.
    assert_eq!(code, code::PRIOR_SNAPSHOT_FAILED);
    let data: PriorFailedData = serde_json::from_value(data.unwrap()).unwrap();
    assert_eq!(data.reason, PriorFailure::NoSpace);
    // The operation did not run: worktree, branch and work intact.
    assert_eq!(r.runs.load(Ordering::SeqCst), 0);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");

    let oplog = r.stop_and_oplog();
    let op = oplog
        .operation(&data.operation_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::Aborted);
    assert!(op.prior_snapshot.is_none());
}

// ----- Scope and wiring -------------------------------------------------------------

/// A worktree of another repo in the declared scope, or a folder that is
/// not an observed worktree, is refused before anything is recorded.
#[test]
fn a_scope_outside_the_repo_is_refused() {
    let fx = Fixture::with_commit(&git());
    fx.git_in(
        &fx.other_repo,
        &["commit", "-q", "--allow-empty", "-m", "x"],
    );
    let main = canonical(&fx.repo);
    let other = canonical(&fx.other_repo);
    let r = start(fx, None, false);

    r.stand_in(StandIn::ForeignScope(other.clone()));
    let (code, _) = rpc_error(r.run("abort-in-progress", &main));
    assert_eq!(code, code::SCOPE_REFUSED);
    r.stand_in(StandIn::Inspect);
    let (code, _) = rpc_error(r.run("abort-in-progress", &other));
    assert_eq!(code, code::SCOPE_REFUSED);
    let (code, _) = rpc_error(r.run("abort-in-progress", &main.join("sub")));
    assert_eq!(code, code::SCOPE_REFUSED);
    assert_eq!(r.runs.load(Ordering::SeqCst), 0);

    let oplog = r.stop_and_oplog();
    assert!(oplog.operations(&Default::default()).unwrap().is_empty());
}

/// Without a catalog the daemon still answers "not implemented" (F-001-02):
/// the first operation story wires it (US-MCP-008).
#[test]
fn without_a_catalog_operations_are_not_implemented() {
    let tp = TempProfile::new();
    let daemon = Daemon::start(config(&tp, None)).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let (code, _) = rpc_error(connect(&tp).call(
        methods::OPERATION_PREPARE,
        json!({ "operation": "abort-in-progress", "worktree": "/tmp", "surface": "tui" }),
    ));
    assert_eq!(code, code::NOT_IMPLEMENTED);
    handle.request(StopCause::Signal("TERM"));
    join.join().unwrap();
}
