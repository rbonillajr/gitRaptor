//! US-GRP-008 on Windows (DS-US-GRP-008 § 9.5): scenarios 1 and 2 with the real `raptor` binary as
//! daemon and client, over a temporary repo and profile, never this repo nor the real profile
//! (NFR-01). It follows the harness of `claude_sessions_windows.rs` (copies of `cmd.exe` as the
//! simulated processes, the daemon stopped by its pid). The editors `cursor.exe`, `code.exe` and
//! `codex.exe` write a file and end; Claude Code is a simulated `claude.exe` that sleeps in the
//! worktree. Scenario 3 holds by construction there (no argv, so S3 never attributes) and is
//! covered by `an_unreadable_session_git_is_no_evidence` in `crates/core`.
//!
//! NOT run from macOS: it compiles empty there. Its real run is for the cross-platform validation
//! stage (`docs/architecture/xplat-pendientes.md`).
#![cfg(windows)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const GIT: &str = r"C:\Program Files\Git\cmd\git.exe";
/// The only agent executable the debug daemon knows: `claude.exe` is Claude Code.
const CLAUDE: &str = "claude";

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
    feat_pagos: PathBuf,
    feat_login: PathBuf,
    profile: PathBuf,
    /// Where the simulated processes are installed, outside every worktree.
    bin: PathBuf,
    spawned: Vec<Child>,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("path with spaces").join("demo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        for (name, content) in [
            ("login.txt", "user\n"),
            ("pagos.txt", "pay\n"),
            ("README.md", "# demo\n"),
        ] {
            std::fs::write(repo.join(name), content).unwrap();
        }
        git(&repo, &["add", "login.txt", "pagos.txt", "README.md"]);
        git(&repo, &["commit", "-q", "-m", "demo"]);
        let mut worktrees = Vec::new();
        for name in ["feat-pagos", "feat-login"] {
            let wt = root.path().join(format!("wt-{name}"));
            git(
                &repo,
                &["worktree", "add", "-q", "-b", name, wt.to_str().unwrap()],
            );
            worktrees.push(plain(&wt));
        }
        let profile = root.path().join("profile");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let (mut observed, _) = Profile::open(ProfileDirs::under_root(&profile)).unwrap();
        observed.add_repo(&repo.join(".git"), None, 1).unwrap();
        drop(observed);
        Self {
            _root: root,
            repo,
            feat_login: worktrees.pop().unwrap(),
            feat_pagos: worktrees.pop().unwrap(),
            profile,
            bin,
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
            ("GITRAPTOR_AGENT_EXECUTABLES", CLAUDE.into()),
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

    /// A copy of `cmd.exe` named `name`, outside every worktree.
    fn install(&self, name: &str) -> PathBuf {
        let exe = self.bin.join(name);
        if !exe.exists() {
            std::fs::copy(r"C:\Windows\System32\cmd.exe", &exe).unwrap();
        }
        exe
    }

    /// Claude Code, `claude.exe`: sleeps with `cwd` as its folder.
    fn launch_claude(&mut self, cwd: &Path) {
        let child = Command::new(self.install(&format!("{CLAUDE}.exe")))
            .args(["/C", "ping -n 120 127.0.0.1 >NUL"])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        self.spawned.push(child);
    }

    /// An editor of the developer (`cursor.exe`, `code.exe`, `codex.exe`), outside
    /// `GITRAPTOR_AGENT_EXECUTABLES`: appends a line to `file` in `cwd` and ends.
    fn editor(&self, name: &str, cwd: &Path, file: &str) {
        let status = Command::new(self.install(name))
            .args(["/C", &format!("echo edited>> {file}")])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{name}: {status}");
    }

    /// The worktree `raptor status --json` shows at `worktree`, once `ok` accepts it (polling,
    /// bounded).
    fn worktree_when(&self, worktree: &Path, ok: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let out = self.raptor(&["status", "--json"]);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let status: Value = serde_json::from_slice(&out.stdout).unwrap();
            let found = status["repos"][0]["worktrees"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| {
                    w["path"]
                        .as_str()
                        .is_some_and(|p| p.eq_ignore_ascii_case(&worktree.to_string_lossy()))
                })
                .cloned();
            if let Some(w) = found.as_ref().filter(|w| ok(w)) {
                return w.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "{}: {found:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn with_change(&self, worktree: &Path, file: &str) -> Value {
        self.worktree_when(worktree, |w| {
            w["changes"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["path"] == file))
        })
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

fn assert_unattributed_change(w: &Value) {
    assert_eq!(
        w["changes_actor"],
        json!({"actor": "unattributed"}),
        "the changes must figure as unattributed (changes_actor): {w:#}"
    );
}

/// Escenario 1: a change made from `editor` in "feat-pagos" is unattributed and makes no session.
fn a_change_without_an_identified_agent(editor: &str) {
    let m = Machine::new();
    let pagos = m.feat_pagos.clone();
    m.editor(editor, &pagos, "pagos.txt");
    let w = m.with_change(&pagos, "pagos.txt");
    assert_eq!(w["sessions"], json!([]), "{w:#}");
    assert_unattributed_change(&w);
    let out = m.raptor(&["sessions", "--all", "--json"]);
    let all: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(all["sessions"], json!([]), "{all:#}");
}

#[test]
fn a_change_without_an_identified_agent_is_unattributed_cursor() {
    a_change_without_an_identified_agent("cursor.exe");
}

#[test]
fn a_change_without_an_identified_agent_is_unattributed_other_editor() {
    a_change_without_an_identified_agent("code.exe");
}

#[test]
fn a_change_without_an_identified_agent_is_unattributed_unregistered_codex() {
    a_change_without_an_identified_agent("codex.exe");
}

/// Escenario 2: a doubtful change next to Claude Code is not Claude Code's.
#[test]
fn a_doubtful_change_next_to_claude_code_is_unattributed() {
    let mut m = Machine::new();
    let login = m.feat_login.clone();
    m.launch_claude(&login);
    m.worktree_when(&login, |w| {
        w["sessions"]
            .as_array()
            .is_some_and(|s| s.len() == 1 && is_claude(&s[0]))
    });
    m.editor("cursor.exe", &login, "README.md");
    let w = m.with_change(&login, "README.md");
    assert_eq!(w["sessions"].as_array().unwrap().len(), 1, "{w:#}");
    assert_unattributed_change(&w);
    assert_ne!(w["changes_actor"]["actor"], "claude-code", "{w:#}");
}
