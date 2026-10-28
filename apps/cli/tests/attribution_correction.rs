//! US-GRP-010 end to end: each Gherkin scenario with the real `raptor` binary as daemon and as
//! client, over a temporary machine (INF-GRP-001): temporary repo and profile, never this repo
//! nor the real profile (NFR-01). The developer runs under a pty (`script`), so the engine takes
//! them for the developer.
//!
//! The detected "Claude Code" is `raptor-fake-agent`, a copy of this test binary that runs each
//! line of its input with `/bin/sh` (so what it runs descends from it), the only agent
//! executable the debug daemon knows (`GITRAPTOR_AGENT_EXECUTABLES`). Its commits run a slow
//! `post-commit` hook so S3 attributes them (US-GRP-007). No fixed waits: each state is awaited
//! with a deadline.
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
    GitEventView,
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
/// Set for a simulated agent.
const AGENT_MODE: &str = "RAPTOR_FAKE_AGENT_MODE";
/// Printed by a simulated agent after each command, with its outcome.
const DONE: &str = "<<raptor-fake-agent-done>>";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated agent: with [`AGENT_MODE`] it runs every line of its input with
/// `/bin/sh` and prints [`DONE`] after each; at the end of its input it exits. As a normal test
/// it does nothing.
#[test]
fn fake_agent_entry() {
    if std::env::var_os(AGENT_MODE).is_none() {
        return;
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
    /// Runs `command` in its shell, waits for it and returns whether it succeeded and what it
    /// printed (stdout and stderr).
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

    /// `raptor <args>` from the agent's shell, in `lang`.
    fn raptor_in(&mut self, lang: &str, args: &str) -> (bool, String) {
        self.try_run(&format!("LANG={lang} {RAPTOR} {args}"))
    }
}

impl Drop for FakeAgent {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the folder the agents are
/// installed in and the slow hooks.
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
        // A hook folder used per command (`git -c core.hooksPath=...`): the repo's hooks are
        // never touched. It keeps `git` alive for a second, as a commit with hooks does (S3).
        let hooks = outside.path().join("slow-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let post_commit = hooks.join("post-commit");
        std::fs::write(&post_commit, "#!/bin/sh\nsleep 1\n").unwrap();
        std::fs::set_permissions(&post_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { f, outside }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_CLAUDE.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
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
    fn register(&self, agent: &str, worktree: &Path) {
        let out = self.developer(&[
            "agent",
            "register",
            agent,
            "--worktree",
            worktree.to_str().unwrap(),
        ]);
        assert!(out.status.success(), "{}", text(&out));
    }

    /// `raptor agent correct <agent> --worktree <worktree> [extra]` by the developer.
    fn correct_in(&self, lang: &str, agent: &str, worktree: &Path, extra: &[&str]) -> Output {
        let mut args = vec![
            "agent",
            "correct",
            agent,
            "--worktree",
            worktree.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        self.developer_in(lang, &args)
    }

    fn correct(&self, agent: &str, worktree: &Path) -> String {
        let out = self.correct_in("en_US.UTF-8", agent, worktree, &[]);
        assert!(
            out.status.success(),
            "correct {agent} failed: {}",
            text(&out)
        );
        text(&out)
    }

    fn uncorrect(&self, worktree: &Path) -> String {
        let out = self.developer(&[
            "agent",
            "uncorrect",
            "--worktree",
            worktree.to_str().unwrap(),
        ]);
        assert!(out.status.success(), "uncorrect failed: {}", text(&out));
        text(&out)
    }

    fn add(&self) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    /// Launches the simulated Claude Code in `worktree`.
    fn launch(&self, worktree: &Path) -> FakeAgent {
        let agent = self.outside.path().join("bin").join(FAKE_CLAUDE);
        if !agent.exists() {
            gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
        }
        let mut child = Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_MODE, "attached")
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
        let out = self.raptor(&["status", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: gitraptor_api::messages::Snapshot = self
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

    /// Waits until `status --json` shows sessions in `worktree` that `ok` accepts.
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
        let out = self.raptor(&["sessions", "--all", "--json"]);
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

    /// Waits until the history has the commit `oid`.
    fn commit_event(&self, oid: &str) -> GitEventView {
        let start = Instant::now();
        loop {
            let history = self.history();
            if let Some(e) = history.iter().find(|e| {
                e.kind == GitEventKind::Commit && e.details.new_commit.as_deref() == Some(oid)
            }) {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no commit {oid}: {history:#?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The actor the history gives to the commit `oid`, now.
    fn actor_of(&self, oid: &str) -> Actor {
        self.commit_event(oid).actor
    }

    /// A commit by the simulated agent in `worktree` (with the slow hook, so S3 attributes it);
    /// its id, once the engine has recorded it.
    fn agent_commit(&self, agent: &mut FakeAgent, worktree: &Path, message: &str) -> String {
        agent.run(&format!(
            "printf 'user\\n{message}\\n' > login.txt && git add login.txt && \
             git -c core.hooksPath={} commit -q -m '{message}'",
            self.outside.path().join("slow-hooks").display()
        ));
        let oid = self
            .f
            .git_in(worktree, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        self.commit_event(&oid);
        oid
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

/// "demo": `main` with `login.txt`, branch `feat-login` and its worktree "feat-login".
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

fn claude_detected() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn codex_registered() -> Actor {
    Actor::Agent {
        kind: AgentKind::Other,
        name: Some(UntrustedName::new("Codex")),
        origin: AgentOrigin::Registered,
    }
}

/// A session of Claude Code with origin "detected".
fn is_claude_detected(s: &Value) -> bool {
    s["agent"] == "claude-code" && s["origin"] == "detected"
}

/// A session of "other agent: Codex" with origin "registered".
fn is_codex_registered(s: &Value) -> bool {
    s["agent"] == "other" && s["agent_name"] == "Codex" && s["origin"] == "registered"
}

/// The only present session of `worktree` is the detected Claude Code (agent alive).
fn wait_detected(m: &Machine, wt: &Path) -> Value {
    let sessions = m.sessions_when(wt, |s| {
        let p = present(s);
        p.len() == 1 && is_claude_detected(p[0])
    });
    present(&sessions)[0].clone()
}

/// "feat-login" has one session, Codex registered, and does not appear as shared.
fn assert_only_codex(m: &Machine, wt: &Path) {
    let worktree = m.status_worktree(wt);
    let sessions = worktree["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_codex_registered(&sessions[0]), "{sessions:#?}");
    assert_eq!(worktree["shared"], false, "{worktree:#}");
}

// ------------------------------------------------------------ Escenario 1

/// GRP010-CLI-1: corregir reemplaza la atribución detectada.
#[test]
fn correcting_replaces_the_detected_attribution() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado una única sesión en "feat-login", de "Claude Code" con origen "detectado".
    let mut claude = m.launch(&wt);
    wait_detected(&m, &wt);

    // Cuando el desarrollador corrige la atribución de "feat-login" a "Codex".
    let out = m.correct("Codex", &wt);
    assert!(
        out.contains("the attribution of this session changed from")
            && out.contains("from its start; its events follow it"),
        "{out}"
    );

    // Entonces "feat-login" tiene una sola sesión, de "otro agente: Codex", "registrado".
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_codex_registered(&sessions[0]), "{sessions:#?}");
    // Y "feat-login" no figura como compartido.
    assert_eq!(m.status_worktree(&wt)["shared"], false);
    // Y un commit posterior tiene como actor "otro agente: Codex".
    let oid = m.agent_commit(&mut claude, &wt, "after the correction");
    assert_eq!(m.actor_of(&oid), codex_registered());
    m.stop();
}

// ------------------------------------------------------------ Escenario 2

/// GRP010-CLI-2: la corrección alcanza lo ya atribuido a la sesión desde su inicio, no a otra.
#[test]
fn the_correction_reaches_the_session_from_its_start() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado un commit "c0" de otra sesión de "Claude Code" ya terminada en "feat-login".
    let mut first = m.launch(&wt);
    wait_detected(&m, &wt);
    let c0 = m.agent_commit(&mut first, &wt, "c0");
    assert_eq!(m.actor_of(&c0), claude_detected());
    drop(first);
    m.sessions_when(&wt, |s| s.len() == 1 && s[0]["state"] == "ended");
    // Y una sesión de "Claude Code" detectada a la que se atribuyeron "c1" y "c2".
    let mut second = m.launch(&wt);
    m.sessions_when(&wt, |s| {
        s.len() == 2 && present(s).len() == 1 && is_claude_detected(present(s)[0])
    });
    let c1 = m.agent_commit(&mut second, &wt, "c1");
    let c2 = m.agent_commit(&mut second, &wt, "c2");
    assert_eq!(m.actor_of(&c1), claude_detected());
    assert_eq!(m.actor_of(&c2), claude_detected());

    // Cuando el desarrollador corrige la atribución de "feat-login" a "Codex".
    m.correct("Codex", &wt);

    // Entonces "c1" y "c2" son de "otro agente: Codex", "registrado".
    assert_eq!(m.actor_of(&c1), codex_registered(), "c1");
    assert_eq!(m.actor_of(&c2), codex_registered(), "c2");
    // Y "c0" sigue figurando como de "Claude Code" con origen "detectado".
    assert_eq!(m.actor_of(&c0), claude_detected(), "c0");
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// GRP010-CLI-3: la detección no deshace una corrección vigente.
#[test]
fn detection_does_not_undo_a_correction() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch(&wt);
    wait_detected(&m, &wt);
    m.correct("Codex", &wt);

    // Cuando hay nueva actividad en "feat-login" (the simulated agent is still alive).
    let oid = m.agent_commit(&mut claude, &wt, "more activity");
    assert_eq!(m.actor_of(&oid), codex_registered());

    // Entonces sigue con una sola sesión, de "otro agente: Codex" con origen "registrado".
    assert_only_codex(&m, &wt);
    let all = m.all_sessions();
    assert_eq!(all.len(), 1, "{all:#?}");
    assert!(is_codex_registered(&all[0]), "{all:#?}");
    m.stop();
}

// ------------------------------------------------------------ Escenario 4

/// GRP010-CLI-4: retirar la corrección devuelve la atribución detectada, también a lo
/// reatribuido (Q50).
#[test]
fn withdrawing_returns_the_detected_attribution() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch(&wt);
    wait_detected(&m, &wt);
    let c1 = m.agent_commit(&mut claude, &wt, "c1");
    let c2 = m.agent_commit(&mut claude, &wt, "c2");
    m.correct("Codex", &wt);
    assert_eq!(m.actor_of(&c1), codex_registered());
    assert_eq!(m.actor_of(&c2), codex_registered());

    // Cuando el desarrollador retira la corrección.
    let out = m.uncorrect(&wt);
    assert!(out.contains("the correction was withdrawn"), "{out}");

    // Entonces una sola sesión, de "Claude Code" con origen "detectado".
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_claude_detected(&sessions[0]), "{sessions:#?}");
    // Y "c1" y "c2" vuelven a figurar como de "Claude Code" con origen "detectado".
    assert_eq!(m.actor_of(&c1), claude_detected(), "c1");
    assert_eq!(m.actor_of(&c2), claude_detected(), "c2");
    m.stop();
}

// ------------------------------------------------------------ Escenario 5

/// GRP010-CLI-5: la corrección se mantiene tras reiniciar el motor.
#[test]
fn the_correction_survives_an_engine_restart() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch(&wt);
    wait_detected(&m, &wt);
    let c1 = m.agent_commit(&mut claude, &wt, "c1");
    let c2 = m.agent_commit(&mut claude, &wt, "c2");
    m.correct("Codex", &wt);
    let before = m.status_sessions(&wt);
    assert_eq!(before.len(), 1, "{before:#?}");

    // Cuando el motor deja de ejecutarse y vuelve a arrancar (`status` lo arranca a demanda).
    m.stop();
    let after = m.sessions_when(&wt, |s| s.len() == 1 && is_codex_registered(&s[0]));

    // Entonces sigue con una sola sesión, de Codex "registrado", y "c1" y "c2" también.
    assert_eq!(after[0]["session_id"], before[0]["session_id"]);
    assert_only_codex(&m, &wt);
    assert_eq!(m.actor_of(&c1), codex_registered(), "c1");
    assert_eq!(m.actor_of(&c2), codex_registered(), "c2");
    m.stop();
}

// ------------------------------------------------------------ Escenario 6

/// GRP010-CLI-6: un agente no corrige (6c), en inglés y en español; el intento queda auditado
/// y nada cambia.
#[test]
fn an_agent_correcting_is_told_only_the_developer_can() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch(&wt);
    let before = wait_detected(&m, &wt);

    // Cuando el agente "Claude Code" de "feat-login" corrige la atribución de "feat-login".
    let (ok, out) = claude.raptor_in("en_US.UTF-8", "agent correct Codex");
    assert!(!ok, "{out}");
    assert!(
        out.contains("only the developer can correct an attribution"),
        "{out}"
    );
    let (ok, out) = claude.raptor_in("es_ES.UTF-8", "agent correct Codex");
    assert!(!ok, "{out}");
    assert!(
        out.contains("solo el desarrollador puede corregir una atribución"),
        "{out}"
    );

    // Entonces ninguna atribución ni sesión cambia, y el intento queda auditado.
    let sessions = m.status_sessions(&wt);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert_eq!(sessions[0]["session_id"], before["session_id"]);
    assert!(is_claude_detected(&sessions[0]), "{sessions:#?}");
    let attempts: Vec<AuditEntry> = m
        .audit()
        .into_iter()
        .filter(|e| e.operation == methods::ATTRIBUTION_CORRECT)
        .collect();
    assert!(!attempts.is_empty(), "the refusal was not audited");
    assert!(
        attempts.iter().all(|e| e.outcome == AuditOutcome::Rejected),
        "{attempts:#?}"
    );
    m.stop();
}

/// GRP010-CLI-7: el texto de `registered-session` dice retirar el registro y registrar el
/// agente correcto, en inglés y en español.
#[test]
fn a_registered_session_text_says_withdraw_and_register() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    m.register("Codex", &wt);
    let sessions = m.sessions_when(&wt, |s| s.len() == 1 && is_codex_registered(&s[0]));
    let id = sessions[0]["session_id"].as_str().unwrap().to_owned();

    // Cuando el desarrollador corrige esa sesión registrada.
    let out = m.correct_in("en_US.UTF-8", "Claude Code", &wt, &["--session", &id]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(
            "withdraw the registration with raptor agent withdraw and register the right agent"
        ),
        "{}",
        text(&out)
    );
    let out = m.correct_in("es_ES.UTF-8", "Claude Code", &wt, &["--session", &id]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out)
            .contains("retira el registro con raptor agent withdraw y registra el agente correcto"),
        "{}",
        text(&out)
    );
    // Nada cambió.
    assert_only_codex(&m, &wt);
    m.stop();
}

/// GRP010-CLI-9 (D9): con una corrección activa, corregir al agente detectado se acepta y el
/// mensaje sugiere `raptor agent uncorrect`, en inglés y en español.
#[test]
fn correcting_back_to_the_detected_agent_hints_uncorrect() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let _claude = m.launch(&wt);
    wait_detected(&m, &wt);
    m.correct("Codex", &wt);

    let out = m.correct_in("en_US.UTF-8", "Claude Code", &wt, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(
            "this is the agent the engine detected: raptor agent uncorrect returns to it \
             without adding another correction"
        ),
        "{}",
        text(&out)
    );
    let out = m.correct_in("es_ES.UTF-8", "Codex", &wt, &[]);
    assert!(out.status.success(), "{}", text(&out));
    let out = m.correct_in("es_ES.UTF-8", "Claude Code", &wt, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(
            "es el agente que detectó el motor: raptor agent uncorrect vuelve a él sin añadir \
             otra corrección"
        ),
        "{}",
        text(&out)
    );
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

/// GRP010-CLI-8: corregir y retirar no modifican el repo: cero diferencias fuera del perfil
/// (INF-GRP-001, NFR-01).
#[test]
fn repo_intact_correcting_does_not_modify_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo();
    std::fs::write(wt.join("login.txt"), "user\nchanged\n").unwrap();
    m.f.write("untracked.txt", "u\n");
    let report = check("US-GRP-010 correction", &m.f, &engine_profile(), || {
        m.add();
        let _claude = m.launch(&wt);
        wait_detected(&m, &wt);
        m.correct("Codex", &wt);
        assert_only_codex(&m, &wt);
        m.uncorrect(&wt);
        let sessions = m.status_sessions(&wt);
        assert!(is_claude_detected(&sessions[0]), "{sessions:#?}");
        m.stop();
    });
    report.assert_intact();
}
