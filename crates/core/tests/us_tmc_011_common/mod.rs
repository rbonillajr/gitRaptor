//! Actor harness of the US-TMC-011 tests: a real daemon in this process (TM wiring of
//! `us_tmc_013_common`) that knows one agent executable, `raptor-fake-agent`, so the Claude Code
//! session that runs the suite is never an agent here. "claude-1" and "claude-2" are two
//! `raptor-fake-agent` shell processes that run `git` in the worktree (pattern of `us_grp_010.rs`):
//! the engine gives each one a session by process ancestry and attributes its commits to it.
//! Session ids are always read from `sessions.list`. Temporary repos and profile only (NFR-01);
//! every wait ends on an explicit signal (session list, timeline) with a deadline.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId};
use gitraptor_api::messages::{ClientKind, SessionStateView, SessionView, SessionsListResult};
use gitraptor_api::timemachine::{TimelineEntry, TimelineResult, UndoResult};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig, TestConfirmation};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
};
use gitraptor_core::executor::{
    GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts, StepPlan,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::oplog::{
    OperationFilter, OperationKind, OperationView, Oplog, SnapshotFilter,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepError,
};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

#[path = "../us_tmc_013_common/mod.rs"]
pub mod base013;
pub use base013::{canonical, confirm_data, git, params, rejected, token_of};

pub const ENV: &str = "RAPTOR_T_TMC011";
pub const DEADLINE: Duration = Duration::from_secs(40);
pub const AGENT_NAME: &str = "raptor-fake-agent";
const DONE: &str = "<<raptor-fake-agent-done>>";

// ----- Role of the process that calls from inside an agent ---------------------------------------

/// The process that asks the daemon from inside an agent's process tree: runs each `[method,
/// params]` of its spec and writes the answers (`{ok}`, `{code, data}` or `{other}`) to `out`. As a
/// normal test it does nothing.
#[test]
fn role_entry() {
    let Some(spec) = std::env::var_os(ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    if spec["role"] == "shell" {
        shell_role();
    }
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let answers: Vec<Value> = spec["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(
            |call| match client.call::<_, Value>(call[0].as_str().unwrap(), call[1].clone()) {
                Ok(v) => json!({ "ok": v }),
                Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
                Err(e) => json!({ "other": e.to_string() }),
            },
        )
        .collect();
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

/// The simulated agent: runs each line of its input with `/bin/sh` and prints [`DONE`] after it.
fn shell_role() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(&line).status();
        println!("{DONE} {}", status.map(|s| s.success()).unwrap_or(false));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

// ----- Test catalog: nothing to plan, the Time Machine's own commands need no catalog ---------------

struct NoCatalog;

impl OperationCatalog for NoCatalog {
    fn plan_op(
        &self,
        _operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        _facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        Err(PlanError::NotImplemented)
    }

    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Err(StepError::new("this catalog has no steps".to_owned()))
    }
}

struct AllowAll;

impl GuardrailsGate for AllowAll {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

// ----- The simulated agent -------------------------------------------------------------------------

pub struct Agent {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// Its session id, as `sessions.list` gave it.
    pub session: String,
}

impl Agent {
    pub fn run(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "({command}) 2>&1").unwrap();
        stdin.flush().unwrap();
        let mut output = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake agent died"
            );
            if let Some((before, ok)) = line.split_once(DONE) {
                output.push_str(before);
                assert_eq!(ok.trim(), "true", "{command} failed: {output}");
                return;
            }
            output.push_str(&line);
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ----- The world -----------------------------------------------------------------------------------

struct Booted {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    oplog: Arc<Mutex<Oplog>>,
}

/// A repo `main` with `a.rs`, `api.rs`, `README.md` and `docs.md` committed, its worktree
/// `feat-login` (the one every request is made from), and the daemon over them.
pub struct World {
    // Last field to drop is irrelevant; the guard serializes the worlds of one test binary: the
    // engine samples `git` while it lives, and daemons competing for the CPU miss the sample.
    _serial: std::sync::MutexGuard<'static, ()>,
    pub fx: Fixture,
    pub tp: TempProfile,
    pub repo_id: String,
    pub worktree: PathBuf,
    bin: PathBuf,
    hooks: PathBuf,
    confirmation: Option<TestConfirmation>,
    booted: Option<Booted>,
    out_n: std::cell::Cell<u32>,
}

pub fn world() -> World {
    boot_world(true)
}

/// A world whose engine store cannot be opened, so the daemon has no attribution to give.
pub fn world_without_engine() -> World {
    boot_world(false)
}

static SERIAL: Mutex<()> = Mutex::new(());

fn boot_world(engine: bool) -> World {
    let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::with_commit(&git());
    for (file, content) in [
        ("a.rs", "fn a() {}\n"),
        ("api.rs", "pub fn api() {}\n"),
        ("README.md", "# demo\n"),
        ("docs.md", "docs\n"),
        (".env", "TOKEN=old\n"),
    ] {
        fx.write(file, content);
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "base"]);
    fx.git(&["branch", "feat-login"]);
    let worktree = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let tp = TempProfile::new();
    let (entry, _) = tp
        .open()
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    if !engine {
        let repos = tp.dirs().repos_dir();
        std::fs::create_dir_all(&repos).unwrap();
        std::fs::write(
            repos.join(format!("{}.sqlite", entry.repo_id)),
            b"this is not a database",
        )
        .unwrap();
    }
    let bin = tp.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let hooks = tp.root.path().join("slow-hooks");
    std::fs::create_dir(&hooks).unwrap();
    // A slow `post-commit`, so `git` is alive when the engine samples it and attributes the commit.
    let post_commit = hooks.join("post-commit");
    std::fs::write(&post_commit, "#!/bin/sh\nsleep 2\n").unwrap();
    std::fs::set_permissions(&post_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
    gitraptor_testkit::fixture::clone_binary(
        &std::env::current_exe().unwrap(),
        &bin.join(AGENT_NAME),
    );
    let mut w = World {
        _serial: serial,
        fx,
        tp,
        repo_id: entry.repo_id,
        worktree,
        bin,
        hooks,
        confirmation: Some(TestConfirmation::Eligible),
        booted: None,
        out_n: std::cell::Cell::new(0),
    };
    w.boot();
    if engine {
        w.warm_up();
    }
    w
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop_daemon();
    }
}

impl World {
    fn boot(&mut self) {
        let wiring = OperationsWiring {
            catalog: Arc::new(NoCatalog),
            gate: Arc::new(AllowAll),
            test_layer_override: Some(Layer::Cockpit),
            prior_deadline: Duration::from_secs(30),
            prior_layer: None,
        };
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let daemon = Daemon::start(DaemonConfig {
            dirs: self.tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: ChannelConfig {
                agents: AgentMatcher::only(vec![AGENT_NAME.into()]),
                test_confirmation: self.confirmation,
                ..ChannelConfig::default()
            },
            protected: None,
            operations: Some(wiring),
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            power: Default::default(),
            predict: Default::default(),
            // A capture of its own would add points the repo-intact checks do not expect.
            tm_capture: TmCapture {
                config: CaptureConfig {
                    enabled: false,
                    ..CaptureConfig::default()
                },
                ..Default::default()
            },
        })
        .unwrap();
        let oplog = daemon.oplog(&self.repo_id).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        self.booted = Some(Booted {
            handle,
            join: Some(join),
            oplog,
        });
    }

    /// Waits until the engine watches the repo: an empty commit in the main checkout (not the
    /// worktree the tests use) shows up as a Git event of the timeline.
    fn warm_up(&self) {
        self.fx
            .git(&["commit", "--allow-empty", "-q", "-m", "warm-up"]);
        let start = Instant::now();
        loop {
            let shown: TimelineResult = self
                .client()
                .call(
                    methods::TM_TIMELINE,
                    json!({"worktree": self.fx.repo, "since": "1h"}),
                )
                .unwrap();
            if shown.entries.iter().any(|e| e.id.starts_with("event:")) {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "the engine never showed the warm-up commit"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Stops the daemon (the engine with it) and waits for it. Nothing observes the repo until
    /// [`World::restart_daemon`].
    pub fn stop_daemon(&mut self) {
        if let Some(mut b) = self.booted.take()
            && let Some(join) = b.join.take()
        {
            b.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }

    /// A new daemon over the same profile: what happened meanwhile is a gap for the engine.
    pub fn restart_daemon(&mut self) {
        self.stop_daemon();
        self.boot();
    }

    pub fn oplog(&self) -> Arc<Mutex<Oplog>> {
        Arc::clone(&self.booted.as_ref().expect("a running daemon").oplog)
    }

    pub fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    // ----- sessions and agents

    pub fn sessions(&self) -> Vec<SessionView> {
        let r: SessionsListResult = self
            .client()
            .call(
                methods::SESSIONS_LIST,
                json!({"repo_id": self.repo_id, "include_ended": true}),
            )
            .unwrap();
        r.sessions
            .into_iter()
            .filter(|s| Path::new(s.worktree.raw()) == self.worktree)
            .collect()
    }

    /// A new simulated Claude Code working in the worktree; returns when `sessions.list` shows
    /// its session as a detected Claude Code one (its owner is asserted here, never assumed).
    pub fn spawn_agent(&self) -> Agent {
        let known: Vec<String> = self.sessions().into_iter().map(|s| s.session_id).collect();
        let spec = json!({"role": "shell", "root": self.tp.root.path().join("profile")});
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(self.bin.join(AGENT_NAME))
            .args([
                "us_tmc_011_common::role_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ENV, spec.to_string())
            .env("HOME", &self.fx.home)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", exe.display()));
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let start = Instant::now();
        let session = loop {
            let found = self.sessions().into_iter().find(|s| {
                !known.contains(&s.session_id)
                    && s.state != SessionStateView::Ended
                    && matches!(
                        &s.actor,
                        Actor::Agent {
                            kind: AgentKind::ClaudeCode,
                            origin: AgentOrigin::Detected,
                            ..
                        }
                    )
            });
            if let Some(s) = found {
                break s.session_id;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "the engine never showed the fake agent as a Claude Code session: {:#?}",
                self.sessions()
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        Agent {
            child,
            stdin,
            stdout,
            session,
        }
    }

    /// Waits for a present detected Claude Code session and returns its id (after a restart the
    /// engine shows the process again, maybe as a new session).
    pub fn present_agent_session(&self) -> String {
        let start = Instant::now();
        loop {
            let found = self.sessions().into_iter().find(|s| {
                s.state != SessionStateView::Ended
                    && matches!(
                        &s.actor,
                        Actor::Agent {
                            kind: AgentKind::ClaudeCode,
                            origin: AgentOrigin::Detected,
                            ..
                        }
                    )
            });
            if let Some(s) = found {
                return s.session_id;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no present Claude Code session: {:#?}",
                self.sessions()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The session `session_id` as `sessions.list` shows it now.
    pub fn session(&self, session_id: &str) -> SessionView {
        self.sessions()
            .into_iter()
            .find(|s| s.session_id == session_id)
            .unwrap_or_else(|| panic!("no session {session_id}"))
    }

    /// "codex-1": registers another agent in the worktree through the channel, as the developer.
    pub fn register_other(&self, name: &str) {
        let answer = self.as_developer(
            methods::REGISTRATION_REGISTER,
            json!({"agent": {"kind": "other", "name": name}, "worktree": self.worktree}),
        );
        assert!(answer.get("ok").is_some(), "register {name}: {answer}");
    }

    /// `attribution.correct` of `session_id` to the other agent `name`.
    pub fn correct(&self, session_id: &str, name: &str) -> Value {
        self.as_developer(
            methods::ATTRIBUTION_CORRECT,
            json!({"agent": {"kind": "other", "name": name}, "worktree": self.worktree,
                   "session_id": session_id}),
        )
    }

    /// `attribution.withdraw-correction` of `session_id`.
    pub fn withdraw(&self, session_id: &str) -> Value {
        self.as_developer(
            methods::ATTRIBUTION_WITHDRAW,
            json!({"worktree": self.worktree, "session_id": session_id}),
        )
    }

    fn out_file(&self) -> PathBuf {
        let n = self.out_n.get();
        self.out_n.set(n + 1);
        self.tp.root.path().join(format!("answers-{n}.json"))
    }

    fn calls_spec(&self, calls: &[(&str, Value)], out: &Path) -> Value {
        json!({
            "role": "calls",
            "root": self.tp.root.path().join("profile"),
            "calls": calls.iter().map(|(m, p)| json!([m, p])).collect::<Vec<_>>(),
            "out": out,
        })
    }

    /// A reserved call by the developer: the test binary under `script`, so it has a controlling
    /// terminal and the engine takes it for the developer.
    pub fn as_developer(&self, method: &str, params: Value) -> Value {
        let out = self.out_file();
        let spec = self.calls_spec(&[(method, params)], &out);
        let exe = std::env::current_exe().unwrap();
        let entry = [
            "us_tmc_011_common::role_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ];
        let line = std::iter::once(exe.to_string_lossy().into_owned())
            .chain(entry.iter().map(|s| (*s).to_owned()))
            .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let under_tty = if cfg!(target_os = "macos") {
            format!("/usr/bin/script -q /dev/null {line}")
        } else {
            format!(
                "script -q -e -c \"{}\" /dev/null",
                line.replace('"', "\\\"")
            )
        };
        // The in-process daemon would see a child of this test as its own descendant and refuse a
        // reserved command: the client is started detached (double fork), so it is an orphan with
        // a terminal of its own, as a developer's shell would give it.
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("( {under_tty} </dev/null >/dev/null 2>&1 & )"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.fx.home)
            .env(ENV, spec.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        let start = Instant::now();
        while !out.exists() {
            assert!(
                start.elapsed() < DEADLINE,
                "the developer's client ({status}) left no answers"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut answers: Vec<Value> =
            serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        answers.remove(0)
    }

    /// A call from inside `agent`'s process tree: the requester is that agent's session. Returns
    /// `{ok}` or `{code, data}`.
    pub fn as_agent(&self, agent: &mut Agent, method: &str, params: Value) -> Value {
        let out = self.out_file();
        let spec = self.calls_spec(&[(method, params)], &out);
        let exe = std::env::current_exe().unwrap();
        let quoted = spec.to_string().replace('\'', r"'\''");
        agent.run(&format!(
            "{ENV}='{quoted}' '{}' us_tmc_011_common::role_entry --exact --nocapture --test-threads=1",
            exe.display()
        ));
        assert!(out.exists(), "the agent's client left no answers");
        let mut answers: Vec<Value> =
            serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        answers.remove(0)
    }

    // ----- work

    /// A commit by `agent`: writes the files, stages them and commits, from the agent's process.
    /// Returns when the timeline shows one more event of an agent.
    pub fn agent_commit(&self, agent: &mut Agent, files: &[(&str, &str)], message: &str) -> String {
        let before = self.event_count(true);
        agent.run(&self.commit_script(files, message, true));
        self.wait_events(true, before + 1);
        self.head()
    }

    /// A commit by the test itself (no agent in its ancestry): an unattributed event.
    pub fn unattributed_commit(&self, files: &[(&str, &str)], message: &str) -> String {
        let before = self.event_count(false);
        for (file, content) in files {
            std::fs::write(self.worktree.join(file), content).unwrap();
        }
        let mut add = vec!["add", "--"];
        add.extend(files.iter().map(|(f, _)| *f));
        self.fx.git_in(&self.worktree, &add);
        self.fx
            .git_in(&self.worktree, &["commit", "-q", "-m", message]);
        self.wait_events(false, before + 1);
        self.head()
    }

    /// A commit made by `agent` without waiting for the engine (the engine is stopped).
    pub fn agent_commit_unobserved(
        &self,
        agent: &mut Agent,
        files: &[(&str, &str)],
        message: &str,
    ) {
        agent.run(&self.commit_script(files, message, false));
    }

    fn commit_script(&self, files: &[(&str, &str)], message: &str, slow: bool) -> String {
        let mut script = String::new();
        for (file, content) in files {
            assert!(!content.contains('\'') && !file.contains('\''));
            script.push_str(&format!("printf '%s\\n' '{content}' > '{file}' && "));
        }
        script.push_str("git add --");
        for (file, _) in files {
            script.push_str(&format!(" '{file}'"));
        }
        let hooks = if slow {
            format!(" -c core.hooksPath={}", self.hooks.display())
        } else {
            String::new()
        };
        script.push_str(&format!(" && git{hooks} commit -q -m '{message}'"));
        script
    }

    pub fn head(&self) -> String {
        self.fx
            .git_in(&self.worktree, &["rev-parse", "HEAD"])
            .trim()
            .to_owned()
    }

    /// Subjects of the commits of the worktree's branch, newest first.
    pub fn log(&self) -> Vec<String> {
        self.fx
            .git_in(&self.worktree, &["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.worktree.join(file)).unwrap()
    }

    pub fn write(&self, file: &str, content: &str) {
        std::fs::write(self.worktree.join(file), content).unwrap();
    }

    /// The content of `file` in the index.
    pub fn staged(&self, file: &str) -> String {
        self.fx
            .git_in(&self.worktree, &["show", &format!(":{file}")])
    }

    // ----- timeline

    pub fn timeline(&self, agent: Option<&str>, since: &str) -> TimelineResult {
        let mut p = json!({"worktree": self.worktree, "since": since, "limit": 200});
        if let Some(agent) = agent {
            p["agent"] = json!(agent);
        }
        self.client().call(methods::TM_TIMELINE, p).unwrap()
    }

    /// Git events of the worktree (`event:<seq>`), attributed to an agent or not.
    pub fn events(&self, by_agent: bool) -> Vec<TimelineEntry> {
        self.timeline(None, "1h")
            .entries
            .into_iter()
            .filter(|e| e.id.starts_with("event:"))
            .filter(|e| matches!(e.actor, Actor::Agent { .. }) == by_agent)
            .collect()
    }

    fn event_count(&self, by_agent: bool) -> usize {
        self.events(by_agent).len()
    }

    fn wait_events(&self, by_agent: bool, at_least: usize) {
        let start = Instant::now();
        loop {
            let n = self.event_count(by_agent);
            if n >= at_least {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "the timeline never showed {at_least} events (agent: {by_agent}), it shows {n}: {:#?}",
                self.timeline(None, "1h")
                    .entries
                    .iter()
                    .map(|e| (&e.id, &e.actor))
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // ----- calls

    pub fn undo_call(
        &self,
        c: &mut Client,
        agent: &str,
        since: Option<&str>,
        token: Option<&str>,
    ) -> Result<Value, ClientError> {
        c.call(
            methods::TM_UNDO,
            self.undo_params(Some(agent), since, token),
        )
    }

    pub fn undo_params(
        &self,
        agent: Option<&str>,
        since: Option<&str>,
        token: Option<&str>,
    ) -> Value {
        let mut p = params(&self.worktree, token);
        if let Some(agent) = agent {
            p["agent"] = json!(agent);
        }
        if let Some(since) = since {
            p["since"] = json!(since);
        }
        p
    }

    /// An undo of `agent` in the last 20 minutes by the unattributed test client, confirmed as
    /// the interactive seam does: the challenge first, then its token.
    pub fn confirmed_agent_undo(&self, agent: &str) -> UndoResult {
        let mut c = self.client();
        let first = confirm_data(self.undo_call(&mut c, agent, Some("20m"), None));
        let token = token_of(&first);
        let value = self
            .undo_call(&mut c, agent, Some("20m"), Some(&token))
            .unwrap_or_else(|e| panic!("the confirmed agent undo failed: {e:?}"));
        serde_json::from_value(value).expect("an UndoResult")
    }

    /// A plain undo (no agent, no period), confirmed when the daemon asks for it.
    pub fn plain_undo_call(
        &self,
        c: &mut Client,
        token: Option<&str>,
    ) -> Result<Value, ClientError> {
        c.call(methods::TM_UNDO, self.undo_params(None, None, token))
    }

    pub fn redo_call(&self, c: &mut Client, token: Option<&str>) -> Result<Value, ClientError> {
        c.call(methods::TM_REDO, params(&self.worktree, token))
    }

    // ----- oplog

    /// The Time Machine's own requests of `kind` (undo, restore), oldest first: the operation, the
    /// states of its journal in order and the reason of its rejection.
    pub fn requests(&self, kind: OperationKind) -> Vec<Request> {
        let log = self.oplog();
        let log = log.lock().unwrap();
        let mut views = log.operations(&OperationFilter::default()).unwrap();
        views.sort_by_key(|v| v.record.seq);
        views
            .into_iter()
            .filter(|v| v.record.kind == kind)
            .map(|view| {
                let journal = log.journal(&view.record.operation_id).unwrap();
                let states = journal.iter().filter_map(|j| j.state.clone()).collect();
                let reason = journal
                    .iter()
                    .rev()
                    .find(|j| j.state.as_deref() == Some("rejected"))
                    .and_then(|j| j.detail.clone());
                Request {
                    view,
                    states,
                    reason,
                }
            })
            .collect()
    }

    pub fn snapshot_count(&self) -> usize {
        self.oplog()
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap()
            .len()
    }
}

pub struct Request {
    pub view: OperationView,
    /// States the request went through, oldest first.
    pub states: Vec<String>,
    pub reason: Option<String>,
}

/// An id of the timeline or of `undone_operation_ids` as a comparable key: `event:<seq>`,
/// `git-event-<seq>` and `operation:<id>` all reduce to what follows the prefix.
pub fn key_of(id: &str) -> String {
    for prefix in ["event:", "git-event-", "operation:"] {
        if let Some(rest) = id.strip_prefix(prefix) {
            return rest.to_owned();
        }
    }
    id.to_owned()
}

pub fn assert_intact(w: &World, before: &gitraptor_testkit::fingerprint::Snapshot) {
    let changes = gitraptor_testkit::diff(before, &w.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
}
