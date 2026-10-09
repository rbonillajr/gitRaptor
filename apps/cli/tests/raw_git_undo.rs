//! Exit criterion 3 of M1, end to end (US-TMC-004): an agent that throws
//! away uncommitted work with raw `git reset --hard` gets it back with
//! `raptor undo`. The real `raptor` binary as daemon and as client, over a
//! temporary machine built by the "intact repo" harness (INF-GRP-001):
//! temporary repo, home and profile, never this repo nor the real profile
//! (NFR-01).
//!
//! Claude Code is simulated as in US-GRP-007: a copy of this test binary
//! named `raptor-fake-agent` (the only agent executable the debug daemon
//! knows), launched in the worktree, that runs each line of its input with
//! `/bin/sh`, so its `git` and its `raptor undo` descend from it. Its `git
//! reset` runs with a `reference-transaction` hook that keeps `git` alive a
//! moment, as the slow hook of US-GRP-007 does for a commit, so the engine's
//! sample sees it and attributes the reset to the session. The developer
//! runs under a pty (`script`), so the engine takes them for the developer.
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
use std::time::{Duration, Instant};

use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, Snapshot,
};
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{Actor, AgentKind, PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const DONE: &str = "<<raptor-fake-claude-done>>";
const DEADLINE: Duration = Duration::from_secs(20);

/// Entry point of the simulated Claude Code (see the module docs). As a
/// normal test it does nothing.
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

struct FakeClaude {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeClaude {
    /// Runs `command` in its shell and waits for it; whether it succeeded.
    fn run(&mut self, command: &str) -> bool {
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
            if let Some(ok) = line.split_once(DONE).map(|(_, ok)| ok) {
                return ok.trim() == "true";
            }
        }
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the
/// folder Claude Code is installed in, its hooks and its output files.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    worktree: PathBuf,
}

impl Machine {
    /// "demo": `main` with `login.txt`, branch `feat-login` and its worktree
    /// "feat-login" (`<root>/wt-feat-login`), observed.
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        let worktree = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        // Hooks used per command (`git -c core.hooksPath=…`): the repo's
        // hooks are never touched.
        let hooks = outside.path().join("slow-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let hook = hooks.join("reference-transaction");
        std::fs::write(
            &hook,
            "#!/bin/sh\n[ \"$1\" = committed ] && sleep 0.5\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let m = Self {
            f,
            outside,
            worktree,
        };
        let out = m.developer(
            &m.f.root,
            &["repo", "add", m.f.repo.to_str().unwrap()],
            "en_US.UTF-8",
        );
        assert!(out.status.success(), "{}", text(&out));
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self, lang: &str) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", lang.into()),
        ]
    }

    /// The developer, from their own terminal in `cwd`.
    fn developer(&self, cwd: &Path, args: &[&str], lang: &str) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The developer launches Claude Code in the worktree.
    fn launch_claude(&self) -> FakeClaude {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        let mut child = Command::new(agent)
            .args([
                "fake_claude_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env("en_US.UTF-8"))
            .env(FAKE_CLAUDE, "1")
            .env("HOME", &self.f.home)
            .current_dir(&self.worktree)
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

    /// Waits until the history has an event of `kind` in the worktree.
    fn event(&self, kind: GitEventKind) -> GitEventView {
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
                .unwrap();
            if let Some(e) = page
                .events
                .iter()
                .find(|e| e.kind == kind && Path::new(e.worktree.raw()) == self.worktree)
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no {kind:?}: {:#?}",
                page.events
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for a snapshot of the worktree whose `path` holds `content`.
    fn captured(&self, path: &str, content: &[u8]) -> String {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id) {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    let found = store
                        .files(id, "wt-wt-feat-login")
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                        .any(|(p, _, oid)| p == path && store.read_blob(oid).unwrap() == content);
                    if found {
                        return id.clone();
                    }
                }
            }
            assert!(start.elapsed() < DEADLINE, "{path} never captured");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// What the worktree holds that a user sees: its files, `HEAD`, the
    /// branch tip and the status.
    fn state(&self) -> Vec<String> {
        let git = |args: &[&str]| self.f.git_in(&self.worktree, args);
        let mut files: Vec<String> = std::fs::read_dir(&self.worktree)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name() != ".git")
            .map(|e| {
                format!(
                    "{}={:?}",
                    e.file_name().to_string_lossy(),
                    std::fs::read(e.path()).unwrap_or_default()
                )
            })
            .collect();
        files.sort();
        files.push(git(&["rev-parse", "HEAD"]));
        files.push(git(&["rev-parse", "feat-login"]));
        files.push(git(&["status", "--porcelain=v1"]));
        files
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&self.f.root, &["daemon", "stop", "--yes"], "en_US.UTF-8");
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

const WORK: &[u8] = b"user\npassword\n";
const UTIL: &[u8] = b"fn util() {}\n";

/// Criterio de salida 3 de M1: un agente hace `git reset --hard` con trabajo
/// sin commitear y lo recupera con `raptor undo` desde su shell, without being
/// asked. Before that, the developer at a terminal is asked to confirm taking
/// back the agent's work (US-TMC-013) and, without an answer, nothing changes.
#[test]
fn an_agent_recovers_its_work_after_a_raw_reset_hard() {
    let m = Machine::new();
    let mut claude = m.launch_claude();

    // The agent edits a tracked file and creates an untracked one; the
    // Time Machine captures them as they are.
    assert!(
        claude.run("printf 'user\\npassword\\n' > login.txt && printf 'fn util() {}\\n' > util.rs")
    );
    let captured = m.captured("login.txt", WORK);
    m.captured("util.rs", UTIL);

    // It throws the uncommitted work away with raw Git.
    let hooks = m.outside.path().join("slow-hooks");
    assert!(claude.run(&format!(
        "git -c core.hooksPath={} reset -q --hard",
        hooks.display()
    )));
    assert_eq!(
        std::fs::read(m.worktree.join("login.txt")).unwrap(),
        b"user\n"
    );
    // The engine names the reset and attributes it to the agent.
    let reset = m.event(GitEventKind::Reset);
    assert!(
        matches!(
            reset.actor,
            Actor::Agent {
                kind: AgentKind::ClaudeCode,
                ..
            }
        ),
        "{reset:#?}"
    );

    // The developer, at a terminal, is asked to confirm taking back the
    // agent's work (US-TMC-013); with no answer it is not confirmed and
    // nothing changes.
    let before = m.state();
    let out = m.developer(&m.worktree, &["undo"], "en_US.UTF-8");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("work of another agent")
            && text(&out).contains("not confirmed; nothing changed"),
        "{}",
        text(&out)
    );
    assert_eq!(m.state(), before);

    // The agent runs `raptor undo` from its shell and gets its work back:
    // the state the Time Machine captured by observation before the reset.
    let report = m.outside.path().join("undo.json");
    assert!(
        claude.run(&format!("{RAPTOR} undo --json > {} 2>&1", report.display())),
        "{}",
        std::fs::read_to_string(&report).unwrap_or_default()
    );
    let undo: UndoResult = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        undo.undone_subtype.as_ref().map(|s| s.sanitized()),
        Some("reset".to_owned())
    );
    assert_eq!(undo.target_snapshot_id, captured);
    assert_eq!(std::fs::read(m.worktree.join("login.txt")).unwrap(), WORK);
    assert_eq!(std::fs::read(m.worktree.join("util.rs")).unwrap(), UTIL);

    // Its own undo is not raw Git: a second one has nothing left, and says
    // so.
    let again = m.outside.path().join("again.txt");
    assert!(!claude.run(&format!("{RAPTOR} undo > {} 2>&1", again.display())));
    let said = std::fs::read_to_string(&again).unwrap();
    assert!(said.contains("nothing to undo"), "{said}");
    assert_eq!(std::fs::read(m.worktree.join("login.txt")).unwrap(), WORK);
    drop(claude);
    m.stop();
}
