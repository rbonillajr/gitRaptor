//! US-TMC-009 end to end: `timemachine.restore` brings the worktree it is
//! asked from back to a point of the timeline. A real daemon (in-process)
//! with its own repo layer and snapshot store, a real client over the
//! channel, real Git, testkit fixtures (temporary repo, worktrees and home)
//! and a separate temporary profile; never this repo or the real profile
//! (NFR-01).
//!
//! The work done after the point is real Git commands run by a test catalog
//! through `operation.prepare` and `operation.run`, like US-TMC-002; a step
//! declares the worktrees and refs it touches. "The point" is the prior
//! snapshot of the first operation after the reference state. In-process
//! clients descend from the daemon, so they resolve as "unattributed". An
//! agent's work is written into the daemon's oplog as the executor would
//! record it. No test waits on time: each step waits for the daemon's
//! answer.
//!
//! macOS and Linux, like the other channel tests.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::Untrusted;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    OperationRunResult, PriorFailedData, PriorFailure, RestoreResult, TmRejectReason,
    TmRejectedData, UndoResult,
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
    Channel, CompleteInfo, NewOperation, NewSnapshot, OperationKind, OperationState,
    OperationTransition, OperationView, Oplog, Requester, RequesterOrigin, Scope, SnapshotLevel,
    SnapshotRefs, SnapshotState, Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter,
    ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_core::timemachine::restore::{
    KEPT_REF_IN_RECREATED_WORKTREE, recreated_worktree_warning,
};
use gitraptor_core::timemachine::store::{CaptureError, SnapshotStore};
use gitraptor_testkit::fingerprint::{Kind, Snapshot};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

// ----- Test catalog: real Git commands as the work done after the point -----------

/// The Git commands the next operation runs in the requested worktree, the
/// subtype it is recorded with and the scope it declares.
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

impl Script {
    /// Declares other worktrees and the refs the operation moves or deletes.
    fn declaring(mut self, worktrees: &[&Path], refs: &[&str]) -> Self {
        self.scope = StepScope {
            worktrees: worktrees.iter().map(|p| p.to_path_buf()).collect(),
            refs: refs.iter().map(|r| (*r).to_owned()).collect(),
        };
        self
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

/// Guardrails double: allows everything.
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
    /// The daemon's own oplog of the repo, shared with it.
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

/// Observes `fx`'s repo in a fresh profile and starts the daemon with the
/// test catalog; with `tm_no_space`, the Time Machine's priors (the
/// restore's) fail with `ENOSPC`.
fn start(fx: Fixture, tm_no_space: bool) -> Running {
    start_with(fx, tm_no_space, CaptureConfig::default())
}

/// [`start`] with the continuous capture of `capture`.
fn start_with(fx: Fixture, tm_no_space: bool, capture: CaptureConfig) -> Running {
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
        tm_prior_layer: tm_no_space.then(|| {
            TmPriorLayer(Arc::new(|_: Arc<dyn PriorSnapshotter>| {
                Arc::new(NoSpace) as Arc<dyn PriorSnapshotter>
            }))
        }),
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: TmCapture {
            config: capture,
            ..Default::default()
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        catalog,
        oplog,
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

    fn restore_ok(&self, worktree: &Path, snapshot_id: &str) -> RestoreResult {
        match self.restore(worktree, snapshot_id) {
            Ok(v) => serde_json::from_value(v).unwrap(),
            Err(e) => panic!("the restore failed: {e:?}"),
        }
    }

    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        match connect(&self.tp).call(
            methods::TM_UNDO,
            json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
        ) {
            Ok(v) => serde_json::from_value(v).unwrap(),
            Err(e) => panic!("the undo failed: {e:?}"),
        }
    }

    fn store(&self) -> SnapshotStore {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .unwrap()
            .unwrap()
    }

    fn op(&self, id: &str) -> OperationView {
        self.oplog.lock().unwrap().operation(id).unwrap().unwrap()
    }

    fn operation_count(&self) -> usize {
        self.oplog
            .lock()
            .unwrap()
            .operations(&Default::default())
            .unwrap()
            .len()
    }

    /// The key the point `snapshot_id` gives the worktree at `root`.
    fn key_in(&self, snapshot_id: &str, root: &Path) -> String {
        self.store()
            .meta(snapshot_id)
            .unwrap()
            .worktrees
            .iter()
            .find(|w| Path::new(&w.path) == root)
            .unwrap()
            .key
            .clone()
    }

    /// Writes an operation into the daemon's oplog as the executor or the
    /// recovery leaves it: its intent, its prior and `steps` after that.
    fn inject(&self, new: &NewOperation, prior: &str, last: OperationTransition<'_>) -> String {
        let mut log = self.oplog.lock().unwrap();
        let id = log.record_operation(new, now_ms()).unwrap();
        for step in [
            OperationTransition::PriorSnapshot { snapshot_id: prior },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            last,
        ] {
            log.advance_operation(&id, step, now_ms()).unwrap();
        }
        id
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
    assert_eq!(code, code::OPERATION_REJECTED, "{data:?}");
    serde_json::from_value(data.unwrap()).unwrap()
}

/// One path of a worktree as Git can see it: a regular file (its exec bits
/// and its bytes) or a symlink (its target).
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    File { exec: u32, content: Vec<u8> },
    Symlink(PathBuf),
    Other,
}

/// The exact state of a worktree: every file outside its own `.git`
/// (tracked, untracked and ignored) with its type, exec bits and bytes;
/// `HEAD` (symbolic and resolved), the branch and the status; the index
/// content (`ls-files -s`, so staged and unstaged work with the same status
/// differ); `refs/stash` and the per-worktree refs; and its own entry of the
/// worktree list. Refs and worktrees of the rest of the repo are left out:
/// several scenarios compare one worktree while another one changes by
/// design. Times, inodes, the index stat cache and object packs are left out
/// too: a restore and its undo may rewrite them without changing what the
/// user has.
#[derive(Debug, PartialEq, Eq)]
struct State {
    files: BTreeMap<PathBuf, Entry>,
    head: String,
    symbolic_head: String,
    branch: String,
    status: String,
    index: String,
    own_refs: String,
    listed: String,
}

fn state(fx: &Fixture, root: &Path) -> State {
    use std::os::unix::fs::PermissionsExt;

    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Entry>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if dir == root && entry.file_name() == ".git" {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let rel = path.strip_prefix(root).unwrap().to_owned();
            let kind = meta.file_type();
            if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_symlink() {
                out.insert(rel, Entry::Symlink(std::fs::read_link(&path).unwrap()));
            } else if kind.is_file() {
                let exec = meta.permissions().mode() & 0o111;
                out.insert(
                    rel,
                    Entry::File {
                        exec,
                        content: std::fs::read(&path).unwrap(),
                    },
                );
            } else {
                out.insert(rel, Entry::Other);
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    // `symbolic-ref` fails on a detached `HEAD`, which is a state too.
    let symbolic = fx
        .git_command(root, &["symbolic-ref", "-q", "HEAD"])
        .output()
        .unwrap();
    let me = canonical(root);
    let listed = fx
        .git_in(root, &["worktree", "list", "--porcelain"])
        .split("\n\n")
        .find(|block| {
            block
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("worktree "))
                .is_some_and(|p| Path::new(p).canonicalize().ok().as_deref() == Some(&me))
        })
        .unwrap_or_default()
        .to_owned();
    State {
        files,
        head: fx.git_in(root, &["rev-parse", "HEAD"]).trim().to_owned(),
        symbolic_head: format!(
            "{:?} {}",
            symbolic.status.code(),
            String::from_utf8_lossy(&symbolic.stdout).trim()
        ),
        branch: fx
            .git_in(root, &["symbolic-ref", "-q", "--short", "HEAD"])
            .trim()
            .to_owned(),
        status: fx.git_in(root, &["status", "--porcelain=v1"]),
        index: fx.git_in(root, &["ls-files", "-s"]),
        own_refs: fx.git_in(
            root,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/stash",
                "refs/worktree",
                "refs/bisect",
            ],
        ),
        listed,
    }
}

/// Every local branch and where it points.
fn branches(fx: &Fixture) -> BTreeMap<String, String> {
    fx.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/heads",
    ])
    .lines()
    .filter_map(|l| l.split_once(' '))
    .map(|(n, o)| (n.to_owned(), o.to_owned()))
    .collect()
}

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_owned()
}

fn branch_exists(fx: &Fixture, branch: &str) -> bool {
    fx.git_command(
        &fx.repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .output()
    .unwrap()
    .status
    .success()
}

/// Branch names of a result, short or full: compared as short names.
fn names(list: &[Untrusted]) -> Vec<String> {
    list.iter()
        .map(|u| {
            let raw = u.raw();
            raw.strip_prefix("refs/heads/").unwrap_or(raw).to_owned()
        })
        .collect()
}

fn paths(list: &[Untrusted]) -> Vec<PathBuf> {
    list.iter().map(|u| PathBuf::from(u.raw())).collect()
}

/// The fingerprint entries under `prefix` of scope `scope`: kind, size,
/// content hash and inode.
fn entries_under(
    snap: &Snapshot,
    scope: &str,
    prefix: &Path,
) -> BTreeMap<PathBuf, (Kind, u64, u64, u64)> {
    snap.entries
        .iter()
        .filter(|((s, p), _)| s == scope && p.starts_with(prefix))
        .map(|((_, p), e)| (p.clone(), (e.kind, e.size, e.hash, e.ino)))
        .collect()
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

/// A linked worktree `feat-x` on its own branch, with an untracked file.
fn add_feat_x(fx: &Fixture) -> PathBuf {
    fx.git(&["branch", "feat-x"]);
    let x = canonical(&fx.add_worktree("feat-x", "feat-x"));
    std::fs::write(x.join("x.rs"), "fn x() { sin_commitear(); }\n").unwrap();
    x
}

/// A branch `name` one commit ahead of `from`, without checking it out.
fn branch_ahead(fx: &Fixture, name: &str, from: &str) -> String {
    let tree = format!("{from}^{{tree}}");
    let tree = fx.git(&["rev-parse", &tree]).trim().to_owned();
    let commit = fx
        .git(&["commit-tree", &tree, "-p", from, "-m", name])
        .trim()
        .to_owned();
    fx.git(&["branch", name, &commit]);
    commit
}

fn reset_hard() -> Script {
    script("reset-hard", &[&["reset", "-q", "--hard"]])
}

fn commit_all(message: &str) -> Vec<Vec<String>> {
    vec![
        vec!["add".into(), "-A".into()],
        vec!["commit".into(), "-q".into(), "-m".into(), message.into()],
    ]
}

fn commit(subtype: &'static str, message: &str) -> Script {
    Script {
        subtype,
        commands: commit_all(message),
        scope: StepScope::default(),
    }
}

fn unattributed(worktree: &Path, refs: &[&str]) -> NewOperation {
    NewOperation {
        kind: OperationKind::Restore,
        subtype: None,
        scope: Scope {
            worktrees: vec![worktree.to_string_lossy().into_owned()],
            refs: refs.iter().map(|r| (*r).to_owned()).collect(),
        },
        requester: Requester::Unattributed,
        channel: Channel::Cli,
        confirmed: false,
        target: Target::None,
        warnings: Vec::new(),
        engine_mark: i64::MAX / 2,
    }
}

/// The restore of `id` was recorded rejected, of kind `restore`, with no
/// prior snapshot.
fn assert_recorded_rejected(r: &Running, refusal: &TmRejectedData, target: &str) {
    let id = refusal.operation_id.as_deref().unwrap();
    let rec = r.op(id);
    assert_eq!(rec.state, OperationState::Rejected);
    assert_eq!(rec.record.kind, OperationKind::Restore);
    assert_eq!(rec.record.requester, Requester::Unattributed);
    assert_eq!(rec.record.target, Target::Snapshot(target.to_owned()));
    assert_eq!(rec.prior_snapshot, None);
}

// ----- Scenarios -------------------------------------------------------------------

/// Escenario 1: Restaurar a un punto anterior.
#[test]
fn restore_returns_the_worktree_to_the_point_with_its_uncommitted_work() {
    let (fx, wt) = repo_with_login();
    // Uncommitted, staged and untracked work at the point.
    std::fs::write(wt.join("a.rs"), "fn a() { trabajo_sin_commitear(); }\n").unwrap();
    std::fs::write(wt.join("b.txt"), "beta preparado\n").unwrap();
    fx.git_in(&wt, &["add", "b.txt"]);
    std::fs::write(wt.join("nuevo.rs"), "fn nuevo() {}\n").unwrap();
    let at_point = state(&fx, &wt);
    let r = start(fx, false);

    // Three operations after the point: commit, edit + commit, reset --hard.
    let first = r.operation(&wt, commit("commit", "uno"));
    let point = first.prior_snapshot_id.clone();
    std::fs::write(wt.join("a.rs"), "fn a() { dos(); }\n").unwrap();
    r.operation(&wt, commit("commit", "dos"));
    std::fs::write(wt.join("a.rs"), "fn a() { descartado(); }\n").unwrap();
    r.operation(&wt, reset_hard());
    assert_ne!(state(&r.fx, &wt), at_point);

    let restore = r.restore_ok(&wt, &point);

    // Files, HEAD, branch and status as at the point, uncommitted work included.
    assert_eq!(state(&r.fx, &wt), at_point);
    assert_eq!(restore.target_snapshot_id, point);
    assert_eq!(paths(&restore.worktrees).first(), Some(&wt));
    assert!(restore.recreated.is_empty());
    assert!(restore.not_restored.is_empty());
    // The restore is recorded with its requester, channel and target.
    let rec = r.op(&restore.operation_id);
    assert_eq!(rec.state, OperationState::Finished);
    assert_eq!(rec.record.kind, OperationKind::Restore);
    assert_eq!(rec.record.requester, Requester::Unattributed);
    assert_eq!(rec.record.channel, Channel::Cli);
    assert_eq!(rec.record.target, Target::Snapshot(point.clone()));
    assert_eq!(
        rec.prior_snapshot.as_deref(),
        Some(restore.prior_snapshot_id.as_str())
    );
    assert_eq!(
        rec.record.scope.worktrees.first().map(String::as_str),
        Some(wt.to_str().unwrap())
    );
}

/// Escenario 2: lo cambiado después del punto vuelve; lo excluido (una
/// credencial y un repo anidado) y un worktree que no cambió, no se tocan.
#[test]
fn restore_reaches_what_changed_after_the_point_and_never_the_excluded() {
    let (fx, wt) = repo_with_login();
    let v2_tip = branch_ahead(&fx, "feat-login-v2", "feat-login");
    std::fs::write(wt.join("deploy.pem"), "-----BEGIN KEY-----\nv1\n").unwrap();
    let lib = wt.join("vendor/lib");
    std::fs::create_dir_all(&lib).unwrap();
    fx.git_in(&lib, &["init", "-q"]);
    std::fs::write(lib.join("lib.rs"), "pub fn lib() {}\n").unwrap();
    fx.git_in(&lib, &["add", "lib.rs"]);
    fx.git_in(&lib, &["commit", "-q", "-m", "lib"]);
    fx.git(&["branch", "feat-pagos"]);
    let pagos = canonical(&fx.add_worktree("feat-pagos", "feat-pagos"));
    std::fs::write(pagos.join("pagos.rs"), "fn cobrar() {}\n").unwrap();
    let r = start(fx, false);

    // After the point: feat-login-v2 deleted from feat-login, and a commit there.
    let first = r.operation(
        &wt,
        script("branch-delete", &[&["branch", "-D", "feat-login-v2"]])
            .declaring(&[], &["refs/heads/feat-login-v2"]),
    );
    let point = first.prior_snapshot_id.clone();
    assert!(!branch_exists(&r.fx, "feat-login-v2"));
    std::fs::write(wt.join("a.rs"), "fn a() { despues(); }\n").unwrap();
    r.operation(&wt, commit("commit", "despues"));
    // The excluded change after the point, outside any operation.
    std::fs::write(wt.join("deploy.pem"), "-----BEGIN KEY-----\nv2\n").unwrap();
    std::fs::write(lib.join("extra.rs"), "pub fn extra() {}\n").unwrap();
    let pagos_state = state(&r.fx, &pagos);
    let pagos_tip = tip(&r.fx, "feat-pagos");
    let before = r.fx.fingerprint();

    let restore = r.restore_ok(&wt, &point);

    // The branch deleted after the point is back, at its commit of the point.
    assert!(branch_exists(&r.fx, "feat-login-v2"));
    assert_eq!(tip(&r.fx, "feat-login-v2"), v2_tip);
    assert!(names(&restore.refs).contains(&"feat-login-v2".to_owned()));
    // The credential and the nested repo: same bytes, same inode.
    let after = r.fx.fingerprint();
    for excluded in [Path::new("deploy.pem"), Path::new("vendor/lib")] {
        let was = entries_under(&before, "wt-feat-login", excluded);
        assert!(!was.is_empty(), "{excluded:?}");
        assert_eq!(
            entries_under(&after, "wt-feat-login", excluded),
            was,
            "{excluded:?}"
        );
    }
    assert_eq!(
        std::fs::read(wt.join("deploy.pem")).unwrap(),
        b"-----BEGIN KEY-----\nv2\n"
    );
    assert!(lib.join("extra.rs").exists());
    // feat-pagos did not change: not a byte, not its branch.
    let changes: Vec<_> = diff(&before, &after)
        .into_iter()
        .filter(|c| c.scope == "wt-feat-pagos")
        .collect();
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(state(&r.fx, &pagos), pagos_state);
    assert_eq!(tip(&r.fx, "feat-pagos"), pagos_tip);
    assert!(!paths(&restore.worktrees).contains(&pagos));
}

/// Escenario 3: una restauración se deshace y vuelve al estado exacto de
/// antes, sin commitear, preparado y sin seguimiento incluidos.
#[test]
fn a_restore_can_be_undone_back_to_the_exact_prior_state() {
    let (fx, wt) = repo_with_login();
    branch_ahead(&fx, "feat-login-v2", "feat-login");
    let r = start(fx, false);
    let first = r.operation(
        &wt,
        script("branch-delete", &[&["branch", "-D", "feat-login-v2"]])
            .declaring(&[], &["refs/heads/feat-login-v2"]),
    );
    let point = first.prior_snapshot_id.clone();
    std::fs::write(wt.join("a.rs"), "fn a() { despues(); }\n").unwrap();
    r.operation(&wt, commit("commit", "despues"));
    // The state S right before the restore.
    std::fs::write(wt.join("a.rs"), "fn a() { sin_commitear(); }\n").unwrap();
    std::fs::write(wt.join("b.txt"), "beta preparado\n").unwrap();
    r.fx.git_in(&wt, &["add", "b.txt"]);
    std::fs::write(wt.join("sin_seguimiento.rs"), "fn s() {}\n").unwrap();
    let s_state = state(&r.fx, &wt);
    let s_branches = branches(&r.fx);
    assert!(!s_branches.contains_key("refs/heads/feat-login-v2"));

    let restore = r.restore_ok(&wt, &point);
    assert!(branch_exists(&r.fx, "feat-login-v2"));
    assert_ne!(state(&r.fx, &wt), s_state);

    let undo = r.undo_ok(&wt);

    assert_eq!(undo.undone_operation_id, restore.operation_id);
    assert_eq!(undo.target_snapshot_id, restore.prior_snapshot_id);
    assert_eq!(state(&r.fx, &wt), s_state);
    // The refs of the scope are back too: feat-login-v2 is gone again.
    assert_eq!(branches(&r.fx), s_branches);
    assert!(undo.not_restored.is_empty());
}

/// Escenario 4: un punto pendiente, sin ref en el almacén o descartado no se
/// restaura y nada cambia; un id desconocido no existe y no se registra.
#[test]
fn an_incomplete_point_is_not_restored_and_nothing_changes() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { intacto(); }\n").unwrap();
    let r = start(fx, false);
    let real = r.operation(&wt, reset_hard());
    let keys = r
        .oplog
        .lock()
        .unwrap()
        .snapshot(&real.prior_snapshot_id)
        .unwrap()
        .unwrap()
        .record
        .worktrees;
    let new = NewSnapshot {
        level: SnapshotLevel::GuaranteedPrior,
        worktrees: keys,
        engine_mark: None,
        cause_operation: None,
        cause_event_seq: None,
    };
    let (pending, no_ref, discarded) = {
        let mut log = r.oplog.lock().unwrap();
        let pending = log.begin_snapshot(&new, now_ms()).unwrap();
        let no_ref = log.begin_snapshot(&new, now_ms()).unwrap();
        log.complete_snapshot(&no_ref, &CompleteInfo::default(), now_ms())
            .unwrap();
        let discarded = log.begin_snapshot(&new, now_ms()).unwrap();
        log.set_snapshot_state(&discarded, SnapshotState::Discarded, now_ms())
            .unwrap();
        (pending, no_ref, discarded)
    };

    for (case, id) in [
        ("pending", &pending),
        ("complete without a ref in the store", &no_ref),
        ("discarded", &discarded),
    ] {
        let before = r.fx.fingerprint();
        let refusal = reject_reason(r.restore(&wt, id));
        assert_eq!(refusal.reason, TmRejectReason::TargetUnavailable, "{case}");
        let changes = diff(&before, &r.fx.fingerprint());
        assert!(changes.is_empty(), "{case}: {changes:#?}");
        assert_recorded_rejected(&r, &refusal, id);
    }

    // An id this repo never had: not found, and nothing recorded.
    let ops = r.operation_count();
    let before = r.fx.fingerprint();
    let (code, _) = rejected(r.restore(&wt, "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5b"));
    assert_eq!(code, code::NOT_FOUND);
    assert_eq!(r.operation_count(), ops);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}

/// Escenario 5 (sin US-TMC-013): restaurar sobre un commit de claude-1
/// posterior al punto pide una confirmación que todavía no existe, así que
/// se rechaza y el repo no cambia. "Lo confirma" queda pendiente de
/// US-TMC-013.
#[test]
fn restoring_over_an_agents_work_is_refused_until_confirmation_exists() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { mio(); }\n").unwrap();
    let r = start(fx, false);
    let first = r.operation(&wt, commit("commit", "mio"));
    let point = first.prior_snapshot_id.clone();
    std::fs::write(wt.join("a.rs"), "fn a() { del_agente(); }\n").unwrap();
    let second = r.operation(&wt, commit("commit", "agente"));
    // claude-1's work after the point, as the executor records it.
    r.inject(
        &NewOperation {
            kind: OperationKind::Protected,
            subtype: Some("commit".into()),
            requester: Requester::Agent {
                name: "claude-1".into(),
                origin: RequesterOrigin::Detected,
                session_id: "4242:1".into(),
            },
            channel: Channel::Mcp,
            ..unattributed(&wt, &[])
        },
        &second.prior_snapshot_id,
        OperationTransition::Finished,
    );
    let before = r.fx.fingerprint();

    let refusal = reject_reason(r.restore(&wt, &point));

    assert_eq!(refusal.reason, TmRejectReason::ConfirmationRequired);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    assert_recorded_rejected(&r, &refusal, &point);
}

/// Requisito técnico: un worktree del punto que se borró después se recrea
/// sin checkout, con su HEAD y sus archivos del punto.
#[test]
fn a_deleted_worktree_is_recreated_without_checkout() {
    let (fx, wt) = repo_with_login();
    let x = add_feat_x(&fx);
    std::fs::write(x.join("a.rs"), "fn a() { en_x(); }\n").unwrap();
    let x_at_point = state(&fx, &x);
    let r = start(fx, false);
    let first = r.operation(
        &wt,
        script(
            "worktree-remove",
            &[&["worktree", "remove", "--force", x.to_str().unwrap()]],
        )
        .declaring(&[&x], &[]),
    );
    let point = first.prior_snapshot_id.clone();
    assert!(!x.exists());

    let restore = r.restore_ok(&wt, &point);

    assert!(x.exists());
    assert_eq!(
        r.fx.git_in(&x, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim(),
        "feat-x"
    );
    let now = state(&r.fx, &x);
    assert_eq!(now.files, x_at_point.files);
    assert_eq!(now.head, x_at_point.head);
    assert_eq!(now.branch, x_at_point.branch);
    assert_eq!(paths(&restore.recreated), std::slice::from_ref(&x));
    assert_eq!(paths(&restore.worktrees).first(), Some(&wt));
    assert!(!paths(&restore.worktrees).contains(&x));
}

/// BR-TMC-CONS-001: si el punto previo a la restauración falla, la
/// restauración no se ejecuta.
#[test]
fn a_failed_prior_means_no_restore() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { perdido(); }\n").unwrap();
    let r = start(fx, true);
    let first = r.operation(&wt, reset_hard());
    let before = r.fx.fingerprint();

    let (code, data) = rejected(r.restore(&wt, &first.prior_snapshot_id));

    assert_eq!(code, code::PRIOR_SNAPSHOT_FAILED);
    let data: PriorFailedData = serde_json::from_value(data.unwrap()).unwrap();
    assert_eq!(data.reason, PriorFailure::NoSpace);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    let rec = r.op(&data.operation_id.unwrap());
    assert_eq!(rec.state, OperationState::Aborted);
    assert_eq!(rec.record.kind, OperationKind::Restore);
}

/// NFR-01: deshacer una restauración que recreó un worktree termina
/// siempre. La rama que solo tiene sacada el worktree recreado se queda como
/// está (con un aviso) en vez de rechazar el undo con `ref-in-use`; el
/// worktree pedido y las demás ramas vuelven al estado exacto de antes.
#[test]
fn undoing_a_restore_that_recreated_a_worktree_always_completes() {
    let (fx, wt) = repo_with_login();
    let x = add_feat_x(&fx);
    let x_tip_at_point = tip(&fx, "feat-x");
    let r = start(fx, false);
    // After the point: a commit in feat-x, then feat-x is removed.
    let first = r.operation(
        &wt,
        script(
            "worktree-remove",
            &[
                &["-C", x.to_str().unwrap(), "add", "-A"],
                &["-C", x.to_str().unwrap(), "commit", "-q", "-m", "x"],
                &["worktree", "remove", "--force", x.to_str().unwrap()],
            ],
        )
        .declaring(&[&x], &["refs/heads/feat-x"]),
    );
    let point = first.prior_snapshot_id.clone();
    assert!(!x.exists());
    assert_ne!(tip(&r.fx, "feat-x"), x_tip_at_point);
    std::fs::write(wt.join("a.rs"), "fn a() { despues(); }\n").unwrap();
    r.operation(&wt, commit("commit", "despues"));
    // The exact state before the restore: tree and refs.
    std::fs::write(wt.join("a.rs"), "fn a() { sin_commitear(); }\n").unwrap();
    let s_state = state(&r.fx, &wt);
    let s_branches = branches(&r.fx);

    let restore = r.restore_ok(&wt, &point);
    assert_eq!(paths(&restore.recreated), std::slice::from_ref(&x));
    assert_eq!(tip(&r.fx, "feat-x"), x_tip_at_point);
    let x_key = r.key_in(&point, &x);
    let warning = recreated_worktree_warning(&x_key);
    assert!(
        r.op(&restore.operation_id)
            .record
            .warnings
            .contains(&warning),
        "{warning}"
    );
    let x_restored = state(&r.fx, &x);

    let undo = r.undo_ok(&wt);

    assert_eq!(undo.undone_operation_id, restore.operation_id);
    assert_eq!(undo.target_snapshot_id, restore.prior_snapshot_id);
    // feat-login exactly as before the restore: tree and refs.
    assert_eq!(state(&r.fx, &wt), s_state);
    let now = branches(&r.fx);
    assert_eq!(
        now.keys().collect::<Vec<_>>(),
        s_branches.keys().collect::<Vec<_>>()
    );
    for (name, oid) in &s_branches {
        if name != "refs/heads/feat-x" {
            assert_eq!(now.get(name), Some(oid), "{name}");
        }
    }
    // The recreated worktree and its branch stay, and the undo says so.
    assert_eq!(tip(&r.fx, "feat-x"), x_tip_at_point);
    assert!(x.exists());
    assert_eq!(state(&r.fx, &x), x_restored);
    let kept = |undo: &UndoResult| {
        undo.warnings
            .iter()
            .chain(r.op(&undo.operation_id).record.warnings.iter())
            .any(|w| w.starts_with(KEPT_REF_IN_RECREATED_WORKTREE))
    };
    assert!(kept(&undo), "{:?}", undo.warnings);

    // --- An interrupted restore that recreated the worktree --------------------
    // No in-process way exists to interrupt the applier through the daemon:
    // the restore is written as the recovery leaves a cut one (`interrupted`
    // after its prior, the recreated worktree in its warnings), on top of the
    // state the real one started from.
    let half_way = r.inject(
        &NewOperation {
            target: Target::Snapshot(point.clone()),
            warnings: vec![warning.clone()],
            ..unattributed(&wt, &["refs/heads/feat-login", "refs/heads/feat-x"])
        },
        &restore.prior_snapshot_id,
        OperationTransition::Interrupted,
    );
    std::fs::write(wt.join("a.rs"), "fn a() { a_medias(); }\n").unwrap();

    let undo = r.undo_ok(&wt);

    assert_eq!(undo.undone_operation_id, half_way);
    assert_eq!(undo.target_snapshot_id, restore.prior_snapshot_id);
    assert_eq!(state(&r.fx, &wt), s_state);
    assert_eq!(tip(&r.fx, "feat-x"), x_tip_at_point);
    assert!(x.exists());
    assert!(kept(&undo), "{:?}", undo.warnings);
}

/// A8: un worktree del plan que Git sigue registrando pero que ya no está
/// en el disco se rechaza antes del snapshot previo.
///
/// Continuous capture is off, so the count of points only moves if the
/// restore takes its prior.
#[test]
fn a_registered_worktree_missing_on_disk_is_refused_before_the_prior() {
    let (fx, wt) = repo_with_login();
    let x = add_feat_x(&fx);
    let no_capture = CaptureConfig {
        enabled: false,
        ..CaptureConfig::default()
    };
    let r = start_with(fx, false, no_capture);
    let first = r.operation(
        &wt,
        script(
            "commit",
            &[&[
                "-C",
                x.to_str().unwrap(),
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                "x",
            ]],
        )
        .declaring(&[&x], &["refs/heads/feat-x"]),
    );
    let point = first.prior_snapshot_id.clone();
    std::fs::remove_dir_all(&x).unwrap();
    let listed = r.fx.git(&["worktree", "list", "--porcelain"]);
    assert!(listed.contains(x.to_str().unwrap()), "{listed}");
    let before = r.fx.fingerprint();
    let snapshots = |r: &Running| r.store().snapshot_ids().unwrap().unwrap().len();
    let points = snapshots(&r);

    let refusal = reject_reason(r.restore(&wt, &point));

    assert_eq!(refusal.reason, TmRejectReason::WorktreeUnavailable);
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(snapshots(&r), points);
    assert_recorded_rejected(&r, &refusal, &point);
}

/// D3: las ramas creadas después del punto no se borran; el resultado las
/// nombra como dejadas en su sitio.
#[test]
fn branches_created_after_the_point_are_kept_and_reported() {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("a.rs"), "fn a() { del_punto(); }\n").unwrap();
    let at_point = state(&fx, &wt);
    let r = start(fx, false);
    let first = r.operation(&wt, commit("commit", "uno"));
    let point = first.prior_snapshot_id.clone();
    r.operation(
        &wt,
        script("branch", &[&["branch", "later"]]).declaring(&[], &["refs/heads/later"]),
    );
    r.fx.git(&["branch", "later-raw"]);
    let later = tip(&r.fx, "later");
    let later_raw = tip(&r.fx, "later-raw");

    let restore = r.restore_ok(&wt, &point);

    assert_eq!(state(&r.fx, &wt), at_point);
    assert_eq!(tip(&r.fx, "later"), later);
    assert_eq!(tip(&r.fx, "later-raw"), later_raw);
    let kept = names(&restore.kept_branches);
    assert!(kept.contains(&"later".to_owned()), "{kept:?}");
    assert!(kept.contains(&"later-raw".to_owned()), "{kept:?}");
    for at_the_point in ["main", "feat-login"] {
        assert!(!kept.contains(&at_the_point.to_owned()), "{kept:?}");
    }
    assert!(!names(&restore.refs).contains(&"later".to_owned()));
}

/// Limitación de D1 a la vista: una rama del punto borrada desde otro
/// worktree no vuelve, y el resultado la nombra.
#[test]
fn a_branch_deleted_from_another_worktree_is_named_as_not_returned() {
    let (fx, wt) = repo_with_login();
    fx.git(&["branch", "feat-old"]);
    std::fs::write(wt.join("a.rs"), "fn a() { del_punto(); }\n").unwrap();
    let at_point = state(&fx, &wt);
    let r = start(fx, false);
    let first = r.operation(&wt, commit("commit", "uno"));
    let point = first.prior_snapshot_id.clone();
    // Deleted from the main worktree, not from feat-login.
    let main = r.fx.repo.clone();
    r.operation(
        &main,
        script("branch-delete", &[&["branch", "-D", "feat-old"]])
            .declaring(&[], &["refs/heads/feat-old"]),
    );
    assert!(!branch_exists(&r.fx, "feat-old"));

    let restore = r.restore_ok(&wt, &point);

    assert_eq!(state(&r.fx, &wt), at_point);
    assert!(!branch_exists(&r.fx, "feat-old"));
    let not_returned = names(&restore.not_returned_branches);
    assert_eq!(not_returned, ["feat-old"]);
    assert!(!names(&restore.refs).contains(&"feat-old".to_owned()));
    assert!(!names(&restore.kept_branches).contains(&"feat-old".to_owned()));
}
