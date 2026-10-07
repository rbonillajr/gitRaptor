//! US-GRP-004 end to end: each Gherkin scenario, and the autostart of
//! `raptor daemon enable` and `disable` (PQ-1, ADR-GRP-005 § 3, SEC-14),
//! with the real `raptor` binary as daemon and as client, over a temporary
//! machine built by the "intact repo" harness (INF-GRP-001): temporary repo,
//! profile and home, never this repo, the real profile nor the real
//! `~/Library/LaunchAgents` (NFR-01).
//!
//! launchd is simulated: `launchctl` is a script ([`SERVICE_TOOL_ENV`]) that
//! records its arguments and, for `bootstrap` and `kickstart`, runs exactly
//! what the plist written by `enable` says (its `ProgramArguments` and its
//! `EnvironmentVariables`) through [`fake_launchd_entry`]. No fixed waits:
//! the tests wait for explicit signals (the engine's `daemon_started` log
//! line, the history, the state of a worktree) with a deadline.
//!
//! macOS only: `script` options and launchd are the macOS ones. Linux and
//! Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, Snapshot,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, UntrustedName, methods};
use gitraptor_core::autostart::{AUTOSTART_DIR_ENV, SERVICE_TOOL_ENV};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const FAKE_LAUNCHD: &str = "RAPTOR_FAKE_LAUNCHD";
/// Printed by the simulated Claude Code after each command.
const DONE: &str = "<<raptor-fake-claude-done>>";
/// The plist, relative to the fixture's home.
const PLIST: &str = "Library/LaunchAgents/dev.gitraptor.plist";
/// How long a signal may take. A deadline, never a wait.
const DEADLINE: Duration = Duration::from_secs(45);
/// Slack between the end of a Git command and the moment the engine records
/// it: the NFR-04 engine budget (300 ms) plus the macOS runner's late
/// timers (INF-GRP-002). The assertion is "at the time it happened", not a
/// freshness gate.
const OBSERVED_WITHIN_MS: i64 = 2_000;

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

// ------------------------------------------------------------ Simulators

/// Entry point of the simulated Claude Code: when this test binary runs as
/// `raptor-fake-agent` with `RAPTOR_FAKE_CLAUDE`, it runs every line of its
/// input with `/bin/sh` and prints [`DONE`] after each. As a normal test it
/// does nothing.
#[test]
fn fake_claude_entry() {
    if std::env::var_os(FAKE_CLAUDE).is_none() {
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(&line).status();
        println!("{DONE} {}", status.map(|s| s.success()).unwrap_or(false));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

/// Entry point of the simulated launchd: with `RAPTOR_FAKE_LAUNCHD` set to
/// a plist, starts its `ProgramArguments` detached, with launchd's minimal
/// environment plus the plist's `EnvironmentVariables`, in a process
/// group of its own, and exits. As a
/// normal test it does nothing.
#[test]
fn fake_launchd_entry() {
    let Some(plist) = std::env::var_os(FAKE_LAUNCHD) else {
        return;
    };
    use std::os::unix::process::CommandExt;
    let (args, env) = parse_plist(&std::fs::read_to_string(plist).unwrap());
    let mut cmd = Command::new(&args[0]);
    // Outside the caller's terminal session, as launchd's jobs are: the
    // developer's pty closing must not reach it.
    cmd.process_group(0);
    cmd.args(&args[1..])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .envs(std::env::var_os("HOME").map(|h| ("HOME", h)))
        .envs(env)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().unwrap();
    std::process::exit(0);
}

/// `ProgramArguments` and `EnvironmentVariables` of a plist written by
/// `raptor daemon enable`.
fn parse_plist(text: &str) -> (Vec<String>, Vec<(String, String)>) {
    fn unescape(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
    }
    fn tags<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        let mut out = Vec::new();
        let mut rest = text;
        while let Some((_, after)) = rest.split_once(open.as_str()) {
            let (inner, tail) = after.split_once(close.as_str()).unwrap();
            out.push(inner);
            rest = tail;
        }
        out
    }
    let program = text
        .split_once("<key>ProgramArguments</key>")
        .unwrap()
        .1
        .split_once("</array>")
        .unwrap()
        .0;
    let args = tags(program, "string").into_iter().map(unescape).collect();
    let env = match text.split_once("<key>EnvironmentVariables</key>") {
        Some((_, after)) => {
            let dict = after.split_once("</dict>").unwrap().0;
            tags(dict, "key")
                .into_iter()
                .zip(tags(dict, "string"))
                .map(|(k, v)| (unescape(k), unescape(v)))
                .collect()
        }
        None => Vec::new(),
    };
    (args, env)
}

/// A running simulated Claude Code.
struct FakeClaude {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeClaude {
    /// Hands `command` to its shell without waiting for it.
    fn send(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
    }

    /// Waits until the command sent last ends.
    fn done(&mut self, command: &str) {
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake claude died"
            );
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
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------ Machine

/// The temporary machine, plus what lives outside the fixture's root: the
/// folder Claude Code is installed in, the simulated `launchctl` and its
/// log, and the sessions' clock skew.
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
        // macOS always has it; `enable` creates it if missing.
        std::fs::create_dir_all(f.home.join("Library/LaunchAgents")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        let m = Self { f, outside };
        // A hook folder used per command (`git -c core.hooksPath=…`), the
        // repo's hooks untouched: `post-commit` keeps the `git` alive until
        // the test releases it, so the S3 sample always sees it (the S3
        // race accepted by ADR-GRP-012; US-GRP-007 Dev Spec). Bounded by
        // the deadline, so a failed test leaves no `git` behind.
        std::fs::create_dir(m.held_hooks()).unwrap();
        let post_commit = m.held_hooks().join("post-commit");
        std::fs::write(
            &post_commit,
            format!(
                "#!/bin/sh\n\
                 n=0\n\
                 while [ ! -e '{release}' ] && [ $n -lt {polls} ]; do sleep 0.05; n=$((n+1)); done\n",
                release = m.release_file().display(),
                polls = DEADLINE.as_millis() / 50,
            ),
        )
        .unwrap();
        std::fs::set_permissions(&post_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tool = m.launchctl();
        std::fs::write(
            &tool,
            format!(
                "#!/bin/sh\n\
                 echo \"$*\" >> '{log}'\n\
                 case \"$1\" in\n\
                 bootstrap|kickstart)\n\
                 \x20 [ -f '{plist}' ] || exit 1\n\
                 \x20 HOME='{home}' {FAKE_LAUNCHD}='{plist}' exec '{bin}' fake_launchd_entry \
                 --exact --nocapture --test-threads=1 >/dev/null 2>&1 ;;\n\
                 print) [ -f '{plist}' ] ;;\n\
                 esac\n",
                log = m.launchctl_log().display(),
                plist = m.plist().display(),
                home = std::env::var("HOME").unwrap(),
                bin = std::env::current_exe().unwrap().display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn plist(&self) -> PathBuf {
        self.f.home.join(PLIST)
    }

    fn launchctl(&self) -> PathBuf {
        self.outside.path().join("launchctl")
    }

    fn launchctl_log(&self) -> PathBuf {
        self.outside.path().join("launchctl.log")
    }

    /// What `launchctl` was asked, one call per line.
    fn launchctl_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.launchctl_log())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn skew_file(&self) -> PathBuf {
        self.outside.path().join("clock-skew-ms")
    }

    fn held_hooks(&self) -> PathBuf {
        self.outside.path().join("held-hooks")
    }

    /// Its existence lets the held `post-commit` end.
    fn release_file(&self) -> PathBuf {
        self.outside.path().join("release")
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
            (
                AUTOSTART_DIR_ENV,
                self.f.home.join("Library/LaunchAgents").into_os_string(),
            ),
            (SERVICE_TOOL_ENV, self.launchctl().into_os_string()),
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

    fn enable(&self) -> Output {
        self.developer(&["daemon", "enable"])
    }

    fn launch_claude(&self, worktree: &Path) -> FakeClaude {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
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
        let out = self.raptor(&["status", "--json"]);
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

    /// The repo's whole history, oldest first.
    fn history(&self) -> Vec<GitEventView> {
        let page: EventsHistoryResult = self
            .client()
            .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
            .unwrap();
        page.events
    }

    /// Waits until the history has `n` commits in `worktree`; returns them.
    fn commits(&self, worktree: &Path, n: usize) -> Vec<GitEventView> {
        let start = Instant::now();
        loop {
            let history = self.history();
            let commits: Vec<GitEventView> = history
                .iter()
                .filter(|e| {
                    e.kind == GitEventKind::Commit && Path::new(e.worktree.raw()) == worktree
                })
                .cloned()
                .collect();
            if commits.len() >= n {
                return commits;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "{} commits in {}: {history:#?}",
                commits.len(),
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The worktree at `path` in `status --json`, once `ok` holds on it.
    fn worktree_when(&self, path: &Path, ok: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let status = self.status();
            let wt = status["repos"][0]["worktrees"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["path"] == path.to_str().unwrap())
                .cloned()
                .unwrap_or(Value::Null);
            if ok(&wt) {
                return wt;
            }
            assert!(start.elapsed() < DEADLINE, "{}: {status}", path.display());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Signal that the observer watches `worktree`: a change shows in its
    /// state, and its removal too.
    fn settled(&self, worktree: &Path) {
        let probe = worktree.join("probe.txt");
        std::fs::write(&probe, "probe\n").unwrap();
        self.worktree_when(worktree, |w| w["clean"] == false);
        std::fs::remove_file(&probe).unwrap();
        self.worktree_when(worktree, |w| w["clean"] == true);
    }

    /// The sessions `raptor status --json` shows for `worktree`.
    fn status_sessions(&self, worktree: &Path) -> Vec<Value> {
        self.status()["repos"][0]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["path"] == worktree.to_str().unwrap())
            .map(|w| w["sessions"].as_array().unwrap().clone())
            .unwrap_or_default()
    }

    fn sessions_when(&self, worktree: &Path, ok: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let start = Instant::now();
        loop {
            let sessions = self.status_sessions(worktree);
            if ok(&sessions) {
                return sessions;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "{}: {sessions:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `raptor sessions --all --json`, sorted by id.
    fn all_sessions(&self) -> Vec<Value> {
        let out = self.raptor(&["sessions", "--all", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        let mut sessions = value["sessions"].as_array().unwrap().clone();
        sessions.sort_by_key(|s| s["session_id"].to_string());
        sessions
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs().state.join("daemon.log")).unwrap_or_default()
    }

    /// Waits until the engine logged its `n`th start, then returns its pid.
    fn started(&self, n: usize) -> u32 {
        let start = Instant::now();
        loop {
            if self.log().matches("daemon_started").count() >= n
                && let Some(pid) = running_pid(&self.dirs().state).unwrap()
            {
                return pid;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "start {n}: {}\nlaunchctl: {:?}",
                self.log(),
                self.launchctl_calls()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn pid(&self) -> Option<u32> {
        running_pid(&self.dirs().state).unwrap()
    }

    /// Waits until no engine holds the instance lock.
    fn wait_stopped(&self) {
        let start = Instant::now();
        while self.pid().is_some() {
            assert!(start.elapsed() < DEADLINE, "the engine did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The developer stops the engine in order.
    fn stop(&self) {
        if self.pid().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        self.wait_stopped();
    }

    /// The engine dies without stopping in order.
    fn kill(&self) {
        let pid = self.pid().unwrap();
        let out = Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        self.wait_stopped();
    }

    /// What launchd does at login, or after a crash with
    /// `KeepAlive.SuccessfulExit = false`: start the job of the plist.
    fn launchd_starts_the_job(&self) {
        let out = Command::new(self.launchctl())
            .args(["kickstart", "gui/launchd"])
            .env_clear()
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
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

fn wall_ms() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since.as_millis()).unwrap()
}

fn uid() -> String {
    let out = Command::new("/usr/bin/id").arg("-u").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// "demo": `main` with `login.txt`, branches `feat-login` and `feat-api`
/// and their worktrees `<root>/wt-feat-login` and `<root>/wt-feat-api`.
fn demo() -> (Machine, PathBuf, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    f.git(&["branch", "feat-api"]);
    let login = f
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();
    let api = f
        .add_worktree("feat-api", "feat-api")
        .canonicalize()
        .unwrap();
    (Machine::new(f), login, api)
}

/// `demo` observed, with the observer watching "feat-login".
fn observed() -> (Machine, PathBuf, PathBuf) {
    let (m, login, api) = demo();
    m.add();
    m.settled(&login);
    (m, login, api)
}

/// Commits in `worktree` with no surface involved; returns the wall-clock
/// interval of the Git command.
fn commit(m: &Machine, worktree: &Path, line: &str) -> (i64, i64) {
    let before = wall_ms();
    let path = worktree.join("login.txt");
    let mut content = std::fs::read_to_string(&path).unwrap();
    content.push_str(line);
    content.push('\n');
    std::fs::write(&path, content).unwrap();
    m.f.git_in(worktree, &["commit", "-qam", line]);
    (before, wall_ms())
}

fn other_agent(name: &str) -> Actor {
    Actor::Agent {
        kind: AgentKind::Other,
        name: Some(UntrustedName::new(name)),
        origin: AgentOrigin::Registered,
    }
}

fn claude_code_detected() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn assert_observed_at(e: &GitEventView, (before, after): (i64, i64)) {
    assert!(!e.details.worktree_inferred, "{e:#?}");
    assert!(
        e.observed_utc_ms >= before && e.observed_utc_ms <= after + OBSERVED_WITHIN_MS,
        "observed at {}, the commit ran in [{before}, {after}]: {e:#?}",
        e.observed_utc_ms
    );
}

// ------------------------------------------------------------ Escenario 1

/// La actividad se captura sin ninguna superficie abierta: the engine was
/// started by `raptor repo add`, which exited; no TUI, CLI nor MCP is open
/// while the two commits happen.
#[test]
fn activity_is_captured_with_no_surface_open() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, login, _api) = observed();
    let pid = m.pid().unwrap();

    // Cuando se hacen 2 commits en el worktree "feat-login".
    let first = commit(&m, &login, "first");
    let second = commit(&m, &login, "second");

    // Y después se consulta el estado del motor.
    let commits = m.commits(&login, 2);
    assert_eq!(commits.len(), 2, "{commits:#?}");
    assert_eq!(m.pid(), Some(pid), "the same engine observed them");
    // Entonces el historial contiene los 2 commits en "feat-login" con la
    // fecha y hora en que ocurrieron: recorded before anyone asked.
    assert_observed_at(&commits[0], first);
    assert_observed_at(&commits[1], second);
    for c in &commits {
        assert_eq!(c.details.branch.as_ref().unwrap().raw(), "feat-login");
    }
    let out = m.raptor(&["events"]);
    assert_eq!(
        // With its declared authorship (US-GRD-019): "commit by Test · wt-feat-login (feat-login)".
        text(&out).matches("(feat-login)  (no agent)").count(),
        2,
        "{}",
        text(&out)
    );
    m.stop();
}

// ------------------------------------------------------------ Escenario 2

/// Lo observado sobrevive a que el motor deje de ejecutarse y vuelva a
/// arrancar. `crash`: the engine is killed instead of stopped in order.
fn what_was_observed_survives_a_restart(crash: bool) {
    let (m, login, api) = observed();
    // Dado "otro agente: Codex" registrado en "feat-login".
    m.register("Codex", &login);
    let codex = commit(&m, &login, "codex");
    // Y una sesión terminada de "Claude Code" en "feat-api".
    m.settled(&api);
    let mut claude = m.launch_claude(&api);
    m.sessions_when(&api, |s| {
        s.iter()
            .any(|x| x["agent"] == "claude-code" && x["state"] == "active")
    });
    // Its `git` lives until the engine recorded the commit.
    let command = format!(
        "printf 'user\\nclaude\\n' > login.txt && git -c core.hooksPath='{}' commit -qam claude",
        m.held_hooks().display()
    );
    claude.send(&command);
    m.commits(&api, 1);
    std::fs::write(m.release_file(), "").unwrap();
    claude.done(&command);
    claude.close();
    m.sessions_when(&api, |s| {
        s.len() == 1 && s[0]["agent"] == "claude-code" && s[0]["state"] == "ended"
    });
    // Dado un historial de eventos atribuidos.
    let by_codex = m.commits(&login, 1).remove(0);
    assert_observed_at(&by_codex, codex);
    assert_eq!(by_codex.actor, other_agent("Codex"));
    let by_claude = m.commits(&api, 1).remove(0);
    assert_eq!(by_claude.actor, claude_code_detected());
    let history = m.history();
    let sessions = m.all_sessions();

    // Cuando el motor deja de ejecutarse y vuelve a arrancar.
    let pid = m.pid().unwrap();
    if crash {
        m.kill();
    } else {
        m.stop();
    }
    assert!(m.status()["repos"][0]["worktrees"].is_array());
    assert_ne!(m.pid(), Some(pid), "a new engine");

    // Entonces el historial de eventos y sus atribuciones son los mismos.
    assert_eq!(m.history(), history);
    // Y "otro agente: Codex" sigue registrado en "feat-login".
    let at_login = m.status_sessions(&login);
    assert!(
        at_login.iter().any(|s| s["agent"] == "other"
            && s["agent_name"] == "Codex"
            && s["origin"] == "registered"
            && s["state"] != "ended"),
        "{at_login:#?}"
    );
    // Y la sesión de "Claude Code" en "feat-api" sigue en "Terminado".
    let after = m.all_sessions();
    let claude_before = sessions
        .iter()
        .find(|s| s["agent"] == "claude-code")
        .unwrap();
    let claude_after = after
        .iter()
        .find(|s| s["session_id"] == claude_before["session_id"])
        .unwrap();
    assert_eq!(claude_after["state"], "ended", "{claude_after:#?}");
    assert_eq!(claude_after["end_cause"], claude_before["end_cause"]);
    assert_eq!(claude_after["ended_utc_ms"], claude_before["ended_utc_ms"]);
    let out = m.raptor(&["status"]);
    assert!(
        text(&out).contains("another agent: Codex"),
        "{}",
        text(&out)
    );
    m.stop();
}

#[test]
fn what_was_observed_survives_the_engine_stopping_and_starting_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    what_was_observed_survives_a_restart(false);
}

#[test]
fn what_was_observed_survives_the_engine_dying_and_starting_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    what_was_observed_survives_a_restart(true);
}

// ------------------------------------------------------------ Escenario 3

/// La observación se reanuda sola al volver a arrancar el motor: with the
/// autostart enabled, the engine dies and launchd starts the job of the
/// plist again; nobody opens a surface before the commit.
#[test]
fn observation_resumes_by_itself_when_the_engine_starts_again() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, login, _api) = demo();
    // Dado el repo "demo" observado, by the engine launchd started.
    let out = m.enable();
    assert!(out.status.success(), "{}", text(&out));
    let pid = m.started(1);
    m.add();
    m.settled(&login);
    assert_eq!(m.pid(), Some(pid));

    // Cuando el motor deja de ejecutarse y vuelve a arrancar sin que el
    // desarrollador abra ninguna superficie de GitRaptor: it dies, and
    // launchd starts the job again (KeepAlive.SuccessfulExit = false).
    m.kill();
    m.launchd_starts_the_job();
    let pid_again = m.started(2);
    assert_ne!(pid_again, pid);
    let args = Command::new("/bin/ps")
        .args(["-o", "args=", "-p", &pid_again.to_string()])
        .output()
        .unwrap();
    assert!(
        text(&args).contains(&format!("{RAPTOR} daemon --autostart")),
        "{}",
        text(&args)
    );

    // Y después se hace un commit en "feat-login".
    let when = commit(&m, &login, "after restart");

    // Entonces el commit aparece en el historial de eventos de "feat-login",
    // recorded by that engine when it happened.
    let commits = m.commits(&login, 1);
    assert_observed_at(commits.last().unwrap(), when);
    assert_eq!(m.pid(), Some(pid_again), "no client started another engine");
    m.stop();
}

// ------------------------------------------------------------ Autostart (PQ-1)

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

/// `enable` and `disable` create and remove exactly the plist of ADR-GRP-005
/// § 3 and nothing else outside the profile; the repo stays byte for byte
/// (NFR-01, Validación 11, INF-GRP-001).
#[test]
fn repo_intact_enable_and_disable_change_only_the_plist() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    let ex = Exceptions::autostart(&[("home", Path::new(PLIST))]).and(engine_profile());
    let report = check("US-GRP-004 enable", &m.f, &ex, || {
        let out = m.enable();
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("autostart at login is on"),
            "{}",
            text(&out)
        );
        m.started(1);
    });
    report.assert_intact();
    assert!(m.plist().is_file());
    let report = check("US-GRP-004 disable", &m.f, &ex, || {
        let out = m.developer(&["daemon", "disable"]);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
    });
    report.assert_intact();
    assert!(!m.plist().exists());
    assert_eq!(
        std::fs::read_dir(m.f.home.join("Library/LaunchAgents"))
            .unwrap()
            .count(),
        0
    );
}

/// The plist runs this binary with `daemon --autostart`, and `enable`
/// hands it to launchd for this session; a second `enable` changes nothing.
#[test]
fn enable_registers_this_binary_with_launchd() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    let out = m.enable();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("also in this session"),
        "{}",
        text(&out)
    );
    let plist = std::fs::read_to_string(m.plist()).unwrap();
    let (args, _env) = parse_plist(&plist);
    assert_eq!(args, [RAPTOR, "daemon", "--autostart"]);
    assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(m.plist()).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(
        m.launchctl_calls(),
        [format!("bootstrap gui/{} {}", uid(), m.plist().display())]
    );
    m.started(1);

    let out = m.developer(&["daemon", "enable"]);
    assert!(
        text(&out).contains("autostart at login was already on"),
        "{}",
        text(&out)
    );
    // Spanish.
    let out = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null", RAPTOR, "daemon", "enable"])
        .env_clear()
        .envs(m.env())
        .env("LANG", "es_ES.UTF-8")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        text(&out).contains("el autoarranque al iniciar sesión ya estaba activado"),
        "{}",
        text(&out)
    );
    m.stop();
}

/// SEC-14: `enable` from a temporary copy (as npx leaves it) is refused and
/// writes nothing.
#[test]
fn enable_from_a_temporary_copy_is_refused() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    let copy = tempfile::tempdir().unwrap();
    let exe = copy.path().join("raptor");
    std::fs::copy(RAPTOR, &exe).unwrap();
    let out = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null", exe.to_str().unwrap(), "daemon", "enable"])
        .env_clear()
        .envs(m.env())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("is not an installed GitRaptor"),
        "{}",
        text(&out)
    );
    assert!(!m.plist().exists());
    assert!(m.launchctl_calls().is_empty());
}

/// `raptor daemon status` says whether the autostart is registered, and
/// `disable` does not stop the running engine.
#[test]
fn status_shows_the_autostart_and_disable_keeps_the_engine_running() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    let shown = |m: &Machine| text(&m.raptor(&["daemon", "status"]));
    assert!(
        shown(&m).contains("autostart at login: not registered (raptor daemon enable)"),
        "{}",
        shown(&m)
    );
    let pid = m.pid().unwrap();
    let out = m.enable();
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        shown(&m).contains("autostart at login: registered"),
        "{}",
        shown(&m)
    );

    let out = m.developer(&["daemon", "disable"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("the running engine keeps running"),
        "{}",
        text(&out)
    );
    assert!(shown(&m).contains("not registered"), "{}", shown(&m));
    assert_eq!(m.pid(), Some(pid), "disable did not stop the engine");
    let out = m.developer(&["daemon", "disable"]);
    assert!(
        text(&out).contains("autostart at login was already off"),
        "{}",
        text(&out)
    );
    m.stop();
}

/// With the autostart registered, a client that finds no engine asks
/// launchd to start the job (never `kickstart -k`), so the engine gets
/// launchd's environment instead of the client's.
#[test]
fn a_client_starts_the_engine_through_launchd() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    let out = m.enable();
    assert!(out.status.success(), "{}", text(&out));
    m.started(1);
    m.stop();

    assert!(m.status()["repos"].is_array());
    let calls = m.launchctl_calls();
    assert_eq!(
        calls.last().unwrap(),
        &format!("kickstart gui/{}/dev.gitraptor", uid()),
        "{calls:?}"
    );
    let pid = m.pid().unwrap();
    let args = Command::new("/bin/ps")
        .args(["-o", "args=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert!(
        text(&args).contains("daemon --autostart"),
        "{}",
        text(&args)
    );
    m.stop();
}

/// A second engine started by launchd while one runs exits with 0, so
/// `KeepAlive.SuccessfulExit = false` does not relaunch it in a loop; started
/// by hand it keeps exit code 3 (TS-GRP-003).
#[test]
fn a_second_engine_started_by_launchd_exits_with_zero() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _login, _api) = demo();
    assert!(m.status()["repos"].is_array());
    let pid = m.pid().unwrap();
    let out = m.raptor(&["daemon", "--autostart"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = m.raptor(&["daemon"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert_eq!(m.pid(), Some(pid));
    m.stop();
}
