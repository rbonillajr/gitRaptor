//! US-TMC-010 at process level: `raptor undo --since` says why it did nothing, in English and
//! Spanish, with the real `raptor` binary. A bad period and an empty one change nothing.
//!
//! The daemon runs in this test process (the `raptor` child descends from it, so it is
//! "unattributed"); the child runs with `env_clear()` and a temporary profile and `HOME`: no
//! `CLAUDE_*` variable of the session running the tests reaches it. Temporary repo, worktrees and
//! profile only (NFR-01).
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de validación
//! multiplataforma.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::{Fixture, diff};
use serde_json::json;

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
            governed: None,
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
    let (_entry, _) = store
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
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    })
    .unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Machine {
        fx,
        _tmp: tmp,
        profile,
        worktree,
        handle,
        join: Some(join),
    }
}

impl Machine {
    /// `raptor <args>` in `dir`, in language `lang`, without a terminal.
    fn raptor(&self, dir: &Path, args: &[&str], lang: &str) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", &self.profile)
            .env("HOME", &self.fx.home)
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", lang)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

/// Escenario 3: a bad period is explained in Spanish, fails and changes nothing.
#[test]
fn undo_since_invalid_says_why_in_es() {
    let m = machine();
    std::fs::write(m.worktree.join("src/login.rs"), "fn login() { wip(); }\n").unwrap();
    let before = m.fx.fingerprint();

    let out = m.raptor(&m.worktree, &["undo", "--since", "veinte"], "es_ES.UTF-8");

    assert!(
        !out.status.success(),
        "TMC010-E2E-INVALID: a bad period must fail: {}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains(
            "--since debe ser una duración como 20m, 2h o 1d, de hasta 30 días; nada cambió"
        ),
        "TMC010-E2E-INVALID: undo.invalid-since in Spanish: {stderr}"
    );
    let changes = diff(&before, &m.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// Escenario 4: nothing in the period is told with the period, in English and in Spanish.
#[test]
fn undo_since_with_nothing_says_so_in_en_and_es() {
    let m = machine();
    std::fs::write(m.worktree.join("src/login.rs"), "fn login() { wip(); }\n").unwrap();
    let before = m.fx.fingerprint();

    for (lang, expected) in [
        (
            "en_US.UTF-8",
            "nothing to undo in this worktree in the last 5m; nothing changed",
        ),
        (
            "es_ES.UTF-8",
            "no hay operaciones que deshacer en este worktree en los últimos 5m; nada cambió",
        ),
    ] {
        let out = m.raptor(&m.worktree, &["undo", "--since", "5m"], lang);

        assert!(
            !out.status.success(),
            "TMC010-E2E-NOTHING ({lang}): nothing to undo exits with failure"
        );
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains(expected),
            "TMC010-E2E-NOTHING ({lang}): undo.reason.nothing-in-period with the period: {stderr}"
        );
        let changes = diff(&before, &m.fx.fingerprint());
        assert!(changes.is_empty(), "{lang}: {changes:#?}");
    }
}
