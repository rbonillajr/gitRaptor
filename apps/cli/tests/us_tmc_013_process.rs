//! US-TMC-013 at process level: who may undo whose work, and the confirmation of the requester
//! that is "unattributed", with the real `raptor` binary.
//!
//! The daemon runs in this test process with a test catalog (the only way to record a real Git
//! command as an operation of the oplog), the simulated agent is a copy of this test binary named
//! `raptor-fake-agent` that runs, in series, the commands it is given (`fake_agent_entry`), and
//! the MCP and full-connection clients are this same binary (`rpc_client_entry`). The developer
//! answers on a pty made with `script`. The confirmation eligibility is the debug-only seam of
//! the channel: every client here descends from the daemon, so none would pass the real checks.
//! Temporary repo, worktrees and profile only (NFR-01).
//!
//! macOS and Linux. Linux: Pendiente: etapa de validación multiplataforma (options of `script`,
//! working folder of a peer).
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::OperationRunResult;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig, TestConfirmation};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationFilter, OperationKind, OperationState, OperationTransition,
    OperationView, Oplog, Requester, RequesterOrigin, Scope, Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// A process name the daemon does not know as an agent.
const UNKNOWN_AGENT: &str = "codex";
const SEQ_ENV: &str = "RAPTOR_FAKE_AGENT_SEQ";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";
/// The session of the work of "claude-2", injected into the oplog.
const CLAUDE_2: &str = "4242:1";

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

// ---- the simulated agent and the clients that run under it ------------------------------------

/// The simulated agent: runs, in series, the commands of `RAPTOR_FAKE_AGENT_SEQ` (a JSON list of
/// `{"argv": [..], "rpc": <spec or null>}`) as its children, and exits with the status of the last.
#[test]
fn fake_agent_entry() {
    let Some(seq) = std::env::var_os(SEQ_ENV) else {
        return;
    };
    let seq: Vec<Value> = serde_json::from_str(seq.to_str().unwrap()).unwrap();
    let mut last = 0;
    for step in seq {
        let argv: Vec<String> = serde_json::from_value(step["argv"].clone()).unwrap();
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).env_remove(SEQ_ENV);
        if !step["rpc"].is_null() {
            cmd.env(RPC_ENV, step["rpc"].to_string());
        }
        last = cmd.status().unwrap().code().unwrap_or(1);
    }
    std::process::exit(last);
}

/// The client: `{"root", "kind", "reset"?, "calls"}`. `reset` (a worktree) runs the test
/// catalog's reset through `operation.prepare` and `operation.run`. Prints the answers as one
/// JSON line; an error keeps its code and `data`.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let kind: ClientKind = serde_json::from_value(spec["kind"].clone()).unwrap();
    let mut client = Client::connect(&dirs, kind, PROTOCOL_VERSION).unwrap();
    let answer = |r: Result<Value, ClientError>| match r {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let mut answers = Vec::new();
    if let Some(worktree) = spec["reset"].as_str() {
        let plan = client.call::<_, PrepareResult>(
            methods::OPERATION_PREPARE,
            json!({ "operation": "abort-in-progress", "worktree": worktree, "surface": "tui" }),
        );
        answers.push(match plan {
            Ok(plan) => answer(client.call(
                methods::OPERATION_RUN,
                json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
            )),
            Err(e) => answer(Err(e)),
        });
    }
    for call in spec["calls"].as_array().into_iter().flatten() {
        answers.push(answer(
            client.call(call[0].as_str().unwrap(), call[1].clone()),
        ));
    }
    println!("RPC-ANSWERS {}", serde_json::to_string(&answers).unwrap());
}

// ---- the test catalog -------------------------------------------------------------------------

/// `abort-in-progress` stands for an operation that runs `git reset --hard` in the worktree.
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

// ---- the machine ------------------------------------------------------------------------------

struct Machine {
    fx: Arc<Fixture>,
    tmp: tempfile::TempDir,
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

/// A repo with `src/login.rs` committed, a linked worktree `feat-login` observed and enabled for
/// MCP in a fresh profile, and the daemon serving; `raptor-fake-agent` is its only known agent.
fn machine(test_confirmation: Option<TestConfirmation>) -> Machine {
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
    store
        .set_mcp_enabled(&entry.repo_id, true, "test", 1)
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
        channel: ChannelConfig {
            agents: AgentMatcher::only(vec![FAKE_AGENT.into()]),
            test_confirmation,
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
        tm_capture: Default::default(),
    })
    .unwrap();
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Machine {
        fx,
        tmp,
        profile,
        worktree,
        oplog,
        handle,
        join: Some(join),
    }
}

/// The environment of every `raptor` process: a temporary profile and home, a fixed language.
fn raptor_env(m: &Machine, lang: &str) -> Vec<(&'static str, std::ffi::OsString)> {
    vec![
        ("GITRAPTOR_PROFILE_DIR", m.profile.clone().into_os_string()),
        ("HOME", m.fx.home.clone().into_os_string()),
        ("PATH", "/usr/bin:/bin".into()),
        ("LANG", lang.into()),
    ]
}

/// What the daemon did with each request of the undo family, oldest first.
fn undos(m: &Machine) -> Vec<OperationView> {
    let filter = OperationFilter {
        kind: Some(OperationKind::Undo),
        ..OperationFilter::default()
    };
    let mut all = m.oplog.lock().unwrap().operations(&filter).unwrap();
    all.sort_by_key(|o| o.record.seq);
    all
}

/// Single quotes for a shell word (only for the pty command line of Linux's `script -c`).
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

impl Machine {
    fn client(&self) -> Client {
        let dirs = ProfileDirs::under_root(&self.profile);
        let start = Instant::now();
        loop {
            match Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::yield_now();
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// The reset, as an operation of the oplog, by an unattributed requester.
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

    /// Work of the agent `session` on top of the stack: an operation written into the oplog as the
    /// executor would record it, with the prior snapshot of a real one, so that undoing it
    /// brings the worktree back to that state.
    fn inject_agent_work(&self, session: &str, prior_snapshot_id: &str) -> String {
        let mut log = self.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![self.worktree.to_string_lossy().into_owned()],
                        refs: Vec::new(),
                    },
                    requester: Requester::Agent {
                        name: "claude-code".into(),
                        origin: RequesterOrigin::Detected,
                        session_id: session.into(),
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
                snapshot_id: prior_snapshot_id,
            },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, 2).unwrap();
        }
        id
    }

    /// The common scene: uncommitted work thrown away by a real reset, and an agent's operation
    /// of session `CLAUDE_2` on top that would bring it back. Returns the work, the agent's
    /// operation id and the prior snapshot of the real reset.
    fn agent_work_on_top(&self) -> (&'static str, String, String) {
        let work = "fn login() { del_agente(); }\n";
        std::fs::write(self.worktree.join("src/login.rs"), work).unwrap();
        let real = self.reset_hard();
        let id = self.inject_agent_work(CLAUDE_2, &real.prior_snapshot_id);
        (work, id, real.prior_snapshot_id)
    }

    fn login(&self) -> String {
        std::fs::read_to_string(self.worktree.join("src/login.rs")).unwrap()
    }

    /// `raptor <args>` in the worktree, without a terminal.
    fn raptor(&self, args: &[&str], lang: &str) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(raptor_env(self, lang))
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `raptor <args>` on a pty, as from the developer's own terminal; `answer` is what they type.
    /// Whatever the command prints, on stdout or stderr, comes back in `stdout`.
    fn on_terminal(&self, args: &[&str], lang: &str, answer: Option<&str>) -> Output {
        use std::io::Write;
        let mut cmd = Command::new("/usr/bin/script");
        if cfg!(target_os = "macos") {
            cmd.args(["-q", "/dev/null", RAPTOR]).args(args);
        } else {
            let line: Vec<String> = std::iter::once(RAPTOR)
                .chain(args.iter().copied())
                .map(quoted)
                .collect();
            cmd.args(["-qec", &line.join(" "), "/dev/null"]);
        }
        let mut child = cmd
            .env_clear()
            .envs(raptor_env(self, lang))
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The watchdog only fires if the command hangs: the test waits for the process itself.
        let (done, waiting) = channel::<()>();
        let pid = child.id();
        std::thread::spawn(move || {
            if matches!(
                waiting.recv_timeout(Duration::from_secs(90)),
                Err(RecvTimeoutError::Timeout)
            ) {
                let _ = Command::new("/bin/kill")
                    .args(["-9", &pid.to_string()])
                    .status();
            }
        });
        // Kept open until the command ends: closing it would close the pty under it.
        let mut typed = child.stdin.take();
        if let (Some(input), Some(answer)) = (typed.as_mut(), answer) {
            input.write_all(format!("{answer}\n").as_bytes()).unwrap();
            input.flush().unwrap();
        }
        let out = child.wait_with_output().unwrap();
        drop(typed);
        drop(done);
        out
    }

    /// `commands` run in series by a process named `name`, as its children.
    fn under(&self, name: &str, commands: &[Value]) -> Output {
        let me = std::env::current_exe().unwrap();
        let agent = self.tmp.path().join(name);
        if !agent.exists() {
            std::fs::copy(&me, &agent).unwrap();
        }
        Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(raptor_env(self, "en_US.UTF-8"))
            .env(SEQ_ENV, serde_json::to_string(commands).unwrap())
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A step of `under`: this test binary as a client of the engine.
    fn rpc_step(&self, kind: ClientKind, reset: bool, calls: Value) -> Value {
        let me = std::env::current_exe().unwrap();
        let mut spec = json!({ "root": self.profile, "kind": kind, "calls": calls });
        if reset {
            spec["reset"] = json!(self.worktree);
        }
        json!({
            "argv": [
                me.to_string_lossy(), "rpc_client_entry", "--exact", "--nocapture",
                "--test-threads=1",
            ],
            "rpc": spec,
        })
    }

    /// A step of `under`: the real `raptor`.
    fn raptor_step(&self, args: &[&str]) -> Value {
        let mut argv = vec![RAPTOR];
        argv.extend_from_slice(args);
        json!({ "argv": argv, "rpc": null })
    }
}

/// The answers each client printed, in order (one list per client).
fn answers(out: &Output) -> Vec<Vec<Value>> {
    text(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once("RPC-ANSWERS ").map(|(_, j)| j.to_owned()))
        .map(|j| serde_json::from_str(&j).unwrap())
        .collect()
}

fn reason_of(answer: &Value) -> Option<&str> {
    answer["data"]["reason"].as_str()
}

fn unchanged(m: &Machine, before: &gitraptor_testkit::fingerprint::Snapshot) {
    let changes = diff(before, &m.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

// ---- scenario 1 -------------------------------------------------------------------------------

/// Under the same simulated agent, one child resets through the catalog and the next undoes it over
/// MCP: it is allowed, and the undo's record names that agent's session.
#[test]
fn s1_an_agent_undoes_its_own_operation_over_mcp() {
    let m = machine(None);
    let work = "fn login() { propio(); }\n";
    std::fs::write(m.worktree.join("src/login.rs"), work).unwrap();

    let out = m.under(
        FAKE_AGENT,
        &[
            m.rpc_step(ClientKind::Cli, true, json!([])),
            m.rpc_step(ClientKind::Mcp, false, json!([[methods::TM_UNDO, {}]])),
        ],
    );

    let answers = answers(&out);
    assert_eq!(
        answers.len(),
        2,
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    assert!(answers[0][0]["ok"].is_object(), "{}", answers[0][0]);
    assert!(answers[1][0]["ok"].is_object(), "{}", answers[1][0]);
    assert_eq!(m.login(), work);

    let reset = m
        .oplog
        .lock()
        .unwrap()
        .operations(&OperationFilter {
            kind: Some(OperationKind::Protected),
            ..OperationFilter::default()
        })
        .unwrap();
    let undone = undos(&m);
    assert_eq!(undone.len(), 1, "{undone:#?}");
    let undo = &undone[0].record;
    assert_eq!(undone[0].state, OperationState::Finished);
    assert_eq!(undo.channel, Channel::Mcp);
    let session = undo.requester.session_id().expect("an agent");
    assert_eq!(reset[0].record.requester.session_id(), Some(session));
}

// ---- scenario 2 -------------------------------------------------------------------------------

/// Another agent's work, over MCP from a different session: rejected with its reason, repo intact,
/// and the rejection is in the oplog with the agent that asked.
#[test]
fn s2_claude1_over_mcp_cannot_undo_claude2s_work() {
    let m = machine(None);
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    let out = m.under(
        FAKE_AGENT,
        &[m.rpc_step(ClientKind::Mcp, false, json!([[methods::TM_UNDO, {}]]))],
    );

    let answer = &answers(&out)[0][0];
    assert_eq!(answer["code"], code::OPERATION_REJECTED, "{answer}");
    assert_eq!(reason_of(answer), Some("other-actor"), "{answer}");
    unchanged(&m, &before);
    let undone = undos(&m);
    assert_eq!(undone.len(), 1, "{undone:#?}");
    assert_eq!(undone[0].state, OperationState::Rejected);
    let asker = undone[0].record.requester.session_id().expect("an agent");
    assert_ne!(asker, CLAUDE_2);
}

/// The same agent from its own shell, with the real `raptor`.
#[test]
fn s2_claude1_from_its_own_shell_cannot_undo_claude2s_work() {
    let m = machine(None);
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    let out = m.under(FAKE_AGENT, &[m.raptor_step(&["undo"])]);

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("belongs to another actor"), "{stderr}");
    unchanged(&m, &before);
}

/// An agent the daemon does not recognize (it is not the registered claude): over MCP it is
/// "unattributed", which MCP never serves; if it were recognized, `other-actor`. Either way,
/// nothing changes.
#[test]
fn s2_codex1_over_mcp_cannot_undo_claude2s_work() {
    let m = machine(None);
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    let out = m.under(
        UNKNOWN_AGENT,
        &[m.rpc_step(ClientKind::Mcp, false, json!([[methods::TM_UNDO, {}]]))],
    );

    let answer = &answers(&out)[0][0];
    let scope_refused = answer["code"] == code::SCOPE_REFUSED;
    let other_actor =
        answer["code"] == code::OPERATION_REJECTED && reason_of(answer) == Some("other-actor");
    assert!(scope_refused || other_actor, "{answer}");
    unchanged(&m, &before);
}

// ---- scenario 3 -------------------------------------------------------------------------------

/// The developer, on a terminal, sees whose work it is, answers yes and the agent's work is undone;
/// the oplog keeps the first request as rejected and the undo as confirmed, by an unattributed
/// requester.
#[test]
fn s3_the_developer_confirms_on_a_terminal_and_the_agents_work_is_undone() {
    let m = machine(Some(TestConfirmation::Eligible));
    let (work, agent_op, _) = m.agent_work_on_top();
    assert_eq!(m.login(), "fn login() {}\n");

    let out = m.on_terminal(&["undo"], "en_US.UTF-8", Some("y"));

    let shown = text(&out.stdout);
    assert!(out.status.success(), "{shown}{}", text(&out.stderr));
    assert!(shown.contains("this undoes reset-hard"), "{shown}");
    assert!(shown.contains(&agent_op), "{shown}");
    assert!(shown.contains("[y/N]"), "{shown}");
    assert_eq!(m.login(), work);

    let undone = undos(&m);
    assert_eq!(undone.len(), 2, "{undone:#?}");
    assert_eq!(undone[0].state, OperationState::Rejected);
    assert!(!undone[0].record.confirmed);
    assert_eq!(undone[1].state, OperationState::Finished);
    assert!(undone[1].record.confirmed);
    assert_eq!(undone[1].record.requester, Requester::Unattributed);
    assert_eq!(undone[1].record.channel, Channel::Cli);
}

// ---- scenario 4 -------------------------------------------------------------------------------

/// Without a terminal there is nobody to ask: nothing changes and one rejection is recorded. With
/// the real checks (here a descendant of the daemon), the text says the engine could not verify
/// the terminal, and no question is asked either.
#[test]
fn s4_without_a_terminal_there_is_no_undo() {
    let m = machine(Some(TestConfirmation::Eligible));
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    let out = m.raptor(&["undo"], "en_US.UTF-8");

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("needs a terminal"), "{stderr}");
    assert!(!stderr.contains("[y/N]"), "{stderr}");
    unchanged(&m, &before);
    let undone = undos(&m);
    assert_eq!(undone.len(), 1, "{undone:#?}");
    assert_eq!(undone[0].state, OperationState::Rejected);

    let natural = machine(None);
    natural.agent_work_on_top();
    let before = natural.fx.fingerprint();
    let out = natural.on_terminal(&["undo"], "en_US.UTF-8", None);
    let shown = text(&out.stdout);
    assert!(!out.status.success(), "{shown}");
    assert!(shown.contains("belongs to an agent"), "{shown}");
    assert!(shown.contains("could not verify"), "{shown}");
    assert!(!shown.contains("[y/N]"), "{shown}");
    unchanged(&natural, &before);
}

/// Answering no leaves everything as it was.
#[test]
fn s4_declining_changes_nothing() {
    let m = machine(Some(TestConfirmation::Eligible));
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    let out = m.on_terminal(&["undo"], "en_US.UTF-8", Some("n"));

    let shown = text(&out.stdout);
    assert!(!out.status.success(), "{shown}");
    assert!(shown.contains("not confirmed; nothing changed"), "{shown}");
    unchanged(&m, &before);
    let undone = undos(&m);
    assert_eq!(undone.len(), 1, "{undone:#?}");
    assert_eq!(undone[0].state, OperationState::Rejected);
}

// ---- scenario 5 -------------------------------------------------------------------------------

/// Where the rule does not allow confirming another actor's work (Windows), simulated with the
/// seam: the CLI does not ask and says so, in English and in Spanish.
#[test]
fn s5_on_windows_the_cli_explains_without_asking() {
    let m = machine(Some(TestConfirmation::RuleForbids));
    m.agent_work_on_top();
    let before = m.fx.fingerprint();

    for (lang, expected) in [
        (
            "en_US.UTF-8",
            "on Windows confirming another actor's work is not available yet",
        ),
        (
            "es_ES.UTF-8",
            "en Windows todavía no se puede confirmar trabajo de otro actor",
        ),
    ] {
        let out = m.on_terminal(&["undo"], lang, None);
        let shown = text(&out.stdout);
        assert!(!out.status.success(), "{shown}");
        assert!(shown.contains(expected), "{lang}: {shown}");
        assert!(!shown.contains("[y/N]"), "{lang}: {shown}");
        unchanged(&m, &before);
    }
}

// ---- restore ----------------------------------------------------------------------------------

/// Like scenario 3 with `raptor restore <id>`: the developer confirms on a terminal and the
/// agent's work is taken back.
#[test]
fn restore_s3_the_developer_confirms_a_restore_on_a_terminal() {
    let m = machine(Some(TestConfirmation::Eligible));
    let (work, _, point) = m.agent_work_on_top();
    assert_eq!(m.login(), "fn login() {}\n");

    let out = m.on_terminal(&["restore", &point], "en_US.UTF-8", Some("y"));

    let shown = text(&out.stdout);
    assert!(out.status.success(), "{shown}{}", text(&out.stderr));
    assert!(shown.contains("[y/N]"), "{shown}");
    assert_eq!(m.login(), work);
}
