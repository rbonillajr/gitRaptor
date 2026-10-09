//! #223 I-02 and I-03 (NFR-01, NFR-02, ADR-MCP-001) end to end: `timemachine.undo`,
//! `timemachine.restore` and `timemachine.timeline` never open a `.git` the repo does not own.
//! Asked from a linked worktree whose `.git` was rewritten to point at another observed repo,
//! from a folder the repo does not register, or from a worktree whose `.git` is a symlink to
//! outside, each one is refused with the frozen `scope-refused/not-observed` before anything is
//! recorded or written, and both repos stay intact (INF-GRP-001 fingerprint). The same worktree,
//! once its `.git` is the repo's own again, is undone as usual.
//!
//! A real daemon (in-process) with a test catalog, a real client, real Git, testkit fixtures and a
//! temporary profile; never this repo or the real profile (NFR-01). macOS and Linux, like the
//! other channel tests.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_api::timemachine::{OperationRunResult, UndoResult};
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
use gitraptor_core::observe::locate;
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Scope as FpScope, Snapshot, diff};
use serde_json::{Value, json};

const TIMELINE: &str = "timemachine.timeline";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    gitraptor_core::observe::canonical(p)
}

// ----- Test catalog: a real `git reset --hard` as the operation to undo -------------

struct Catalog {
    fx: Arc<Fixture>,
}

struct Step {
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
        Ok(Box::new(Step {
            fx: Arc::clone(&self.fx),
            cwd: plan.repo.worktree.clone(),
        }))
    }
}

impl ProtectedStep for Step {
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
        if !status.success() {
            return Err(StepError::new("git reset failed"));
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

// ----- Running daemon over two observed repos --------------------------------------------

struct Running {
    ours: Arc<Fixture>,
    theirs: Fixture,
    tp: TempProfile,
    /// Our registered linked worktree, as Orca creates them.
    a: PathBuf,
    /// Their linked worktree, with a file only they have.
    x: PathBuf,
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

/// Both repos observed: the one an attacker points at is one the daemon would act on too.
fn start() -> Running {
    let ours = Fixture::with_commit(&git());
    ours.git(&["branch", "a"]);
    let a = canonical(&ours.add_worktree("a", "a"));
    let theirs = Fixture::with_commit(&git());
    theirs.git(&["branch", "x"]);
    let x = canonical(&theirs.add_worktree("x", "x"));
    std::fs::write(x.join("secret.txt"), "theirs\n").unwrap();

    let ours = Arc::new(ours);
    let tp = TempProfile::new();
    let mut profile = tp.open();
    for repo in [&ours.repo, &theirs.repo] {
        profile
            .add_repo(&canonical(&repo.join(".git")), None, 1)
            .unwrap();
    }
    drop(profile);
    let wiring = OperationsWiring {
        catalog: Arc::new(Catalog {
            fx: Arc::clone(&ours),
        }) as Arc<dyn OperationCatalog>,
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
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        ours,
        theirs,
        tp,
        a,
        x,
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

impl Running {
    /// A `git reset --hard` of uncommitted work in `wt`, through the executor: the operation the
    /// attacks then try to undo or restore around. Its prior snapshot is the point to restore.
    fn reset_uncommitted(&self, wt: &Path) -> OperationRunResult {
        std::fs::write(wt.join("a.txt"), "work in progress\n").unwrap();
        let mut c = connect(&self.tp);
        let plan: PrepareResult = c
            .call(
                methods::OPERATION_PREPARE,
                json!({
                    "operation": "abort-in-progress",
                    "worktree": wt.to_str().unwrap(),
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

    fn undo(&self, wt: &Path) -> Result<Value, ClientError> {
        connect(&self.tp).call(
            methods::TM_UNDO,
            json!({ "worktree": wt.to_str().unwrap(), "surface": "cli" }),
        )
    }

    fn restore(&self, wt: &Path, snapshot_id: &str) -> Result<Value, ClientError> {
        connect(&self.tp).call(
            methods::TM_RESTORE,
            json!({ "worktree": wt.to_str().unwrap(), "snapshot_id": snapshot_id, "surface": "cli" }),
        )
    }

    fn timeline(&self, wt: &Path) -> Result<Value, ClientError> {
        connect(&self.tp).call(TIMELINE, json!({ "worktree": wt.to_str().unwrap() }))
    }

    fn theirs_admin(&self) -> PathBuf {
        locate(&self.theirs.repo)
            .unwrap()
            .join("worktrees")
            .join("wt-x")
    }

    fn fingerprint(&self) -> Snapshot {
        let scopes = [
            FpScope::new("ours", self.ours.repo.clone()),
            FpScope::new("ours-a", self.a.clone()),
            FpScope::new("theirs", self.theirs.repo.clone()),
            FpScope::new("theirs-x", self.x.clone()),
        ];
        Snapshot::take(&scopes, &BTreeSet::new())
    }

    /// Undo, restore and the timeline asked from `wt` are refused as `not-observed`, and both
    /// repos are left as they were.
    fn assert_refused(&self, wt: &Path, point: &str) {
        // `git status` refreshes a stat-dirty index: the "before" is taken after it.
        for root in [&self.ours.repo, &self.a, &self.theirs.repo, &self.x] {
            let _ = self
                .ours
                .git_command(root, &["status", "--porcelain"])
                .output();
        }
        let before = self.fingerprint();
        for (what, result) in [
            ("undo", self.undo(wt)),
            ("restore", self.restore(wt, point)),
            ("timeline", self.timeline(wt)),
        ] {
            match result {
                Err(ClientError::Rpc(e)) => {
                    assert_eq!(e.code, code::SCOPE_REFUSED, "{what}: {e:?}");
                    let data: ScopeRefusedData = serde_json::from_value(e.data.unwrap()).unwrap();
                    assert_eq!(data.reason, ScopeRefusal::NotObserved, "{what}");
                }
                other => panic!("{what} was not refused: {other:?}"),
            }
        }
        let changes = diff(&before, &self.fingerprint());
        assert!(changes.is_empty(), "a repo changed: {changes:#?}");
    }
}

// ----- Scenarios ------------------------------------------------------------------------

#[test]
fn a_linked_git_rewritten_to_another_observed_repo_is_refused_and_undone_once_restored() {
    let r = start();
    let op = r.reset_uncommitted(&r.a);
    let link = std::fs::read(r.a.join(".git")).unwrap();
    std::fs::write(
        r.a.join(".git"),
        format!("gitdir: {}\n", r.theirs_admin().display()),
    )
    .unwrap();

    r.assert_refused(&r.a, &op.prior_snapshot_id);

    // The repo's own `.git` again: the legitimate linked worktree is undone as usual.
    std::fs::write(r.a.join(".git"), link).unwrap();
    let undo: UndoResult = serde_json::from_value(r.undo(&r.a).unwrap()).unwrap();
    assert_eq!(undo.undone_operation_id, op.operation_id);
    assert_eq!(
        std::fs::read(r.a.join("a.txt")).unwrap(),
        b"work in progress\n"
    );
}

#[test]
fn a_folder_the_repo_does_not_register_is_refused() {
    let r = start();
    let op = r.reset_uncommitted(&r.a);
    // A folder that claims our worktree's admin entry: the entry names `wt-a`, not it.
    let ghost = r.ours.root.join("ghost");
    std::fs::create_dir(&ghost).unwrap();
    let admin = locate(&r.ours.repo).unwrap().join("worktrees").join("wt-a");
    std::fs::write(ghost.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();

    r.assert_refused(&canonical(&ghost), &op.prior_snapshot_id);
}

#[test]
fn a_git_that_is_a_symlink_to_outside_is_refused() {
    let r = start();
    let op = r.reset_uncommitted(&r.a);
    std::fs::remove_file(r.a.join(".git")).unwrap();
    std::os::unix::fs::symlink(r.theirs_admin(), r.a.join(".git")).unwrap();

    r.assert_refused(&r.a, &op.prior_snapshot_id);
}
