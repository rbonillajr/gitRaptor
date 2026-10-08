//! US-TMC-002 end to end: `timemachine.undo` takes back the last operation
//! of the worktree it is asked from. A real daemon (in-process) with its own
//! repo layer and snapshot store, a real client over the channel, real Git,
//! testkit fixtures (temporary repo, worktrees and home) and a separate
//! temporary profile; never this repo or the real profile (NFR-01).
//!
//! The operations to undo are real Git commands run by a test catalog
//! (`StepCtx::spawn`) through `operation.prepare` and `operation.run`, like
//! US-TMC-001: production has no catalog yet. In-process clients descend
//! from the daemon, so they resolve as "unattributed", the requester of the
//! scenarios. No test waits on time: each step waits for the daemon's
//! answer.
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    OperationRunResult, PriorFailedData, PriorFailure, TmRejectReason, TmRejectedData, UndoResult,
};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
    TmPriorLayer,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::oplog::{
    Channel, OpRef, OperationKind, OperationState, Oplog, Requester, SnapshotRefs, Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter,
    ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_core::timemachine::store::{CaptureError, SnapshotStore};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

// ----- Test catalog: real Git commands as the operations to undo ----------------

/// The Git commands the next operation runs in the requested worktree, and
/// the subtype it is recorded with.
#[derive(Clone, Default)]
struct Script {
    subtype: &'static str,
    commands: Vec<Vec<String>>,
}

fn script(subtype: &'static str, commands: &[&[&str]]) -> Script {
    Script {
        subtype,
        commands: commands
            .iter()
            .map(|c| c.iter().map(|a| (*a).to_owned()).collect())
            .collect(),
    }
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
        StepScope::default()
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

// ----- Running daemon --------------------------------------------------------------

struct Running {
    fx: Arc<Fixture>,
    tp: TempProfile,
    repo_id: String,
    catalog: Arc<Catalog>,
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

/// Observes `fx`'s repo in a fresh profile and starts the daemon with the
/// test catalog; with `undo_no_space`, the undo's prior fails with `ENOSPC`.
fn start(fx: Fixture, undo_no_space: bool) -> Running {
    start_with(fx, undo_no_space, CaptureConfig::default())
}

/// [`start`] with the continuous capture of `capture`.
fn start_with(fx: Fixture, undo_no_space: bool, capture: CaptureConfig) -> Running {
    let fx = Arc::new(fx);
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
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
        tm_prior_layer: undo_no_space.then(|| {
            TmPriorLayer(Arc::new(|_: Arc<dyn PriorSnapshotter>| {
                Arc::new(NoSpace) as Arc<dyn PriorSnapshotter>
            }))
        }),
        tiers: Default::default(),
        tm_capture: TmCapture {
            config: capture,
            ..Default::default()
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        catalog,
        handle,
        join: Some(join),
    }
}

impl Running {
    /// Runs `script` as an operation on `worktree` (prepare, then run).
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

    fn undo(&self, worktree: &Path) -> Result<Value, ClientError> {
        connect(&self.tp).call(
            methods::TM_UNDO,
            json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
        )
    }

    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        serde_json::from_value(self.undo(worktree).unwrap()).unwrap()
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

fn rejected(result: Result<Value, ClientError>) -> (i64, Option<Value>) {
    match result {
        Err(ClientError::Rpc(e)) => (e.code, e.data),
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn reject_reason(result: Result<Value, ClientError>) -> TmRejectedData {
    let (code, data) = rejected(result);
    assert_eq!(code, code::OPERATION_REJECTED);
    serde_json::from_value(data.unwrap()).unwrap()
}

/// What a worktree holds that a user sees: its files (outside `.git`),
/// `HEAD` and the status.
#[derive(Debug, PartialEq, Eq)]
struct State {
    files: BTreeMap<PathBuf, Vec<u8>>,
    head: String,
    branch: String,
    status: String,
}

fn state(fx: &Fixture, root: &Path) -> State {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_owned();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    State {
        files,
        head: fx.git_in(root, &["rev-parse", "HEAD"]).trim().to_owned(),
        branch: fx
            .git_in(root, &["symbolic-ref", "-q", "--short", "HEAD"])
            .trim()
            .to_owned(),
        status: fx.git_in(root, &["status", "--porcelain=v1"]),
    }
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

fn reset_hard() -> Script {
    script("reset-hard", &[&["reset", "-q", "--hard"]])
}

// ----- Scenarios -------------------------------------------------------------------

/// Escenario: Deshacer la última operación del worktree actual.
#[test]
fn undo_recovers_the_last_operation_of_the_worktree() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { trabajo_sin_commitear(); }\n").unwrap();
    let before = state(&fx, &wt);
    let r = start(fx, false);
    let op = r.operation(&wt, reset_hard());
    assert_eq!(std::fs::read(wt.join("a.rs")).unwrap(), b"fn a() {}\n");

    let undo = r.undo_ok(&wt);

    // "feat-login" is back to the state before the operation, a.rs with it.
    assert_eq!(state(&r.fx, &wt), before);
    assert_eq!(undo.undone_operation_id, op.operation_id);
    assert_eq!(undo.target_snapshot_id, op.prior_snapshot_id);
    assert_eq!(
        undo.undone_subtype.as_ref().map(|s| s.sanitized()),
        Some("reset-hard".to_owned())
    );
    assert!(undo.not_restored.is_empty());
    // The undo is recorded with its requester and the operation it acted on.
    let oplog = r.stop_and_oplog();
    let rec = oplog.operation(&undo.operation_id).unwrap().unwrap();
    assert_eq!(rec.state, OperationState::Finished);
    assert_eq!(rec.record.kind, OperationKind::Undo);
    assert_eq!(rec.record.requester, Requester::Unattributed);
    assert_eq!(rec.record.channel, Channel::Cli);
    assert_eq!(
        rec.record.target,
        Target::Undo(vec![OpRef::Oplog(op.operation_id.clone())])
    );
    assert_eq!(
        rec.record.scope.worktrees,
        [wt.to_string_lossy().into_owned()]
    );
    assert_eq!(rec.record.scope.refs, ["refs/heads/feat-login"]);
}

/// Escenario: El undo queda protegido por su propio punto previo.
#[test]
fn the_undo_has_its_own_prior_point() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { v2(); }\n").unwrap();
    let r = start(fx, false);
    r.operation(&wt, reset_hard());
    // Work done after the operation: the undo takes it back, its prior keeps it.
    std::fs::write(wt.join("despues.rs"), "fn despues() {}\n").unwrap();
    let just_before = state(&r.fx, &wt);

    let undo = r.undo_ok(&wt);
    assert!(!wt.join("despues.rs").exists());

    let store = r.store();
    store.verify(&undo.prior_snapshot_id).unwrap();
    let meta = store.meta(&undo.prior_snapshot_id).unwrap();
    let key = &meta
        .worktrees
        .iter()
        .find(|w| Path::new(&w.path) == wt)
        .unwrap()
        .key;
    let files: BTreeMap<PathBuf, Vec<u8>> = store
        .files(&undo.prior_snapshot_id, key)
        .unwrap()
        .into_iter()
        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
        .map(|(p, _, oid)| (PathBuf::from(p), store.read_blob(oid).unwrap()))
        .collect();
    assert_eq!(files, just_before.files);
    assert_eq!(
        meta.worktrees
            .iter()
            .find(|w| &w.key == key)
            .unwrap()
            .head_commit,
        Some(just_before.head)
    );
    // And it is a point of the timeline: complete, and the undo's prior.
    let oplog = r.stop_and_oplog();
    let point = oplog.snapshot(&undo.prior_snapshot_id).unwrap().unwrap();
    assert!(point.state.is_available() && !point.tampered);
    let rec = oplog.operation(&undo.operation_id).unwrap().unwrap();
    assert_eq!(
        rec.prior_snapshot.as_deref(),
        Some(undo.prior_snapshot_id.as_str())
    );
}

/// Escenario: Undos seguidos retroceden una operación más cada vez.
#[test]
fn consecutive_undos_walk_back() {
    let (fx, wt) = repo_with_login();
    fx.git(&["branch", "B"]);
    std::fs::write(wt.join("a.rs"), "fn a() { cambio_a(); }\n").unwrap();
    let before_a = state(&fx, &wt);
    let r = start(fx, false);
    let a = r.operation(
        &wt,
        script("commit", &[&["add", "-A"], &["commit", "-q", "-m", "A"]]),
    );
    let after_a = state(&r.fx, &wt);
    let b = r.operation(&wt, script("checkout", &[&["checkout", "-q", "B"]]));
    assert_eq!(state(&r.fx, &wt).branch, "B");

    let first = r.undo_ok(&wt);
    assert_eq!(first.undone_operation_id, b.operation_id);
    assert_eq!(state(&r.fx, &wt), after_a);

    let second = r.undo_ok(&wt);
    assert_eq!(second.undone_operation_id, a.operation_id);
    // "feat-login" is as before "A": branch, HEAD, files and status.
    assert_eq!(state(&r.fx, &wt), before_a);
    assert_eq!(r.fx.git(&["rev-parse", "feat-login"]).trim(), before_a.head);
}

/// Escenario: El undo no actúa sobre otros worktrees.
#[test]
fn undo_never_touches_other_worktrees() {
    let (fx, login) = repo_with_login();
    fx.git(&["branch", "feat-pagos"]);
    let pagos = canonical(&fx.add_worktree("feat-pagos", "feat-pagos"));
    std::fs::write(login.join("a.rs"), "fn a() { login(); }\n").unwrap();
    let login_before = state(&fx, &login);
    let r = start(fx, false);
    let in_login = r.operation(&login, reset_hard());
    // The repo's last operation is in feat-pagos.
    std::fs::write(pagos.join("pagos.rs"), "fn cobrar() {}\n").unwrap();
    r.operation(
        &pagos,
        script(
            "commit",
            &[&["add", "-A"], &["commit", "-q", "-m", "pagos"]],
        ),
    );
    let pagos_tip = r.fx.git(&["rev-parse", "feat-pagos"]);
    let pagos_print = r.fx.fingerprint();

    let undo = r.undo_ok(&login);

    assert_eq!(undo.undone_operation_id, in_login.operation_id);
    assert_eq!(state(&r.fx, &login), login_before);
    // feat-pagos did not change: not a byte, not its branch.
    let changes: Vec<_> = diff(&pagos_print, &r.fx.fingerprint())
        .into_iter()
        .filter(|c| c.scope == "wt-feat-pagos")
        .collect();
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(r.fx.git(&["rev-parse", "feat-pagos"]), pagos_tip);
}

/// Escenario: No hay nada que deshacer.
#[test]
fn nothing_to_undo_changes_nothing() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { sin_tocar(); }\n").unwrap();
    let r = start(fx, false);
    let before = r.fx.fingerprint();

    let refusal = reject_reason(r.undo(&wt));

    assert_eq!(refusal.reason, TmRejectReason::NothingToUndo);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    // The request is in the oplog, rejected with its reason.
    let id = refusal.operation_id.unwrap();
    let oplog = r.stop_and_oplog();
    let rec = oplog.operation(&id).unwrap().unwrap();
    assert_eq!(rec.state, OperationState::Rejected);
    assert_eq!(rec.record.kind, OperationKind::Undo);
    assert_eq!(rec.record.requester, Requester::Unattributed);
    assert_eq!(rec.prior_snapshot, None);
}

/// Escenario: Si el punto previo al undo falla, el undo no se ejecuta.
#[test]
fn a_failed_prior_means_no_undo() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { perdido(); }\n").unwrap();
    let r = start(fx, true);
    r.operation(&wt, reset_hard());
    let before = r.fx.fingerprint();

    let (code, data) = rejected(r.undo(&wt));

    assert_eq!(code, code::PRIOR_SNAPSHOT_FAILED);
    let data: PriorFailedData = serde_json::from_value(data.unwrap()).unwrap();
    assert_eq!(data.reason, PriorFailure::NoSpace);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    let oplog = r.stop_and_oplog();
    let rec = oplog
        .operation(&data.operation_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(rec.state, OperationState::Aborted);
    assert_eq!(rec.record.kind, OperationKind::Undo);
}

/// Git preconditions run before the undo's prior (ADR-TMC-005 § 4): with a
/// merge in progress the undo is rejected, nothing changes and no point is
/// spent; once it is gone, the same undo runs.
///
/// Continuous capture is off: writing `MERGE_HEAD` is a Git event, and a
/// capture of its own once the worktree is quiet (`Q`) would add a point
/// whenever the undo answers later than `Q`, as under load.
#[test]
fn an_undo_with_a_git_operation_in_progress_is_rejected() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { v2(); }\n").unwrap();
    let no_capture = CaptureConfig {
        enabled: false,
        ..CaptureConfig::default()
    };
    let r = start_with(fx, false, no_capture);
    let op = r.operation(&wt, reset_hard());
    let gitdir = PathBuf::from(
        r.fx.git_in(&wt, &["rev-parse", "--absolute-git-dir"])
            .trim(),
    );
    let head = r.fx.git(&["rev-parse", "HEAD"]);
    std::fs::write(gitdir.join("MERGE_HEAD"), head).unwrap();
    let before = r.fx.fingerprint();
    let snapshots = |r: &Running| r.store().snapshot_ids().unwrap().unwrap().len();
    let points = snapshots(&r);

    let refusal = reject_reason(r.undo(&wt));

    assert_eq!(refusal.reason, TmRejectReason::GitOperationInProgress);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
    assert_eq!(snapshots(&r), points);
    std::fs::remove_file(gitdir.join("MERGE_HEAD")).unwrap();
    assert_eq!(r.undo_ok(&wt).undone_operation_id, op.operation_id);
}

/// A branch the undo would move but that is checked out in a worktree
/// outside its scope is never moved (`ref-in-use`): undoing a checkout in
/// feat-login cannot take its HEAD back to a branch the main worktree now
/// has out.
#[test]
fn a_branch_checked_out_elsewhere_is_never_moved() {
    let (fx, wt) = repo_with_login();
    fx.git(&["branch", "B"]);
    let r = start(fx, false);
    r.operation(&wt, script("checkout", &[&["checkout", "-q", "B"]]));
    r.fx.git(&["checkout", "-q", "feat-login"]);
    let before = r.fx.fingerprint();

    let refusal = reject_reason(r.undo(&wt));

    assert_eq!(refusal.reason, TmRejectReason::RefInUse);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}
