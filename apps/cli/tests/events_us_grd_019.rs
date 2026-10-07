//! US-GRD-019 end to end (DS-US-GRD-018, PR-B): `raptor events` shows who ran each commit and
//! whose name it went in under when they differ, and checks the `inferred` hint against the
//! trailer. Real `raptor` as daemon and client, temporary repo and profile (NFR-01), Claude Code
//! simulated by `raptor-fake-agent` as in `claude_sessions.rs` (its harness is copied here).
//!
//! macOS only, like `claude_sessions.rs`. Linux and Windows: Pendiente: etapa de validación
//! multiplataforma.
#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, Snapshot, SubscribeResult,
    TrailerCheck,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
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

// ------------------------------------------------------------ US-GRD-019

const TRAILER: &str = "Co-Authored-By: Claude <noreply@anthropic.com>";
const ANA: [&str; 4] = [
    "-c",
    "user.name=Ana Pérez",
    "-c",
    "user.email=ana@example.com",
];
/// The subject of every commit message: it must never reach the store or the wire.
const SECRET: &str = "secret-subject-7f3a";

impl Machine {
    /// `raptor events` in `lang`.
    fn events_text(&self, lang: &str) -> String {
        let out = self.raptor(&["events"], &[("LANG", lang)]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }

    fn events_json(&self) -> Value {
        let out = self.raptor(&["events", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// A commit of the index of `worktree` by a `git` that runs outside the repo: no sample sees
    /// it in the repo, so S3 has no sighting and the event takes the worktree's only session as
    /// a hint (amendment of ADR-GRP-012).
    fn commit_from_outside(&self, worktree: &Path, message: &[&str]) {
        let admin = self
            .f
            .git_in(worktree, &["rev-parse", "--absolute-git-dir"]);
        let mut args: Vec<&str> = ANA.to_vec();
        args.extend([
            "-c",
            "core.hooksPath=/dev/null",
            "--git-dir",
            admin.trim(),
            "commit",
            "-q",
        ]);
        for m in message {
            args.extend(["-m", m]);
        }
        self.f.git_in(self.outside.path(), &args);
    }

    /// The stored rows of the repo, after the engine stopped.
    fn stored_authorship(&self, repo_id: &str) -> Vec<String> {
        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let (store, _) = profile.open_store(repo_id).unwrap();
        store
            .events_in_range(0, i64::MAX)
            .unwrap()
            .into_iter()
            .filter_map(|e| e.authorship)
            .collect()
    }
}

fn wt_name(wt: &Path) -> String {
    wt.file_name().unwrap().to_string_lossy().into_owned()
}

/// "commit by Ana Pérez with Claude Code · feat-x" (en/es); `--json` keeps the actor and the
/// authorship apart. The message never reaches the store or the wire (ajuste 1).
#[test]
fn agent_commit_shows_both() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1);
    claude.run(&format!(
        "printf 'x\\n' >> login.txt && git -c 'user.name=Ana Pérez' -c user.email=ana@example.com \
         -c core.hooksPath={} commit -qam '{SECRET}' -m '{TRAILER}'",
        m.slow_hooks().display()
    ));
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");
    let a = e.authorship.as_ref().expect("authorship");
    assert_eq!(a.author.name.raw(), "Ana Pérez");
    assert_eq!(a.committer.email.raw(), "ana@example.com");
    assert_eq!(a.coauthors.len(), 1);
    assert_eq!(a.coauthors[0].agent, Some(AgentKind::ClaudeCode));
    let name = wt_name(&wt);
    let en = m.events_text("en_US.UTF-8");
    assert!(
        en.contains(&format!("commit by Ana Pérez with Claude Code · {name}")),
        "{en}"
    );
    let es = m.events_text("es_ES.UTF-8");
    assert!(
        es.contains(&format!("commit de Ana Pérez con Claude Code · {name}")),
        "{es}"
    );
    assert!(!es.contains("ejecutado por"), "{es}");
    let json = m.events_json();
    let row = json
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "commit" && r["worktree"] == wt.to_str().unwrap())
        .unwrap()
        .clone();
    assert_eq!(row["actor"]["kind"], "claude-code", "{row}");
    assert_eq!(row["authorship"]["author"]["name"], "Ana Pérez", "{row}");
    assert_eq!(
        row["authorship"]["committer"]["email"], "ana@example.com",
        "{row}"
    );
    assert_eq!(
        row["authorship"]["coauthors"][0]["agent"], "claude-code",
        "{row}"
    );
    assert!(!json.to_string().contains(SECRET), "{json}");
    let repo_id = m.repo_id();
    claude.close();
    m.stop();
    let stored = m.stored_authorship(&repo_id);
    assert!(!stored.is_empty());
    assert!(stored.iter().all(|s| !s.contains(SECRET)), "{stored:?}");
}

/// Without an agent: "commit by Ana Pérez · main" and the actor "unattributed".
#[test]
fn an_unattributed_commit_does_not_repeat_the_author() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    std::fs::write(wt.join("login.txt"), "user\ndeveloper\n").unwrap();
    let mut args = ANA.to_vec();
    args.extend(["commit", "-qam", SECRET]);
    m.f.git_in(&wt, &args);
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
    let name = wt_name(&wt);
    let en = m.events_text("en_US.UTF-8");
    let line = en
        .lines()
        .find(|l| l.contains("commit by"))
        .unwrap_or_default();
    assert!(
        line.contains(&format!("commit by Ana Pérez · {name}")),
        "{en}"
    );
    assert!(line.contains("(unattributed)"), "{en}");
    assert!(!line.contains(" with "), "{en}");
    let es = m.events_text("es_ES.UTF-8");
    assert!(
        es.contains(&format!("commit de Ana Pérez · {name}")),
        "{es}"
    );
    m.stop();
}

/// The hint checked against the trailer: confirmed with Claude Code's, unconfirmed without one;
/// the actor stays "unattributed". (A contradicting trailer drops the hint: the table has one
/// agent today, so that row is a unit test in `crates/core/src/daemon/authorship.rs`.)
#[test]
fn the_inferred_hint_is_checked_against_the_trailer() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (message, expected, en, es) in [
        (
            vec![SECRET, TRAILER],
            TrailerCheck::Confirmed,
            "inferred: Claude Code (confirmed by the trailer)",
            "inferido: Claude Code (confirmado por el trailer)",
        ),
        (
            vec![SECRET],
            TrailerCheck::Unconfirmed,
            "inferred: Claude Code (not confirmed by the trailer)",
            "inferido: Claude Code (no confirmado por el trailer)",
        ),
    ] {
        let (m, wt) = observed();
        std::fs::write(wt.join("login.txt"), "user\nhint\n").unwrap();
        m.f.git_in(&wt, &["add", "login.txt"]);
        let _claude = m.launch_claude(&wt);
        m.sessions_when(&wt, |s| s.len() == 1);
        m.commit_from_outside(&wt, &message);
        let e = m.event(GitEventKind::Commit, &wt);
        assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
        let hint = e.inferred.as_ref().expect("hint");
        assert_eq!(hint.trailer, Some(expected), "{e:#?}");
        let out = m.events_text("en_US.UTF-8");
        assert!(out.contains(en), "{out}");
        let out = m.events_text("es_ES.UTF-8");
        assert!(out.contains(es), "{out}");
        m.stop();
    }
}

/// With `human-author` the commit records no hint; changing the policy afterwards does not
/// rewrite the event.
#[test]
fn human_author_records_no_hint() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let settings = m.dirs().config.join("settings.json");
    std::fs::write(
        &settings,
        r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#,
    )
    .unwrap();
    std::fs::write(wt.join("login.txt"), "user\nhuman\n").unwrap();
    m.f.git_in(&wt, &["add", "login.txt"]);
    let _claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1);
    m.commit_from_outside(&wt, &[SECRET, TRAILER]);
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
    assert_eq!(e.inferred, None, "{e:#?}");
    let out = m.events_text("en_US.UTF-8");
    assert!(!out.contains("inferred:"), "{out}");
    // The policy goes back to the default: the stored event keeps no hint.
    std::fs::remove_file(&settings).unwrap();
    let again = m.event(GitEventKind::Commit, &wt);
    assert_eq!(again.inferred, None, "{again:#?}");
    m.stop();
}

/// An agent's commit without its trailer shows the difference: run by Claude Code, in the name
/// of Ana, no trailer. Only the presentation is measured: the commit goes through because the
/// command's hook folder replaces the repo's (no guardrails hook runs, as with `flexible`).
#[test]
fn an_agent_commit_without_trailer_shows_the_difference() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut claude = m.launch_claude(&wt);
    m.sessions_when(&wt, |s| s.len() == 1);
    claude.run(&format!(
        "printf 'y\\n' >> login.txt && git -c 'user.name=Ana Pérez' -c user.email=ana@example.com \
         -c core.hooksPath={} commit -qam '{SECRET}'",
        m.slow_hooks().display()
    ));
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");
    let name = wt_name(&wt);
    let en = m.events_text("en_US.UTF-8");
    assert!(
        en.contains(&format!(
            "commit by Ana Pérez · {name} (feat-login) · run by Claude Code · no trailer"
        )),
        "{en}"
    );
    let es = m.events_text("es_ES.UTF-8");
    assert!(
        es.contains(&format!(
            "commit de Ana Pérez · {name} (feat-login) · ejecutado por Claude Code · sin trailer"
        )),
        "{es}"
    );
    claude.close();
    m.stop();
}

/// `raptor-mcp` never sees names or emails (ajuste 2): it does not ask for `events.authorship`,
/// its stream carries no `git.event` and `events.history` is not offered to it. The shaping of
/// a connection without the capability is `channel::bus::tests::git_events_carry_authorship_*`.
#[test]
fn an_mcp_connection_gets_no_authorship() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed();
    let mut mcp = Client::connect(&m.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let _: SubscribeResult = mcp.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    std::fs::write(wt.join("login.txt"), "user\nmcp\n").unwrap();
    let mut args = ANA.to_vec();
    args.extend(["commit", "-qam", SECRET]);
    m.f.git_in(&wt, &args);
    // The CLI does see it, with its authorship.
    let e = m.event(GitEventKind::Commit, &wt);
    assert!(e.authorship.is_some(), "{e:#?}");
    let history: Result<EventsHistoryResult, _> =
        mcp.call(methods::EVENTS_HISTORY, json!({"repo_id": m.repo_id()}));
    assert!(history.is_err(), "{history:?}");
    while let Some(n) = mcp.next_notification(Duration::from_millis(300)).unwrap() {
        assert!(!n.params.to_string().contains("Ana"), "{}", n.params);
    }
    m.stop();
}
