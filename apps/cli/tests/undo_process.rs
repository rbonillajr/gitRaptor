//! M1 exit criterion 3 as a test (US-TMC-002): a real `git reset --hard`
//! throws away uncommitted work and the real `raptor` binary, run from a
//! subfolder of the worktree, brings it back with `raptor undo`.
//!
//! The daemon runs in this test process with a test catalog: production has
//! no catalog of operations yet, and a catalog is the only way to record a
//! real Git command as an operation of the oplog. The `raptor` client is a
//! child of the daemon's process, so the daemon resolves it as
//! "unattributed" (TS-TMC-004 § 4), like the in-process client that runs the
//! reset. This proves the mechanism; the criterion itself closes with real
//! use and raw Git in the stack (US-TMC-004). Temporary repo, worktrees and
//! profile only (NFR-01).
//!
//! macOS and Windows (the channel is a named pipe there, XP-01; the store
//! and the applier, XP-12). Linux: Pendiente: etapa de validación
//! multiplataforma.
#![cfg(any(target_os = "macos", windows))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::timemachine::{OperationRunResult, UndoResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationTransition, Oplog, Requester, RequesterOrigin,
    Scope, Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

/// The test catalog: `abort-in-progress` stands for an operation that runs
/// `git reset --hard` in the requested worktree.
struct ResetHard {
    fx: Arc<Fixture>,
}

struct ResetStep {
    fx: Arc<Fixture>,
    cwd: PathBuf,
}

impl OperationCatalog for ResetHard {
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
        Ok(Box::new(ResetStep {
            fx: Arc::clone(&self.fx),
            cwd: plan.repo.worktree.clone(),
        }))
    }
}

impl ProtectedStep for ResetStep {
    fn subtype(&self) -> &str {
        "reset-hard"
    }

    fn scope(&self) -> StepScope {
        StepScope::default()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        let mut cmd = self.fx.git_command(&self.cwd, &["reset", "-q", "--hard"]);
        cmd.stdout(Stdio::null());
        let child = ctx
            .spawn(&mut cmd)
            .map_err(|e| StepError::new(e.to_string()))?;
        let status = ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
        if status.success() {
            Ok(StepOutput::default())
        } else {
            Err(StepError::new("git reset --hard failed"))
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

struct Machine {
    fx: Arc<Fixture>,
    _tmp: tempfile::TempDir,
    profile: PathBuf,
    worktree: PathBuf,
    oplog: Arc<Mutex<Oplog>>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

/// A repo with `src/login.rs` committed, a linked worktree `feat-login`
/// observed by a fresh profile, and the daemon serving.
fn machine() -> Machine {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.write("src/login.rs", "fn login() {}\n");
    fx.git(&["add", "src/login.rs"]);
    fx.git(&["commit", "-q", "-m", "login"]);
    fx.git(&["branch", "feat-login"]);
    let worktree = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let fx = Arc::new(fx);
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let dirs = ProfileDirs::under_root(&profile);
    let mut store = Profile::open(dirs.clone()).unwrap().0;
    let (entry, _) = store
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(store);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let daemon = Daemon::start(DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: Some(OperationsWiring {
            catalog: Arc::new(ResetHard {
                fx: Arc::clone(&fx),
            }),
            gate: Arc::new(AllowAll),
            test_layer_override: Some(Layer::Cockpit),
            prior_deadline: Duration::from_secs(30),
            prior_layer: None,
        }),
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: Default::default(),
    })
    .unwrap();
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Machine {
        fx,
        _tmp: tmp,
        profile,
        worktree,
        oplog,
        handle,
        join: Some(join),
    }
}

impl Machine {
    fn client(&self) -> Client {
        let dirs = ProfileDirs::under_root(&self.profile);
        let start = Instant::now();
        loop {
            match Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// The reset, as an operation of the oplog.
    fn reset_hard(&self) -> OperationRunResult {
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

    /// `raptor <args>` in `dir`, in language `lang`, without a terminal.
    fn raptor(&self, dir: &Path, args: &[&str], lang: &str) -> Output {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", &self.profile)
            .env("HOME", &self.fx.home)
            .env("LANG", lang)
            .current_dir(dir)
            .stdin(Stdio::null());
        #[cfg(unix)]
        cmd.env("PATH", "/usr/bin:/bin");
        // Windows needs its system folder; nothing of the user's profile is passed.
        #[cfg(windows)]
        for key in ["SystemRoot", "PATH"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.output().unwrap()
    }
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

/// Criterion 3 of M1: `raptor undo` brings back the uncommitted work a real
/// `git reset --hard` threw away, from a subfolder of the worktree, in
/// English; the same undo, again, takes nothing else and says so in
/// Spanish.
#[test]
fn raptor_undo_recovers_uncommitted_work_after_a_real_reset_hard() {
    let m = machine();
    let login = m.worktree.join("src/login.rs");
    let work = "fn login() { validar_token(); }\n";
    std::fs::write(&login, work).unwrap();
    std::fs::write(m.worktree.join("src/nuevo.rs"), "fn nuevo() {}\n").unwrap();
    let op = m.reset_hard();
    assert_eq!(std::fs::read_to_string(&login).unwrap(), "fn login() {}\n");

    let out = m.raptor(&m.worktree.join("src"), &["undo"], "en_US.UTF-8");

    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&login).unwrap(), work);
    // `git reset --hard` keeps untracked files; the undo leaves them too.
    assert!(m.worktree.join("src/nuevo.rs").exists());
    let stdout = text(&out.stdout);
    assert!(stdout.contains("undone reset-hard"), "{stdout}");
    assert!(stdout.contains(&op.operation_id), "{stdout}");
    assert!(stdout.contains("nothing was lost"), "{stdout}");

    // Consecutive undos walk back (taking back the undo is redo, US-TMC-003):
    // nothing older is left, so the next one changes nothing, in Spanish.
    let before = m.fx.fingerprint();
    let again = m.raptor(&m.worktree, &["undo", "--json"], "es_ES.UTF-8");
    assert!(!again.status.success());
    let stderr = text(&again.stderr);
    assert!(stderr.contains("no hay nada que deshacer"), "{stderr}");
    assert!(diff(&before, &m.fx.fingerprint()).is_empty());
    assert_eq!(std::fs::read_to_string(&login).unwrap(), work);
}

/// `--json` prints the engine's answer as is.
#[test]
fn raptor_undo_json_prints_the_result() {
    let m = machine();
    std::fs::write(m.worktree.join("src/login.rs"), "fn login() { v2(); }\n").unwrap();
    let op = m.reset_hard();

    let out = m.raptor(&m.worktree, &["undo", "--json"], "en_US.UTF-8");

    assert!(out.status.success(), "{}", text(&out.stderr));
    let result: UndoResult = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result.undone_operation_id, op.operation_id);
    assert_eq!(result.target_snapshot_id, op.prior_snapshot_id);
}

/// With nothing to undo the repo does not change and the message says why,
/// in Spanish.
#[test]
fn raptor_undo_with_nothing_to_undo_changes_nothing() {
    let m = machine();
    std::fs::write(m.worktree.join("src/login.rs"), "fn login() { wip(); }\n").unwrap();
    let before = m.fx.fingerprint();

    let out = m.raptor(&m.worktree, &["undo"], "es_ES.UTF-8");

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("no hay nada que deshacer"), "{stderr}");
    let changes = diff(&before, &m.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// The limit accepted for M1 (PO, 2026-10-05): an unattributed requester
/// cannot undo an agent's work without the confirmation of US-TMC-013. The
/// agent's operation is written into the daemon's oplog as the executor
/// would record it, with the prior snapshot of a real one.
#[test]
fn an_agents_work_needs_a_confirmation_that_is_not_there_yet() {
    let m = machine();
    std::fs::write(
        m.worktree.join("src/login.rs"),
        "fn login() { del_agente(); }\n",
    )
    .unwrap();
    let real = m.reset_hard();
    {
        let mut log = m.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![m.worktree.to_string_lossy().into_owned()],
                        refs: Vec::new(),
                    },
                    requester: Requester::Agent {
                        name: "claude-code".into(),
                        origin: RequesterOrigin::Detected,
                        session_id: "4242:1".into(),
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
    }
    let before = m.fx.fingerprint();

    let out = m.raptor(&m.worktree, &["undo"], "en_US.UTF-8");

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("belongs to an agent"), "{stderr}");
    assert!(diff(&before, &m.fx.fingerprint()).is_empty());
}
