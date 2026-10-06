//! US-GRP-007 end to end: each Gherkin scenario, and each example, with the
//! real `raptor` binary as daemon and as client, over a temporary machine
//! built by the "intact repo" harness (INF-GRP-001): temporary repo and
//! profile, never this repo nor the real profile (NFR-01). The developer
//! runs reserved commands under a pty (`script`).
//!
//! Claude Code is simulated: a copy of this test binary named
//! `raptor-fake-agent` (the only agent executable the debug daemon knows,
//! `GITRAPTOR_AGENT_EXECUTABLES`), so the tests also run under a real
//! Claude Code session. Launched with `RAPTOR_FAKE_CLAUDE` and its working
//! folder in a worktree, it runs each line of its standard input with
//! `/bin/sh`, so its `git`s descend from it as those of Claude Code's shell
//! tool do. Closing it is closing its input; killing it is the forced
//! close. It lives outside the fixture's root, so it is not part of the
//! fingerprint.
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
use gitraptor_api::event::SESSION_STATE;
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, SessionStateView, SessionView,
    Snapshot, SubscribeResult,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
/// Printed by the simulated Claude Code after each command.
const DONE: &str = "<<raptor-fake-claude-done>>";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated Claude Code: when this test binary runs as
/// `raptor-fake-agent` with `RAPTOR_FAKE_CLAUDE`, it runs every line of its
/// input with `/bin/sh` and prints [`DONE`] after each; at the end of its
/// input it exits. As a normal test it does nothing.
#[test]
fn fake_claude_entry() {
    if std::env::var_os(FAKE_CLAUDE).is_none() {
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

/// A running simulated Claude Code.
struct FakeClaude {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeClaude {
    /// Runs `command` in its shell and waits for it.
    fn run(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake claude died"
            );
            // libtest may print its header on the same line.
            if let Some(ok) = line.split_once(DONE).map(|(_, ok)| ok) {
                assert_eq!(ok.trim(), "true", "{command} failed");
                return;
            }
        }
    }

    /// Normal close: its input ends and it exits.
    fn close(mut self) {
        self.stdin.take();
        self.child.wait().unwrap();
    }

    /// Forced close.
    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the
/// folder Claude Code is installed in and the test clock's skew file.
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
        // A hook folder used per command (`git -c core.hooksPath=…`): the
        // repo's hooks are never touched.
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

    fn skew_file(&self) -> PathBuf {
        self.outside.path().join("clock-skew-ms")
    }

    /// Moves the clock of the sessions `ms` forward from the real time.
    fn skew(&self, ms: i64) {
        std::fs::write(self.skew_file(), ms.to_string()).unwrap();
    }

    fn slow_hooks(&self) -> PathBuf {
        self.outside.path().join("slow-hooks")
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
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

    /// The developer, from their own terminal.
    fn developer(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn add(&self) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    /// Installs Claude Code: the simulated binary in the outside folder.
    fn install_claude(&self) -> PathBuf {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        agent
    }

    /// The developer launches Claude Code in `worktree`.
    fn launch_claude(&self, worktree: &Path) -> FakeClaude {
        let agent = self.install_claude();
        let mut child = Command::new(agent)
            .args([
                "fake_claude_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_CLAUDE, "1")
            .env("HOME", &self.f.home)
            .current_dir(worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        FakeClaude {
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

    fn daemon_pid(&self) -> u32 {
        running_pid(&self.dirs().state).unwrap().unwrap()
    }

    /// The sessions `raptor status --json` shows for `worktree`.
    fn status_sessions(&self, worktree: &Path) -> Vec<Value> {
        let status = self.status();
        status["repos"][0]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["path"] == worktree.to_str().unwrap())
            .map(|w| w["sessions"].as_array().unwrap().clone())
            .unwrap_or_default()
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
                start.elapsed() < Duration::from_secs(10),
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
        assert_eq!(value["detection_available"], true);
        value["sessions"].as_array().unwrap().clone()
    }

    fn history(&self) -> Vec<GitEventView> {
        let page: EventsHistoryResult = self
            .client()
            .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
            .unwrap();
        page.events
    }

    /// Waits until the history has an event of `kind` in `worktree`.
    fn event(&self, kind: GitEventKind, worktree: &Path) -> GitEventView {
        let start = Instant::now();
        loop {
            let history = self.history();
            if let Some(e) = history
                .iter()
                .find(|e| e.kind == kind && Path::new(e.worktree.raw()) == worktree)
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no {kind:?} in {}: {history:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
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

fn claude_code_detected() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

/// One detected Claude Code session, in `state`.
fn is_claude(s: &Value, state: &str) -> bool {
    s["agent"] == "claude-code" && s["origin"] == "detected" && s["state"] == state
}

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

/// The hooks and the Git configuration of the repo and its worktrees, as
/// bytes (the repo-intact fingerprint checks them too; this says it
/// explicitly).
fn hooks_and_config(m: &Machine, wt: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let common = m.f.repo.join(".git");
    let mut files = vec![
        common.join("config"),
        common.join("config.worktree"),
        common.join("worktrees/feat-login/config.worktree"),
        wt.join(".git"),
    ];
    for dir in [common.join("hooks"), common.join("gitraptor")] {
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if entry.path().is_dir() {
                    stack.push(entry.path());
                } else {
                    files.push(entry.path());
                }
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap_or_default();
            (p, bytes)
        })
        .collect()
}

// ------------------------------------------------------------ Escenario 1

/// Una sesión de Claude Code se detecta sin registrarla.
#[test]
fn a_claude_code_session_is_detected_without_registering_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Dado ningún registro de agentes.
    assert!(m.status_sessions(&wt).is_empty());

    let mut claude = m.launch_claude(&wt);
    let sessions = m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
    assert_eq!(sessions[0]["agent_name"], Value::Null);
    let out = m.raptor(&["status"], &[]);
    assert!(
        text(&out).contains("Claude Code · Active · detected"),
        "{}",
        text(&out)
    );
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("Claude Code · Activo · detectado"),
        "{}",
        text(&out)
    );

    // Claude Code hace un commit en "feat-login" (with a hook that keeps
    // `git` alive for a second, as a commit with hooks does).
    claude.run(&format!(
        "printf 'user\\npassword\\n' > login.txt && git add login.txt && \
         git -c core.hooksPath={} commit -q -m 'add password'",
        m.slow_hooks().display()
    ));
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");
    let out = m.raptor(&["events"], &[]);
    assert!(
        text(&out).contains("(Claude Code, detected)"),
        "{}",
        text(&out)
    );
    let out = m.raptor(&["events"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("(Claude Code, detectado)"),
        "{}",
        text(&out)
    );
    let repo_id = m.repo_id();
    claude.close();
    m.stop();

    // In the history: the commit points to the session, backed by S3.
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let (store, _) = profile.open_store(&repo_id).unwrap();
    let commit = store
        .events_for_worktree(&wt)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "commit")
        .unwrap();
    assert!(commit.session_id.is_some(), "{commit:#?}");
    assert_eq!(commit.evidence.as_deref(), Some(r#"{"signals":["s3"]}"#));
}

// ------------------------------------------------------------ Escenario 2

/// La sesión pasa a inactiva y vuelve a activa según la actividad.
#[test]
fn the_session_goes_idle_and_back_to_active_with_activity() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Ningún nivel de la configuración define un umbral de inactividad.
    let profile_config = std::fs::read_to_string(m.f.profile.join("config/config.toml")).unwrap();
    assert!(!profile_config.contains("inactiv"), "{profile_config}");
    assert!(!m.f.repo.join(".git/gitraptor").exists());

    let _claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
    let mut subscriber = m.client();
    let _: SubscribeResult = subscriber
        .call(methods::EVENTS_SUBSCRIBE, json!({}))
        .unwrap();

    // Almost 5 minutes: still active.
    m.skew(4 * 60_000 + 50_000);
    std::thread::sleep(Duration::from_millis(2_500));
    assert!(is_claude(&m.status_sessions(&wt)[0], "active"));

    // Cuando pasan 5 minutos sin actividad en "feat-login".
    m.skew(5 * 60_000 + 5_000);
    let inactive = next_session_state(&mut subscriber);
    assert_eq!(inactive.state, SessionStateView::Inactive);
    assert_eq!(Path::new(inactive.worktree.raw()), wt);
    assert!(is_claude(&m.status_sessions(&wt)[0], "inactive"));
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(text(&out).contains("Claude Code · Inactivo · detectado"));

    // Cuando se modifica un archivo en "feat-login".
    std::fs::write(wt.join("login.txt"), "user\nchanged\n").unwrap();
    let active = next_session_state(&mut subscriber);
    assert_eq!(active.state, SessionStateView::Active);
    assert_eq!(active.session_id, inactive.session_id);
    assert!(is_claude(&m.status_sessions(&wt)[0], "active"));
    drop(subscriber);
    m.stop();
}

/// The next `session.state` of the stream.
fn next_session_state(subscriber: &mut Client) -> SessionView {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no session.state");
        let Some(n) = subscriber
            .next_notification(Duration::from_millis(500))
            .unwrap()
        else {
            continue;
        };
        let event = &n.params["event"];
        if event["kind"] == SESSION_STATE {
            assert!(event["timings"].is_object(), "{event}");
            return serde_json::from_value(event["data"].clone()).unwrap();
        }
    }
}

// ------------------------------------------------------------ Escenario 3

/// La sesión termina cuando Claude Code se cierra, y al relanzarlo aparece
/// una sesión nueva. `forced`: el cierre forzado (kill).
fn the_session_ends_when_claude_code_closes(forced: bool) {
    let (m, wt) = observed();
    let claude = m.launch_claude(&wt);
    let first = m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
    let first_id = first[0]["session_id"].clone();

    // Cuando Claude Code se cierra.
    if forced {
        claude.kill();
    } else {
        claude.close();
    }
    let ended = m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "ended"));
    assert_eq!(ended[0]["session_id"], first_id);
    assert_eq!(ended[0]["end_cause"], "process-gone");
    assert!(ended[0]["ended_utc_ms"].is_i64());
    let out = m.raptor(&["status"], &[("LANG", "es_ES.UTF-8")]);
    assert!(
        text(&out).contains("Claude Code · Terminado · detectado · terminó"),
        "{}",
        text(&out)
    );

    // Cuando Claude Code se vuelve a lanzar en "feat-login".
    let _again = m.launch_claude(&wt);
    let now = m.sessions_when(&wt, |s| {
        s.len() == 2 && s.iter().any(|x| is_claude(x, "active"))
    });
    let new = now.iter().find(|x| is_claude(x, "active")).unwrap();
    assert_ne!(new["session_id"], first_id);
    let all = m.all_sessions();
    assert_eq!(all.len(), 2, "{all:#?}");
    let old = all.iter().find(|s| s["session_id"] == first_id).unwrap();
    assert!(is_claude(old, "ended"), "{old}");
    m.stop();
}

#[test]
fn the_session_ends_when_claude_code_closes_normally() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    the_session_ends_when_claude_code_closes(false);
}

#[test]
fn the_session_ends_when_claude_code_is_killed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    the_session_ends_when_claude_code_closes(true);
}

// ------------------------------------------------------------ Escenario 4

/// The repo's hooks before the scenario.
#[derive(Clone, Copy)]
enum Hooks {
    /// Only the samples `git init` leaves.
    None,
    /// Guardrails installed but inactive (ADR-GRD-005): its folder with
    /// dispatchers and manifest, and `core.hooksPath` not pointing to it.
    GuardrailsInactive,
    /// A hook of the developer's own, executable.
    Own,
}

/// La detección funciona sin hooks y el motor no instala ninguno.
fn detection_works_without_hooks_and_installs_none(hooks: Hooks) {
    use std::os::unix::fs::PermissionsExt;
    let (m, wt) = demo();
    let common = m.f.repo.join(".git");
    match hooks {
        Hooks::None => {}
        Hooks::GuardrailsInactive => {
            let dir = common.join("gitraptor/hooks");
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["pre-commit", "pre-push", "reference-transaction"] {
                let hook = dir.join(name);
                std::fs::write(
                    &hook,
                    "#!/bin/sh\nexec raptor guardrails hook \"$0\" \"$@\"\n",
                )
                .unwrap();
                std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            std::fs::write(
                common.join("gitraptor/manifest.json"),
                "{\"version\":1,\"hooks\":[\"pre-commit\",\"pre-push\",\"reference-transaction\"]}\n",
            )
            .unwrap();
        }
        Hooks::Own => {
            let hook = common.join("hooks/pre-commit");
            std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let hooks_path =
        m.f.git_command(&m.f.repo, &["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();
    assert!(!hooks_path.status.success(), "core.hooksPath is set");
    let before = hooks_and_config(&m, &wt);
    let report = check("US-GRP-007 hooks", &m.f, &engine_profile(), || {
        m.add();
        // Cuando el desarrollador lanza Claude Code en "feat-login".
        let claude = m.launch_claude(&wt);
        m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
        claude.close();
        m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "ended"));
        m.stop();
    });
    // Y los hooks de Git y la configuración de Git del repo siguen idénticos.
    assert_eq!(hooks_and_config(&m, &wt), before);
    report.assert_intact();
}

#[test]
fn detection_works_without_hooks_and_installs_none_no_hooks() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    detection_works_without_hooks_and_installs_none(Hooks::None);
}

#[test]
fn detection_works_without_hooks_and_installs_none_guardrails_inactive() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    detection_works_without_hooks_and_installs_none(Hooks::GuardrailsInactive);
}

#[test]
fn detection_works_without_hooks_and_installs_none_own_hook() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    detection_works_without_hooks_and_installs_none(Hooks::Own);
}

// ------------------------------------------------------------ Escenario 5

/// Claude Code instalado después se detecta sin tocar GitRaptor.
#[test]
fn claude_code_installed_later_is_detected_without_touching_gitraptor() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    // Una máquina sin Claude Code instalado.
    assert!(!m.outside.path().join("bin").join(FAKE_AGENT).exists());
    // El estado muestra sus worktrees sin ninguna sesión de agente.
    let status = m.status();
    let worktrees = status["repos"][0]["worktrees"].as_array().unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(worktrees.iter().all(|w| w["sessions"] == json!([])));
    assert_eq!(status["session_detection"], true);
    let out = m.raptor(&["status"], &[]);
    assert!(!text(&out).contains("Claude Code"), "{}", text(&out));
    let pid = m.daemon_pid();
    let config = m.f.profile.join("config");
    let config_before = gitraptor_testkit::fingerprint::Snapshot::of_dir(&config);

    // Cuando el desarrollador instala Claude Code y lo lanza en "feat-login".
    m.install_claude();
    let _claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
    // Sin reinstalar ni reconfigurar GitRaptor.
    assert_eq!(m.daemon_pid(), pid);
    assert_eq!(
        gitraptor_testkit::fingerprint::Snapshot::of_dir(&config),
        config_before
    );
    m.stop();
}

// ------------------------------------------- S3 (ADR-GRP-012, Arquitecto)

/// A quick `git commit -m` of Claude Code may end before the sample: its
/// actor is Claude Code or "unattributed", never anything else. When
/// unattributed, a hint can only name the worktree's one session
/// (amendment of ADR-GRP-012).
#[test]
fn a_quick_claude_code_commit_is_claude_code_or_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch_claude(&wt);
    let sessions = m.sessions_when(&wt, |s| s.len() == 1);
    claude.run("printf 'quick\\n' >> login.txt && git commit -qam quick");
    let e = m.event(GitEventKind::Commit, &wt);
    assert!(
        e.actor == claude_code_detected() || e.actor == Actor::Unattributed,
        "{e:#?}"
    );
    if let Some(hint) = &e.inferred {
        assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
        assert_eq!(hint.session_id, sessions[0]["session_id"], "{e:#?}");
    }
    m.stop();
}

/// The developer commits while Claude Code runs a `git` in the same
/// worktree: two `git`s alive, so the commit stays unattributed
/// (BR-EDGE-004).
#[test]
fn a_developer_commit_while_claude_code_runs_git_is_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1);
    // A `git` of Claude Code that stays alive for a few seconds.
    let started = Instant::now();
    claude.run("(sleep 4; echo) | git cat-file --batch-check > /dev/null 2>&1 &");
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(wt.join("login.txt"), "user\ndeveloper\n").unwrap();
    let hooks = format!("core.hooksPath={}", m.slow_hooks().display());
    m.f.git_in(&wt, &["-c", &hooks, "commit", "-qam", "developer"]);
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
    m.stop();
    // Its `git` outlives the session; once this test's folders are gone,
    // its working folder cannot be read and it would count as foreign for
    // the next test (as it must: ADR-GRP-012, rule 6).
    std::thread::sleep(Duration::from_secs(5).saturating_sub(started.elapsed()));
}

// ------------------------------------------------------------ Repo intacto

/// Detecting sessions does not modify the repo: zero differences outside
/// the profile (INF-GRP-001).
#[test]
fn repo_intact_detecting_sessions_does_not_modify_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo();
    std::fs::write(wt.join("login.txt"), "user\nchanged\n").unwrap();
    m.f.write("untracked.txt", "u\n");
    let report = check("US-GRP-007 sessions", &m.f, &engine_profile(), || {
        m.add();
        let claude = m.launch_claude(&wt);
        m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "active"));
        let out = m.raptor(&["sessions", "--all"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        claude.kill();
        m.sessions_when(&wt, |s| s.len() == 1 && is_claude(&s[0], "ended"));
        let out = m.developer(&["repo", "retire", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
    });
    report.assert_intact();
}
