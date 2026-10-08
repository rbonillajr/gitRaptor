//! `raptor restore` end to end (US-TMC-009): the real `raptor` binary, run
//! from a subfolder of the worktree, returns it to a point of the timeline
//! and `raptor undo` takes the restore back; a rejection is explained in
//! English and Spanish and changes nothing.
//!
//! The daemon runs in this test process with the test catalog of
//! `undo_process.rs`, which is the only way to record a real Git command as
//! an operation of the oplog. Temporary repo, worktrees and profile only
//! (NFR-01).
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::timemachine::{OperationRunResult, RestoreResult};
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

/// Message fragments of `apps/cli/i18n/{en,es}/timemachine.txt` that the
/// subcommand must produce.
const EN_POINT: &str = "raptor restore ";
const EN_DONE: &str = "restored ";
const EN_UNDO_HINT: &str = "raptor undo";
const EN_CONFIRMATION: [&str; 2] = ["belongs to an agent", "ask the agent to restore it itself"];
const ES_CONFIRMATION: [&str; 2] = ["es de un agente", "pídele al agente que restaure él mismo"];
const EN_NOT_FOUND: &str = "no restore point";
const ES_NOT_FOUND: &str = "no hay un punto de restauración";
const EN_INVALID_ID: &str = "not a valid restore point id";
const ES_INVALID_ID: &str = "no es un id de punto de restauración válido";

/// A well-formed id that no snapshot of the repo has.
const UNKNOWN_ID: &str = "00000000-0000-4000-8000-000000000000";

/// `timeline` shows the id of the restore point; `restore P` from a
/// subfolder brings the worktree back to it (with its uncommitted work) and
/// says `raptor undo` takes it back; `--json` is a `RestoreResult`; and
/// `raptor undo` returns to the exact state before each restore.
#[test]
fn raptor_restore_returns_to_a_timeline_point_and_undo_takes_it_back() {
    let m = machine();
    let login = m.worktree.join("src/login.rs");
    let work = "fn login() { validar_token(); }\n";
    std::fs::write(&login, work).unwrap();
    std::fs::write(m.worktree.join("src/nuevo.rs"), "fn nuevo() {}\n").unwrap();
    // The reset saves that work as its prior snapshot: the restore point.
    let op = m.reset_hard();
    let point = op.prior_snapshot_id.clone();
    assert_eq!(std::fs::read_to_string(&login).unwrap(), "fn login() {}\n");
    let after_reset = m.fx.fingerprint();

    let timeline = m.raptor(&m.worktree.join("src"), &["timeline"], "en_US.UTF-8");
    assert!(timeline.status.success(), "{}", text(&timeline.stderr));
    let shown = text(&timeline.stdout);
    assert!(
        shown
            .lines()
            .any(|l| l.contains(&format!("{EN_POINT}{point}"))),
        "{shown}"
    );

    let out = m.raptor(&m.worktree.join("src"), &["restore", &point], "en_US.UTF-8");
    assert!(
        out.status.success(),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&login).unwrap(), work);
    let stdout = text(&out.stdout);
    assert!(stdout.contains(EN_DONE), "{stdout}");
    assert!(stdout.contains(EN_UNDO_HINT), "{stdout}");

    let undone = m.raptor(&m.worktree, &["undo"], "en_US.UTF-8");
    assert!(
        undone.status.success(),
        "{}{}",
        text(&undone.stdout),
        text(&undone.stderr)
    );
    let changes = diff(&after_reset, &m.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");

    let json = m.raptor(&m.worktree, &["restore", &point, "--json"], "en_US.UTF-8");
    assert!(json.status.success(), "{}", text(&json.stderr));
    let result: RestoreResult = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(result.target_snapshot_id, point);
    assert_eq!(std::fs::read_to_string(&login).unwrap(), work);

    let undone = m.raptor(&m.worktree, &["undo"], "en_US.UTF-8");
    assert!(undone.status.success(), "{}", text(&undone.stderr));
    let changes = diff(&after_reset, &m.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// A rejection says why, in English and Spanish, exits with an error and
/// leaves the repo as it was.
#[test]
fn raptor_restore_explains_rejections_in_english_and_spanish() {
    let m = machine();
    std::fs::write(
        m.worktree.join("src/login.rs"),
        "fn login() { del_agente(); }\n",
    )
    .unwrap();
    let real = m.reset_hard();
    let point = real.prior_snapshot_id.clone();
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

    let cases: [(&str, &str, &[&str], &[&str]); 3] = [
        (&point, "agent", &EN_CONFIRMATION, &ES_CONFIRMATION),
        (UNKNOWN_ID, "unknown", &[EN_NOT_FOUND], &[ES_NOT_FOUND]),
        ("../etc", "malformed", &[EN_INVALID_ID], &[ES_INVALID_ID]),
    ];
    for (id, what, en, es) in cases {
        let english = m.raptor(&m.worktree, &["restore", id], "en_US.UTF-8");
        assert!(
            !english.status.success(),
            "{what}: {}",
            text(&english.stdout)
        );
        let stderr = text(&english.stderr);
        for fragment in en {
            assert!(stderr.contains(fragment), "{what}: {stderr}");
        }
        let spanish = m.raptor(&m.worktree, &["restore", id], "es_ES.UTF-8");
        assert!(
            !spanish.status.success(),
            "{what}: {}",
            text(&spanish.stdout)
        );
        let stderr = text(&spanish.stderr);
        for fragment in es {
            assert!(stderr.contains(fragment), "{what}: {stderr}");
        }
        let changes = diff(&before, &m.fx.fingerprint());
        assert!(changes.is_empty(), "{what}: {changes:#?}");
    }
}
