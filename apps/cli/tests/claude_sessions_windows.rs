//! US-GRP-007 on Windows (DS-US-GRP-007, Enmienda 2026-10-08): the real `raptor` binary as daemon
//! and client detects a Claude Code process (S1: the executable path and the working folder read
//! from the process through `gitraptor-winsys`) in an observed worktree, shows it in
//! `raptor status`, and drops it when the process ends. Temporary repo and profile, never this repo
//! nor the real profile (NFR-01); the unix suite in `claude_sessions.rs` needs a pty (`script`).
//!
//! Claude Code is simulated as the debug daemon allows (`GITRAPTOR_AGENT_EXECUTABLES`): a copy of
//! `cmd.exe` named `raptor-fake-agent.exe` that sleeps in the worktree. The daemon is stopped by
//! its pid: `raptor daemon stop` is a reserved command that answers only to a desktop console.
#![cfg(windows)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const GIT: &str = r"C:\Program Files\Git\cmd\git.exe";
const KNOWN_AGENT: &str = "raptor-fake-agent";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new(GIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The path in the drive form the engine reports worktrees in.
fn plain(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap();
    let text = canonical.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned())
}

struct Machine {
    _root: tempfile::TempDir,
    repo: PathBuf,
    profile: PathBuf,
    agents: PathBuf,
    spawned: Vec<Child>,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("path with spaces").join("demo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let profile = root.path().join("profile");
        let agents = root.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let (mut observed, _) = Profile::open(ProfileDirs::under_root(&profile)).unwrap();
        observed.add_repo(&repo.join(".git"), None, 1).unwrap();
        drop(observed);
        Self {
            _root: root,
            repo,
            profile,
            agents,
            spawned: Vec::new(),
        }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", KNOWN_AGENT.into()),
            ("GITRAPTOR_TEST_GIT", GIT.into()),
            ("PATH", r"C:\Program Files\Git\cmd".into()),
            (
                "SystemRoot",
                std::env::var_os("SystemRoot").unwrap_or_default(),
            ),
        ]
    }

    fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A process named `name` (a copy of `cmd.exe`) that sleeps with `cwd` as its folder.
    fn launch(&mut self, name: &str, cwd: &Path) -> u32 {
        let exe = self.agents.join(name);
        if !exe.exists() {
            std::fs::copy(r"C:\Windows\System32\cmd.exe", &exe).unwrap();
        }
        let child = Command::new(&exe)
            .args(["/C", "ping -n 120 127.0.0.1 >NUL"])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.spawned.push(child);
        pid
    }

    fn end(&mut self, pid: u32) {
        let child = self.spawned.iter_mut().find(|c| c.id() == pid).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }

    /// The sessions `raptor status --json` shows for the worktree of the demo repo.
    fn sessions(&self) -> Vec<Value> {
        let out = self.raptor(&["status", "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        let want = plain(&self.repo);
        status["repos"][0]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| {
                w["path"]
                    .as_str()
                    .is_some_and(|p| p.eq_ignore_ascii_case(&want.to_string_lossy()))
            })
            .map(|w| w["sessions"].as_array().unwrap().clone())
            .unwrap_or_default()
    }

    /// Waits (polling, bounded) until the sessions of the worktree satisfy `ok`.
    fn sessions_when(&self, ok: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let start = Instant::now();
        loop {
            let sessions = self.sessions();
            if ok(&sessions) {
                return sessions;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "sessions: {sessions:#?}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        for child in &mut self.spawned {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output();
        }
    }
}

fn is_claude(s: &Value) -> bool {
    s["agent"] == "claude-code" && s["origin"] == "detected" && s["state"] == "active"
}

#[test]
fn a_claude_code_process_in_a_worktree_is_a_detected_session_until_it_ends() {
    let mut m = Machine::new();
    // No agent yet: the engine runs and the worktree has no session.
    assert!(m.sessions().is_empty());

    let repo = m.repo.clone();
    let pid = m.launch(&format!("{KNOWN_AGENT}.exe"), &repo);
    let sessions = m.sessions_when(|s| s.len() == 1 && is_claude(&s[0]));
    assert_eq!(sessions[0]["agent_name"], Value::Null);
    let text = m.raptor(&["status"]);
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    assert!(text.contains("Claude Code"), "{text}");
    assert!(
        !text.contains("cannot be detected"),
        "detection is available on Windows now: {text}"
    );

    // The process ends: the session ends with it, because its process is gone.
    m.end(pid);
    let ended = m.sessions_when(|s| s.len() == 1 && s[0]["state"] == "ended");
    assert_eq!(ended[0]["end_cause"], "process-gone");
}

#[test]
fn a_process_that_is_not_claude_code_is_never_a_session() {
    let mut m = Machine::new();
    let repo = m.repo.clone();
    m.launch("some-other-tool.exe", &repo);
    // Detection is on (a known agent in the same worktree is seen), so the other is no session.
    m.launch(&format!("{KNOWN_AGENT}.exe"), &repo);
    let sessions = m.sessions_when(|s| !s.is_empty());
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_claude(&sessions[0]));
}

#[test]
fn an_agent_outside_every_observed_worktree_is_not_a_session() {
    let mut m = Machine::new();
    let elsewhere = m.agents.clone();
    m.launch(&format!("{KNOWN_AGENT}.exe"), &elsewhere);
    // The engine has run its scans by the time the next status answers; nothing in the worktree.
    let repo = m.repo.clone();
    m.launch(&format!("{KNOWN_AGENT}.exe"), &repo);
    let sessions = m.sessions_when(|s| !s.is_empty());
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
}
