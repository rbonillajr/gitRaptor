//! Demo M2, criterion 2 (US-TMC-011): four Claude Code sessions work in four worktrees; in one of
//! them the developer runs `raptor undo --agent claude-code --since 20m`, confirms on a terminal
//! (a pty made with `script`) and only that worktree's agent work goes away; the other three,
//! their branches included, do not change; `raptor redo` brings it back.
//!
//! The real `raptor` binary is the daemon and the client over a temporary machine (temporary repo,
//! home and profile: NFR-01). The four "Claude Code" sessions are copies of this test binary named
//! `raptor-fake-agent`, one per worktree, that run `git` for their commits; the daemon takes them
//! for Claude Code through `GITRAPTOR_AGENT_EXECUTABLES` (debug builds only), so the Claude Code
//! session that runs the suite is nobody's work here. Every process runs with `env_clear()`. No
//! fixed waits: each state is awaited with a deadline.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, EventsHistoryResult, GitEventKind, Snapshot};
use gitraptor_api::{Actor, AgentKind, PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const DONE: &str = "<<raptor-fake-claude-done>>";
const DEADLINE: Duration = Duration::from_secs(30);
const NAMES: [&str; 4] = ["login", "pagos", "perfil", "reportes"];

/// Entry point of a simulated Claude Code (see the module docs). As a normal test it does
/// nothing.
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

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    /// `(name, worktree root)`, one per session.
    worktrees: Vec<(&'static str, PathBuf)>,
}

impl Machine {
    /// "demo": `main` with `api.rs`, and four linked worktrees on their own branches, observed.
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("api.rs", "fn api() {}\n");
        f.git(&["add", "api.rs"]);
        f.git(&["commit", "-q", "-m", "api"]);
        let worktrees: Vec<(&'static str, PathBuf)> = NAMES
            .iter()
            .map(|name| {
                f.git(&["branch", &format!("feat-{name}")]);
                let root = f
                    .add_worktree(&format!("feat-{name}"), &format!("feat-{name}"))
                    .canonicalize()
                    .unwrap();
                (*name, root)
            })
            .collect();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        // A slow hook, used per command (`git -c core.hooksPath=…`): the repo's hooks are never
        // touched, and `git` is alive when the engine samples its processes.
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
            worktrees,
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

    /// The developer, from their own terminal in `cwd`, without anything to type.
    fn developer(&self, cwd: &Path, args: &[&str], lang: &str) -> Output {
        self.on_terminal(cwd, args, lang, None)
    }

    /// `raptor <args>` on a pty in `cwd`, as from the developer's own terminal; `answer` is what
    /// they type. Whatever it prints comes back in `stdout`.
    fn on_terminal(&self, cwd: &Path, args: &[&str], lang: &str, answer: Option<&str>) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        let mut child = Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let (done, waiting) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            if matches!(
                waiting.recv_timeout(Duration::from_secs(90)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
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

    /// The developer launches Claude Code in `worktree`.
    fn launch_claude(&self, name: &str, worktree: &Path) -> FakeClaude {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
        }
        let _ = name;
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

    /// Waits until `worktree` has `n` commit events attributed to Claude Code (the owner is
    /// asserted before anything relies on it).
    fn agent_commits(&self, worktree: &Path, n: usize) {
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
                .unwrap();
            let mine = page
                .events
                .iter()
                .filter(|e| {
                    e.kind == GitEventKind::Commit
                        && Path::new(e.worktree.raw()) == worktree
                        && matches!(
                            e.actor,
                            Actor::Agent {
                                kind: AgentKind::ClaudeCode,
                                ..
                            }
                        )
                })
                .count();
            if mine >= n {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "{} has {mine} agent commits, wanted {n}: {:#?}",
                worktree.display(),
                page.events
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// What a worktree holds that a user sees: its files, `HEAD`, its branch tip and the status.
    fn state(&self, name: &str, worktree: &Path) -> Vec<String> {
        let git = |args: &[&str]| self.f.git_in(worktree, args);
        let mut files: Vec<String> = std::fs::read_dir(worktree)
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
        files.push(git(&["rev-parse", &format!("feat-{name}")]));
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

#[test]
fn four_sessions_and_one_agent_undone() {
    let m = Machine::new();
    let hooks = m.outside.path().join("slow-hooks");
    let base: Vec<Vec<String>> = m.worktrees.iter().map(|(n, w)| m.state(n, w)).collect();
    // Four Claude Code sessions, one per worktree, two commits each (one edits `api.rs`).
    let mut claudes: Vec<FakeClaude> = m
        .worktrees
        .iter()
        .map(|(name, worktree)| m.launch_claude(name, worktree))
        .collect();
    for ((name, worktree), claude) in m.worktrees.iter().zip(claudes.iter_mut()) {
        let commit = |claude: &mut FakeClaude, file: &str, body: &str, message: &str| {
            assert!(
                claude.run(&format!(
                    "printf '{body}\\n' > {file} && git add {file} && \
                     git -c core.hooksPath={} commit -q -m '{message}'",
                    hooks.display()
                )),
                "{name}: the agent's commit failed"
            );
        };
        commit(claude, &format!("{name}.rs"), &format!("fn {name}() {{}}"), "own file");
        commit(claude, "api.rs", &format!("fn api() {{ {name}(); }}"), "edit api");
        m.agent_commits(worktree, 2);
    }
    let after_work: Vec<Vec<String>> = m.worktrees.iter().map(|(n, w)| m.state(n, w)).collect();
    for (before, after) in base.iter().zip(&after_work) {
        assert_ne!(before, after, "every worktree holds its agent's commits");
    }
    let (name, target) = (m.worktrees[0].0, m.worktrees[0].1.clone());

    let out = m.on_terminal(
        &target,
        &["undo", "--agent", "claude-code", "--since", "20m"],
        "en_US.UTF-8",
        Some("y"),
    );

    assert!(
        out.status.success(),
        "TMC011-DEMO-M2: the undo by agent succeeds after the developer confirms: {}",
        text(&out)
    );
    assert_eq!(
        m.state(name, &target),
        base[0],
        "TMC011-DEMO-M2: the target worktree and its branch are as before its agent's work"
    );
    for i in 1..4 {
        assert_eq!(
            m.state(m.worktrees[i].0, &m.worktrees[i].1),
            after_work[i],
            "TMC011-DEMO-M2: worktree {} (files, HEAD, branch, status) must not change",
            m.worktrees[i].0
        );
    }

    let redo = m.on_terminal(&target, &["redo"], "en_US.UTF-8", Some("y"));
    assert!(
        redo.status.success(),
        "TMC011-DEMO-M2: raptor redo succeeds: {}",
        text(&redo)
    );
    assert_eq!(
        m.state(name, &target),
        after_work[0],
        "TMC011-DEMO-M2: redo brings the agent's work back"
    );
    for i in 1..4 {
        assert_eq!(
            m.state(m.worktrees[i].0, &m.worktrees[i].1),
            after_work[i],
            "TMC011-DEMO-M2: the other worktrees stay untouched after the redo"
        );
    }
    drop(claudes);
    m.stop();
}
