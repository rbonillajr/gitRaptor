//! US-GRP-009 end to end: each Gherkin scenario, and each example, with the
//! real `raptor` binary as daemon and as client, over a temporary machine
//! built by the "intact repo" harness (INF-GRP-001): temporary repo and
//! profile, never this repo nor the real profile (NFR-01). The developer
//! runs under a pty (`script`), so the engine takes them for the developer.
//!
//! Two simulated agents, copies of this test binary that run each line of
//! their input with `/bin/sh` (so what they run descends from them):
//! - Claude Code, `raptor-fake-agent`, the only agent executable the debug
//!   daemon knows (`GITRAPTOR_AGENT_EXECUTABLES`);
//! - "the agent itself" without full support, `codex`: not Claude Code for
//!   the engine, and detached from any terminal (`setsid`), as an agent
//!   that runs without one is.
//!
//! No fixed waits: each state is awaited with a deadline.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{
    AuditEntry, AuditListResult, AuditOutcome, ClientKind, EventsHistoryResult, GitEventKind,
    GitEventView, RefusalReason, Snapshot,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, UntrustedName, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_CLAUDE: &str = "raptor-fake-agent";
const FAKE_CODEX: &str = "codex";
/// Set for a simulated agent: `attached` or `detached`.
const AGENT_MODE: &str = "RAPTOR_FAKE_AGENT_MODE";
/// Printed by a simulated agent after each command, with its outcome.
const DONE: &str = "<<raptor-fake-agent-done>>";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of a simulated agent: when this test binary runs with
/// [`AGENT_MODE`], it runs every line of its input with `/bin/sh` and
/// prints [`DONE`] after each; at the end of its input it exits. A
/// `detached` one first leaves its terminal. As a normal test it does
/// nothing.
#[test]
fn fake_agent_entry() {
    let Some(mode) = std::env::var_os(AGENT_MODE) else {
        return;
    };
    if mode == "detached" {
        nix::unistd::setsid().unwrap();
    }
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(&line).status();
        println!("{DONE} {}", status.map(|s| s.success()).unwrap_or(false));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

/// A running simulated agent.
struct FakeAgent {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeAgent {
    /// Runs `command` in its shell, waits for it and returns whether it
    /// succeeded and what it printed (stdout and stderr).
    fn try_run(&mut self, command: &str) -> (bool, String) {
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
            // libtest may print its header on the same line.
            if let Some((before, ok)) = line.split_once(DONE) {
                output.push_str(before);
                return (ok.trim() == "true", output);
            }
            output.push_str(&line);
        }
    }

    fn run(&mut self, command: &str) -> String {
        let (ok, output) = self.try_run(command);
        assert!(ok, "{command} failed: {output}");
        output
    }

    /// `raptor <args>` from the agent's shell, in English.
    fn raptor(&mut self, args: &str) -> (bool, String) {
        self.try_run(&format!("LANG=en_US.UTF-8 {RAPTOR} {args}"))
    }
}

impl Drop for FakeAgent {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the
/// folder the agents are installed in, the test clock's skew file and a
/// repo the engine does not observe.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
}

impl Machine {
    fn new(f: Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        Self { f, outside }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn skew_file(&self) -> PathBuf {
        self.outside.path().join("clock-skew-ms")
    }

    /// Moves the clock of the sessions `ms` forward from the real time.
    fn skew(&self, ms: i64) {
        std::fs::write(self.skew_file(), ms.to_string()).unwrap();
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_CLAUDE.into()),
            (
                "GITRAPTOR_TEST_CLOCK_SKEW_FILE",
                self.skew_file().into_os_string(),
            ),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn raptor(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// The developer, from their own terminal, in `lang`.
    fn developer_in(&self, lang: &str, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .env("LANG", lang)
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn developer(&self, args: &[&str]) -> Output {
        self.developer_in("en_US.UTF-8", args)
    }

    /// The developer registers `agent` in `worktree`; panics on failure.
    fn register(&self, agent: &str, worktree: &Path) -> String {
        let out = self.developer(&[
            "agent",
            "register",
            agent,
            "--worktree",
            worktree.to_str().unwrap(),
        ]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }

    fn withdraw(&self, agent: &str, worktree: &Path) -> Output {
        self.developer(&[
            "agent",
            "withdraw",
            agent,
            "--worktree",
            worktree.to_str().unwrap(),
        ])
    }

    fn add(&self) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    /// Launches a simulated agent in `worktree`: Claude Code, or "codex"
    /// detached from the terminal.
    fn launch(&self, exe: &str, worktree: &Path) -> FakeAgent {
        let agent = self.outside.path().join("bin").join(exe);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        let mode = if exe == FAKE_CLAUDE {
            "attached"
        } else {
            "detached"
        };
        let mut child = Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_MODE, mode)
            .env("HOME", &self.f.home)
            .current_dir(worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        FakeAgent {
            child,
            stdin,
            stdout,
        }
    }

    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: Snapshot = self
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        snapshot.repos[0].repo_id.clone()
    }

    /// The worktree `raptor status --json` shows at `worktree`.
    fn status_worktree(&self, worktree: &Path) -> Value {
        let status = self.status();
        status["repos"][0]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["path"] == worktree.to_str().unwrap())
            .cloned()
            .unwrap_or_else(|| panic!("no {}: {status:#}", worktree.display()))
    }

    fn status_sessions(&self, worktree: &Path) -> Vec<Value> {
        self.status_worktree(worktree)["sessions"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Waits until `status --json` shows sessions in `worktree` that `ok`
    /// accepts.
    fn sessions_when(&self, worktree: &Path, ok: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let start = Instant::now();
        loop {
            let sessions = self.status_sessions(worktree);
            if ok(&sessions) {
                return sessions;
            }
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "{}: {sessions:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `raptor sessions --all --json`.
    fn all_sessions(&self) -> Vec<Value> {
        let out = self.raptor(&["sessions", "--all", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        value["sessions"].as_array().unwrap().clone()
    }

    fn history(&self) -> Vec<GitEventView> {
        let page: EventsHistoryResult = self
            .client()
            .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
            .unwrap();
        page.events
    }

    /// Waits until the history has the commit at `HEAD` of `worktree`.
    fn commit_event(&self, worktree: &Path) -> GitEventView {
        let head = self.f.git_in(worktree, &["rev-parse", "HEAD"]);
        let head = head.trim();
        let start = Instant::now();
        loop {
            let history = self.history();
            if let Some(e) = history.iter().find(|e| {
                e.kind == GitEventKind::Commit
                    && Path::new(e.worktree.raw()) == worktree
                    && e.details.new_commit.as_deref() == Some(head)
            }) {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no commit {head} in {}: {history:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A commit of the developer (no agent process) in `worktree`.
    fn commit(&self, worktree: &Path, message: &str) -> GitEventView {
        std::fs::write(worktree.join("login.txt"), format!("user\n{message}\n")).unwrap();
        self.f.git_in(worktree, &["commit", "-qam", message]);
        self.commit_event(worktree)
    }

    fn audit(&self) -> Vec<AuditEntry> {
        let page: AuditListResult = self.client().call(methods::AUDIT_LIST, json!({})).unwrap();
        page.entries
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// "demo": `main` with `login.txt`, branch `feat-login` and its worktree
/// "feat-login" (`<root>/wt-feat-login`).
fn demo() -> (Machine, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let wt = f
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();
    (Machine::new(f), wt)
}

/// `demo` observed, with the observer settled.
fn observed() -> (Machine, PathBuf) {
    let (m, wt) = demo();
    m.add();
    std::thread::sleep(Duration::from_millis(300));
    (m, wt)
}

fn present(sessions: &[Value]) -> Vec<&Value> {
    sessions.iter().filter(|s| s["state"] != "ended").collect()
}

fn other_agent(name: &str) -> Actor {
    Actor::Agent {
        kind: AgentKind::Other,
        name: Some(UntrustedName::new(name)),
        origin: AgentOrigin::Registered,
    }
}

/// One present registered "other agent" session of `name`, in `state`.
fn is_other(s: &Value, name: &str, state: &str) -> bool {
    s["agent"] == "other"
        && s["agent_name"] == name
        && s["origin"] == "registered"
        && s["state"] == state
}

// ------------------------------------------------------------ Escenario 1

/// Registrar Claude Code crea una sesión con origen registrado.
#[test]
fn registering_claude_code_creates_a_registered_session() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado el worktree "feat-login" sin ninguna sesión.
    assert!(m.status_sessions(&wt).is_empty());

    // Cuando el desarrollador registra "Claude Code" en "feat-login".
    let out = m.register("Claude Code", &wt);
    assert!(out.contains("Claude Code is registered here"), "{out}");

    // Entonces una sesión de "Claude Code", "Activo", origen "registrado".
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    let s = &sessions[0];
    assert_eq!(s["agent"], "claude-code");
    assert_eq!(s["state"], "active");
    assert_eq!(s["origin"], "registered");
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("Claude Code · Activo · registrado"),
        "{}",
        text(&out)
    );
    m.stop();
}

// ------------------------------------------------------------ Escenario 2

/// Registrar un agente ya detectado confirma su sesión (Q39).
#[test]
fn registering_a_detected_agent_confirms_its_session() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado una única sesión en "feat-login", de "Claude Code", "detectado".
    let mut claude = m.launch(FAKE_CLAUDE, &wt);
    let before = m.sessions_when(&wt, |s| {
        s.len() == 1 && s[0]["agent"] == "claude-code" && s[0]["origin"] == "detected"
    });

    // Cuando "Claude Code" se registra a sí mismo en "feat-login".
    let (ok, out) = claude.raptor("agent register 'Claude Code' --json");
    assert!(ok, "{out}");
    // libtest's header may come first.
    let line = out.find('{').map_or("", |i| out[i..].trim());
    let result: Value = serde_json::from_str(line).unwrap_or_else(|_| panic!("{out}"));
    assert_eq!(result["outcome"], "confirmed", "{result:#}");
    assert_eq!(result["session_id"], before[0]["session_id"]);

    // Entonces "feat-login" sigue con una sola sesión, la misma.
    let worktree = m.status_worktree(&wt);
    let sessions = worktree["sessions"].as_array().unwrap();
    assert_eq!(present(sessions).len(), 1, "{sessions:#?}");
    assert_eq!(sessions[0]["session_id"], before[0]["session_id"]);
    assert_eq!(sessions[0]["agent"], "claude-code");
    // Y "feat-login" no figura como compartido.
    assert_eq!(worktree["shared"], false);
    // Registering again changes nothing.
    let (ok, out) = claude.raptor("agent register 'Claude Code'");
    assert!(ok, "{out}");
    assert!(out.contains("already registered here"), "{out}");
    assert_eq!(m.all_sessions().len(), 1);
    m.stop();
}

/// P16 (decided with this story, D6): the confirmed session shows origin
/// "registered"; its id and its detection do not change.
#[test]
fn p16_a_confirmed_session_shows_origin_registered() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch(FAKE_CLAUDE, &wt);
    m.sessions_when(&wt, |s| s.len() == 1 && s[0]["origin"] == "detected");
    let (ok, out) = claude.raptor("agent register claude-code");
    assert!(ok, "{out}");
    assert!(out.contains("its session is confirmed"), "{out}");
    let sessions = m.sessions_when(&wt, |s| s.len() == 1 && s[0]["origin"] == "registered");
    assert_eq!(sessions[0]["state"], "active");
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// Who registers in the scenario outline.
#[derive(Clone, Copy)]
enum Who {
    Developer,
    TheAgentItself,
}

/// Un agente sin soporte completo se acepta como otro agente.
fn an_agent_without_full_support_is_accepted_as_other_agent(who: Who, agent: &str) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let message = format!("{agent} after registering");

    // Cuando "<quien>" registra el agente "<agente>" en "feat-login".
    let (out, commit) = match who {
        Who::Developer => {
            let out = m.register(agent, &wt);
            let out_es = m.developer_in(
                "es_ES.UTF-8",
                &[
                    "agent",
                    "register",
                    agent,
                    "--worktree",
                    wt.to_str().unwrap(),
                ],
            );
            assert!(
                text(&out_es).contains(&format!(
                    "{agent} se registró como otro agente: se observa y se le atribuye su \
                     actividad, sin funciones específicas"
                )),
                "{}",
                text(&out_es)
            );
            (out, m.commit(&wt, &message))
        }
        Who::TheAgentItself => {
            let mut codex = m.launch(FAKE_CODEX, &wt);
            let (ok, out) = codex.raptor(&format!("agent register {agent}"));
            assert!(ok, "{out}");
            codex.run(&format!(
                "printf 'user\\n{agent}\\n' > login.txt && git commit -qam '{message}'"
            ));
            (out, m.commit_event(&wt))
        }
    };

    // Entonces una sesión de "otro agente: <agente>", origen "registrado".
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_other(&sessions[0], agent, "active"), "{sessions:#?}");
    let status = m.raptor(&["status"], &[]);
    assert!(
        text(&status).contains(&format!("another agent: {agent} · Active · registered")),
        "{}",
        text(&status)
    );
    // Y un commit posterior en "feat-login" tiene como actor "otro agente".
    assert_eq!(commit.actor, other_agent(agent), "{commit:#?}");
    let events = m.raptor(&["events"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&events).contains(&format!("(otro agente: {agent}, registrado)")),
        "{}",
        text(&events)
    );
    // Y el motor informa que se observa y se le atribuye su actividad, sin
    // funciones específicas.
    assert!(
        out.contains(&format!(
            "{agent} registered as another agent: it is observed and its activity \
             attributed to it, without specific functions"
        )),
        "{out}"
    );
    m.stop();
}

#[test]
fn an_agent_without_full_support_is_accepted_as_other_agent_developer_codex() {
    an_agent_without_full_support_is_accepted_as_other_agent(Who::Developer, "Codex");
}

#[test]
fn an_agent_without_full_support_is_accepted_as_other_agent_agent_codex() {
    an_agent_without_full_support_is_accepted_as_other_agent(Who::TheAgentItself, "Codex");
}

#[test]
fn an_agent_without_full_support_is_accepted_as_other_agent_developer_cursor() {
    an_agent_without_full_support_is_accepted_as_other_agent(Who::Developer, "Cursor");
}

#[test]
fn an_agent_without_full_support_is_accepted_as_other_agent_developer_copilot() {
    an_agent_without_full_support_is_accepted_as_other_agent(Who::Developer, "Copilot");
}

// ------------------------------------------------------------ Escenario 4

/// El registro en un destino inválido se rechaza.
fn registering_in_an_invalid_destination_is_rejected(destination: &str) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // Dado el repo "demo" observado y el repo "otro" no observado.
    let (m, _wt) = observed();
    let otro = m.outside.path().join("otro");
    std::fs::create_dir(&otro).unwrap();
    m.f.git_in(&otro, &["init", "-q", "-b", "main"]);
    std::fs::write(otro.join("a.txt"), "a\n").unwrap();
    m.f.git_in(&otro, &["add", "a.txt"]);
    m.f.git_in(&otro, &["commit", "-qm", "a"]);
    let otro_wt = m.outside.path().join("otro-feat");
    m.f.git_in(
        &otro,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            otro_wt.to_str().unwrap(),
        ],
    );
    let plain = m.outside.path().join("plain");
    std::fs::create_dir(&plain).unwrap();

    let (target, en, es) = match destination {
        "not-a-worktree" => (
            plain,
            "that folder is not a worktree of any observed repo",
            "ese directorio no es un worktree de ningún repo observado",
        ),
        _ => (
            otro_wt,
            "the repo is not observed",
            "el repo no está observado",
        ),
    };
    // Cuando el desarrollador registra "Claude Code" en "<destino>".
    let args = [
        "agent",
        "register",
        "Claude Code",
        "--worktree",
        target.to_str().unwrap(),
    ];
    let out = m.developer(&args);
    // Entonces el motor rechaza el registro indicando "<motivo>".
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(en), "{}", text(&out));
    let out = m.developer_in("es_ES.UTF-8", &args);
    assert!(text(&out).contains(es), "{}", text(&out));
    let err = m
        .client()
        .call::<_, Value>(
            methods::REGISTRATION_REGISTER,
            json!({"agent": {"kind": "claude-code"}, "worktree": target}),
        )
        .unwrap_err();
    assert!(format!("{err:?}").contains("-32015"), "{err:?}");
    // Y no se crea ninguna sesión.
    assert!(m.all_sessions().is_empty());
    m.stop();
}

#[test]
fn registering_in_an_invalid_destination_is_rejected_not_a_worktree() {
    registering_in_an_invalid_destination_is_rejected("not-a-worktree");
}

#[test]
fn registering_in_an_invalid_destination_is_rejected_repo_not_observed() {
    registering_in_an_invalid_destination_is_rejected("repo-not-observed");
}

// ------------------------------------------------------------ Escenario 5

/// Retirar el registro termina la sesión.
#[test]
fn withdrawing_the_registration_ends_the_session() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado "otro agente: Codex" registrado en "feat-login".
    m.register("Codex", &wt);
    m.sessions_when(&wt, |s| s.len() == 1 && is_other(&s[0], "Codex", "active"));

    // Cuando pasa más del umbral de inactividad sin actividad.
    m.skew(5 * 60_000 + 5_000);
    // Entonces su sesión figura como "Inactivo", no como "Terminado".
    let sessions = m.sessions_when(&wt, |s| s.len() == 1 && s[0]["state"] != "active");
    assert!(is_other(&sessions[0], "Codex", "inactive"), "{sessions:#?}");
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("otro agente: Codex · Inactivo · registrado"),
        "{}",
        text(&out)
    );

    // Cuando el desarrollador retira el registro de "Codex".
    let out = m.withdraw("Codex", &wt);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("the registration of another agent: Codex was withdrawn"),
        "{}",
        text(&out)
    );
    // Entonces la sesión pasa a "Terminado".
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert_eq!(sessions[0]["state"], "ended");
    assert_eq!(sessions[0]["end_cause"], "registration-withdrawn");
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("otro agente: Codex · Terminado · registrado"),
        "{}",
        text(&out)
    );
    m.stop();
}

// ------------------------------------------------- Persistence (M1, D9)

/// A registration survives an engine restart: the session continues and a
/// later commit is still the agent's.
#[test]
fn a_registration_survives_an_engine_restart() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    m.register("Codex", &wt);
    let before = m.sessions_when(&wt, |s| s.len() == 1);
    m.stop();
    // `status` starts the engine again on demand.
    let after = m.sessions_when(&wt, |s| s.len() == 1 && is_other(&s[0], "Codex", "active"));
    assert_eq!(after[0]["session_id"], before[0]["session_id"]);
    let e = m.commit(&wt, "after restart");
    assert_eq!(e.actor, other_agent("Codex"), "{e:#?}");
    m.stop();
}

// ---------------------------------------------- Same agent (BR-CONS-004)

/// Registering the same "other agent" twice, whatever its case or spaces,
/// leaves one session.
#[test]
fn registering_the_same_other_agent_twice_does_not_duplicate() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    m.register("Codex", &wt);
    let out = m.register(" codex ", &wt);
    assert!(out.contains("was already registered here"), "{out}");
    let out = m.developer_in(
        "es_ES.UTF-8",
        &[
            "agent",
            "register",
            "CODEX",
            "--worktree",
            wt.to_str().unwrap(),
        ],
    );
    assert!(
        text(&out).contains("ya estaba registrado aquí"),
        "{}",
        text(&out)
    );
    assert_eq!(m.all_sessions().len(), 1);
    assert_eq!(m.status_worktree(&wt)["shared"], false);
    m.stop();
}

/// ADR-GRP-013 Validation 5: with a second agent registered the worktree
/// is shared and a commit without other evidence is unattributed; once it
/// is withdrawn, commits are Codex's again.
#[test]
fn a_second_agent_makes_commits_unattributed_until_it_is_withdrawn() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    m.register("Codex", &wt);
    m.register("Cursor", &wt);
    assert_eq!(m.status_worktree(&wt)["shared"], true);
    let e = m.commit(&wt, "shared");
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
    let out = m.withdraw("Cursor", &wt);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.status_worktree(&wt)["shared"], false);
    let e = m.commit(&wt, "codex again");
    assert_eq!(e.actor, other_agent("Codex"), "{e:#?}");
    m.stop();
}

// ------------------------------------------------ Withdrawal (Q41, D7)

/// Only a registration is withdrawn: a detected session, even confirmed,
/// ends with its process; an agent without registration has nothing to
/// withdraw; registering again after a withdrawal is a new session.
#[test]
fn withdrawal_only_ends_registrations_and_a_new_one_is_a_new_session() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let _claude = m.launch(FAKE_CLAUDE, &wt);
    m.sessions_when(&wt, |s| s.len() == 1 && s[0]["origin"] == "detected");
    let out = m.register("Claude Code", &wt);
    assert!(out.contains("its session is confirmed"), "{out}");
    for agent in ["Claude Code", "Codex"] {
        let out = m.withdraw(agent, &wt);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("has no registration to withdraw in that worktree"),
            "{}",
            text(&out)
        );
    }
    let out = m.developer_in(
        "es_ES.UTF-8",
        &[
            "agent",
            "withdraw",
            "Codex",
            "--worktree",
            wt.to_str().unwrap(),
        ],
    );
    assert!(
        text(&out).contains("una sesión detectada termina cuando termina su proceso"),
        "{}",
        text(&out)
    );

    m.register("Codex", &wt);
    let first = m.sessions_when(&wt, |s| present(s).len() == 2);
    let first = first.iter().find(|s| s["agent"] == "other").unwrap()["session_id"].clone();
    assert!(m.withdraw("Codex", &wt).status.success());
    m.register("Codex", &wt);
    let all = m.all_sessions();
    let codex: Vec<&Value> = all.iter().filter(|s| s["agent"] == "other").collect();
    assert_eq!(codex.len(), 2, "{all:#?}");
    assert!(
        codex
            .iter()
            .any(|s| s["session_id"] == first && s["state"] == "ended")
    );
    assert!(
        codex
            .iter()
            .any(|s| s["session_id"] != first && s["state"] == "active")
    );
    m.stop();
}

// ------------------------------------- What an agent may not do (M7, SEC-03)

/// An agent registers only in its own worktree and as the agent it is; each
/// refusal is audited.
#[test]
fn an_agent_registers_only_itself_in_its_own_worktree() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut codex = m.launch(FAKE_CODEX, &wt);
    let main = m.f.repo.canonicalize().unwrap();
    let (ok, out) = codex.raptor(&format!(
        "agent register Codex --worktree {}",
        main.display()
    ));
    assert!(!ok, "{out}");
    assert!(
        out.contains("an agent can only register in the worktree it works in"),
        "{out}"
    );
    let (ok, out) = codex.raptor("agent register 'Claude Code'");
    assert!(!ok, "{out}");
    assert!(
        out.contains("an agent can only register as the agent it is"),
        "{out}"
    );
    assert!(m.all_sessions().is_empty());
    let refused: Vec<Option<RefusalReason>> = m
        .audit()
        .into_iter()
        .filter(|e| e.operation == methods::REGISTRATION_REGISTER)
        .inspect(|e| assert_eq!(e.outcome, AuditOutcome::Rejected))
        .map(|e| e.reason)
        .collect();
    assert_eq!(
        refused,
        [
            Some(RefusalReason::WorktreeMismatch),
            Some(RefusalReason::AgentMismatch)
        ]
    );
    // Its own worktree, named or not, is fine.
    let (ok, out) = codex.raptor(&format!("agent register Codex --worktree {}", wt.display()));
    assert!(ok, "{out}");
    m.stop();
}

/// Withdrawing the registration of another agent is reserved to the
/// developer: an agent is refused and the attempt is audited. Withdrawing
/// its own belongs to US-MCP-006 (`unregister_agent`).
#[test]
fn an_agent_cannot_withdraw_another_registration() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    m.register("Cursor", &wt);
    let mut codex = m.launch(FAKE_CODEX, &wt);
    let (ok, out) = codex.raptor(&format!(
        "agent withdraw Cursor --worktree {}",
        wt.display()
    ));
    assert!(!ok, "{out}");
    assert!(out.contains("refused"), "{out}");
    let attempt = m
        .audit()
        .into_iter()
        .find(|e| e.operation == methods::REGISTRATION_WITHDRAW)
        .unwrap();
    assert_eq!(attempt.outcome, AuditOutcome::Rejected);
    assert!(is_other(&m.status_sessions(&wt)[0], "Cursor", "active"));
    m.stop();
}

// ------------------------------------------------------------ Repo intacto

fn engine_profile() -> Exceptions {
    Exceptions::engine_profile("profile")
        .with(Exception::Subtree {
            scope: "profile".into(),
            prefix: "run".into(),
        })
        .with(Exception::DirTimes {
            scope: "profile".into(),
            path: PathBuf::new(),
        })
}

/// Registering, committing and withdrawing do not modify the repo: zero
/// differences outside the profile (INF-GRP-001, BR-CONS-001).
#[test]
fn repo_intact_registering_agents_does_not_modify_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo();
    std::fs::write(wt.join("login.txt"), "user\nchanged\n").unwrap();
    m.f.write("untracked.txt", "u\n");
    let report = check("US-GRP-009 registration", &m.f, &engine_profile(), || {
        m.add();
        m.register("Codex", &wt);
        m.register("Claude Code", &m.f.repo.canonicalize().unwrap());
        let out = m.raptor(&["sessions", "--all"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(m.withdraw("Codex", &wt).status.success());
        m.stop();
    });
    report.assert_intact();
}
