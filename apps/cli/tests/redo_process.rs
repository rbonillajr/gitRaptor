//! US-TMC-003 at process level: `raptor redo` with the real `raptor` binary against a daemon over a
//! temporary profile: done in English and Spanish (naming the undo it redid and the point it saved
//! before), `--json` with the `RedoResult`, and the refusals with exit code 1 and the repo intact.
//!
//! The daemon runs in this test process (the `raptor` child descends from it, so it is
//! "unattributed"); the child runs with `env_clear()` and a temporary profile and `HOME`: no
//! `CLAUDE_*` variable of the session running the tests reaches it. The operation to undo is a real
//! `git reset --hard` of a test catalog over a staged `b.rs`, which the reset deletes. Temporary
//! repo, worktrees and profile only (NFR-01).
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de validación
//! multiplataforma.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::oplog::{OperationFilter, OperationKind, Oplog};
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
    dirs: ProfileDirs,
    oplog: Arc<std::sync::Mutex<Oplog>>,
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

/// A repo with `a.rs` committed and a staged `b.rs`, a linked worktree `feat-login` observed by a
/// fresh profile, and the daemon serving.
fn machine() -> Machine {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.write("a.rs", "fn a() {}\n");
    fx.git(&["add", "a.rs"]);
    fx.git(&["commit", "-q", "-m", "a"]);
    fx.git(&["branch", "feat-login"]);
    let worktree = canonical(&fx.add_worktree("feat-login", "feat-login"));
    std::fs::write(worktree.join("b.rs"), B_RS).unwrap();
    fx.git_in(&worktree, &["add", "b.rs"]);
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
        dirs: dirs.clone(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig {
            agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
            ..ChannelConfig::default()
        },
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
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Machine {
        fx,
        _tmp: tmp,
        profile,
        dirs,
        oplog,
        worktree,
        handle,
        join: Some(join),
    }
}

impl Machine {
    /// `raptor <args>` in the worktree, in language `lang`, without a terminal.
    /// `raptor <args>`. A `raptor redo` or `raptor undo` refused only for the requester's write
    /// burst (S-03) is asked again within 30 s, as `restore_process.rs` does; any other answer
    /// returns at once. Authorized amendment of TMC003-E2E (coordinator, 2026-11-09): the English
    /// and Spanish runs share one unattributed requester (6 writes against a burst of 5).
    fn raptor(&self, args: &[&str], lang: &str) -> Output {
        let start = std::time::Instant::now();
        loop {
            let out = self.raptor_once(args, lang);
            let limited = ["too many requests", "demasiadas peticiones"]
                .iter()
                .any(|t| text(&out.stderr).contains(t) || text(&out.stdout).contains(t));
            if !(limited && matches!(args.first(), Some(&"redo") | Some(&"undo"))) {
                return out;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(30),
                "TMC003-E2E: raptor {args:?} still rate limited after 30 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    fn raptor_once(&self, args: &[&str], lang: &str) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", &self.profile)
            .env("HOME", &self.fx.home)
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", lang)
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// An unattributed operation: `git reset --hard` as a protected operation (it deletes the
    /// staged `b.rs`), asked over the channel.
    fn reset_hard(&self) {
        let mut c = Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
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
        c.call::<_, Value>(
            methods::OPERATION_RUN,
            json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
        )
        .unwrap();
    }

    /// Stages `b.rs` again and runs the operation that deletes it, then undoes it with `raptor
    /// undo --json`: the undo as the CLI printed it.
    fn operation_and_undo(&self) -> Value {
        std::fs::write(self.worktree.join("b.rs"), B_RS).unwrap();
        self.fx.git_in(&self.worktree, &["add", "b.rs"]);
        self.reset_hard();
        assert!(
            !self.worktree.join("b.rs").exists(),
            "the operation deleted b.rs"
        );
        let out = self.raptor(&["undo", "--json"], "en_US.UTF-8");
        assert!(
            out.status.success(),
            "the undo failed: {}{}",
            text(&out.stdout),
            text(&out.stderr)
        );
        assert!(
            self.worktree.join("b.rs").exists(),
            "the undo recovered b.rs"
        );
        serde_json::from_slice(&out.stdout).expect("the undo's JSON")
    }

    /// The point the last redo of the oplog took before itself.
    fn last_redo_prior(&self) -> String {
        let log = self.oplog.lock().unwrap();
        let mut redos: Vec<_> = log
            .operations(&OperationFilter::default())
            .unwrap()
            .into_iter()
            .filter(|v| v.record.kind == OperationKind::Redo)
            .collect();
        redos.sort_by_key(|v| v.record.seq);
        redos
            .pop()
            .expect("a redo in the oplog")
            .prior_snapshot
            .expect("the redo has its prior point")
    }
}

const B_RS: &str = "fn b() {}\n";

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

fn all_text(out: &Output) -> String {
    format!("{}{}", text(&out.stdout), text(&out.stderr))
}

/// Escenario 1 at the terminal: the redo is done, in English and Spanish, and says which undo it
/// redid and where the state right before it was saved.
#[test]
fn redo_is_done_in_english_and_spanish() {
    let m = machine();
    for (lang, lead, saved) in [
        ("en_US.UTF-8", "", ""),
        (
            "es_ES.UTF-8",
            "rehecho el undo ",
            "el estado justo antes de este redo se guardó como ",
        ),
    ] {
        let undo = m.operation_and_undo();
        let undo_id = undo["operation_id"]
            .as_str()
            .expect("the undo id")
            .to_owned();
        assert!(
            m.worktree.join("b.rs").exists(),
            "b.rs is back before the redo"
        );

        let out = m.raptor(&["redo"], lang);

        assert!(
            out.status.success(),
            "TMC003-E2E ({lang}): the redo is done with exit code 0: {}",
            all_text(&out)
        );
        assert!(
            !m.worktree.join("b.rs").exists(),
            "TMC003-E2E ({lang}): the redo deleted b.rs again"
        );
        let shown = all_text(&out);
        let prior = m.last_redo_prior();
        assert!(
            shown.contains(&undo_id),
            "TMC003-E2E ({lang}): the text names the undo it redid ({undo_id}): {shown}"
        );
        assert!(
            shown.contains(&prior),
            "TMC003-E2E ({lang}): the text names the point saved before the redo ({prior}): {shown}"
        );
        if !lead.is_empty() {
            assert!(
                shown.contains(&format!("{lead}{undo_id}")),
                "TMC003-E2E ({lang}): redo.done: {shown}"
            );
            assert!(
                shown.contains(&format!("{saved}{prior}")),
                "TMC003-E2E ({lang}): redo.prior: {shown}"
            );
        }
    }
}

/// `--json` prints the `RedoResult`: the redo as recorded, its prior point, the undo it redid and
/// the point it went to.
#[test]
fn redo_json_prints_the_result() {
    let m = machine();
    let undo = m.operation_and_undo();

    let out = m.raptor(&["redo", "--json"], "en_US.UTF-8");

    assert!(
        out.status.success(),
        "TMC003-E2E: the redo --json is done: {}",
        all_text(&out)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "TMC003-E2E: stdout is the JSON result ({e}): {}",
            all_text(&out)
        )
    });
    assert_eq!(
        result["redone_undo_id"], undo["operation_id"],
        "TMC003-E2E: {result}"
    );
    assert_eq!(
        result["target_snapshot_id"], undo["prior_snapshot_id"],
        "TMC003-E2E: the redo goes to the point the undo took before itself: {result}"
    );
    for key in [
        "operation_id",
        "prior_snapshot_id",
        "written",
        "removed",
        "requester",
    ] {
        assert!(result.get(key).is_some(), "TMC003-E2E: {key} in {result}");
    }
    assert_eq!(result["prior_snapshot_id"], m.last_redo_prior());
    assert!(!m.worktree.join("b.rs").exists());
}

/// Escenario 3 at the terminal: no undo pending is told with exit code 1 and nothing changes.
#[test]
fn nothing_to_redo_exits_with_1_and_changes_nothing() {
    let m = machine();
    m.reset_hard();
    let before = m.fx.fingerprint();

    for (lang, said) in [
        ("en_US.UTF-8", "nothing changed"),
        ("es_ES.UTF-8", "nada cambió"),
    ] {
        let out = m.raptor(&["redo"], lang);

        assert_eq!(
            out.status.code(),
            Some(1),
            "TMC003-E2E ({lang}): a rejected redo exits with 1: {}",
            all_text(&out)
        );
        let shown = all_text(&out);
        assert!(
            shown.contains(said),
            "TMC003-E2E ({lang}): nothing-to-redo says `{said}`: {shown}"
        );
        let changes = diff(&before, &m.fx.fingerprint());
        assert!(changes.is_empty(), "TMC003-E2E ({lang}): {changes:#?}");
    }
}

/// Escenario 4 at the terminal: a later operation invalidated the redo; exit code 1, nothing changes.
#[test]
fn an_operation_after_the_undo_invalidates_the_redo() {
    let m = machine();
    m.operation_and_undo();
    std::fs::write(m.worktree.join("c.rs"), "fn c() {}\n").unwrap();
    m.fx.git_in(&m.worktree, &["add", "c.rs"]);
    m.reset_hard();
    let before = m.fx.fingerprint();

    for (lang, said) in [
        ("en_US.UTF-8", "nothing changed"),
        ("es_ES.UTF-8", "nada cambió"),
    ] {
        let out = m.raptor(&["redo"], lang);

        assert_eq!(
            out.status.code(),
            Some(1),
            "TMC003-E2E ({lang}): the invalidated redo exits with 1: {}",
            all_text(&out)
        );
        assert!(
            all_text(&out).contains(said),
            "TMC003-E2E ({lang}): redo-invalidated says `{said}`: {}",
            all_text(&out)
        );
        let changes = diff(&before, &m.fx.fingerprint());
        assert!(changes.is_empty(), "TMC003-E2E ({lang}): {changes:#?}");
    }
}
