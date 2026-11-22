//! US-MCP-023, beyond the signed contract: `daemon status --json` of an agent and of the
//! developer deserialize into the contract's own types, and `guard exec --json --yes` runs git
//! and reports its exit code once the repo is protected. Same simulated agent as
//! `us_mcp_023_cli_json.rs`: a copy of this test binary named `raptor-fake-agent`.
//!
//! macOS only until the cross-platform validation stage.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use gitraptor_api::cli_contract::{DaemonStatusView, McpDaemonStatusView};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const DONE: &str = "<<raptor-fake-claude-done>>";

/// Entry point of the simulated agent. As a normal test it does nothing.
#[test]
fn fake_claude_entry() {
    if std::env::var_os(FAKE_CLAUDE).is_none() {
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(line.unwrap())
            .status();
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
            if line.contains(DONE) {
                return;
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
    worktree: PathBuf,
}

impl Machine {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        f.git(&["branch", "victim"]);
        let worktree = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let m = Self {
            f,
            outside: tempfile::tempdir().unwrap(),
            worktree,
        };
        let repo = m.f.repo.to_str().unwrap().to_owned();
        m.on_terminal(&["repo", "add", &repo]);
        m.on_terminal(&["mcp", "enable", &repo]);
        m
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// A reserved command as the developer, from their own terminal (a pty).
    fn on_terminal(&self, args: &[&str]) -> String {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        let out = Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `raptor <args>` as the developer, no agent above it: exit code and stdout.
    fn plain(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .env("HOME", &self.f.home)
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    /// `raptor <args>` run by the simulated agent: its stdout.
    fn as_agent(&self, args: &[&str]) -> String {
        let agent = self.outside.path().join(FAKE_AGENT);
        gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
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
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut claude = FakeClaude {
            child,
            stdin,
            stdout,
        };
        let out = self.outside.path().join("out");
        claude.run(&format!("{RAPTOR} {} > {}", args.join(" "), out.display()));
        std::fs::read_to_string(&out).unwrap_or_default()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&ProfileDirs::under_root(&self.f.profile).state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn data_of(stdout: &str) -> Value {
    let value: Value = serde_json::from_str(stdout.trim_end())
        .unwrap_or_else(|e| panic!("not one JSON line ({e}): {stdout:?}"));
    assert_eq!(value["schema_version"], "1.1", "{value}");
    value["data"].clone()
}

#[test]
fn daemon_status_of_an_agent_and_of_the_developer_deserialize_into_the_contract_types() {
    let m = Machine::new();

    let agent = data_of(&m.as_agent(&["daemon", "status", "--json"]));
    let view: McpDaemonStatusView = serde_json::from_value(agent.clone()).unwrap_or_else(|e| {
        panic!("agent daemon status is not McpDaemonStatusView ({e}): {agent}")
    });
    assert!(
        view.requester.is_some(),
        "the daemon sees the agent: {agent}"
    );

    let (code, stdout) = m.plain(&["daemon", "status", "--json"]);
    assert_eq!(code, 0, "{stdout}");
    let developer = data_of(&stdout);
    let view: DaemonStatusView = serde_json::from_value(developer.clone()).unwrap_or_else(|e| {
        panic!("developer daemon status is not DaemonStatusView ({e}): {developer}")
    });
    assert_eq!(view.repos.len(), 1, "{developer}");
}

#[test]
fn guard_exec_json_reports_gits_exit_code_in_a_protected_repo() {
    let m = Machine::new();
    let repo = m.f.repo.to_str().unwrap().to_owned();
    m.on_terminal(&["guard", "install", &repo, "--yes"]);
    // Under a pty stdout and stderr merge: the envelope is the first JSON object.
    let shown = m.on_terminal(&[
        "guard", "exec", "--json", "--yes", "--", "git", "-C", &repo, "branch", "-d", "victim",
    ]);
    let start = shown
        .find('{')
        .unwrap_or_else(|| panic!("no JSON in {shown:?}"));
    let line = shown[start..].lines().next().unwrap_or_default();
    let data = data_of(line);
    assert_eq!(data["git_exit_code"], 0, "{shown}");
    assert!(
        m.f.git(&["branch", "--list", "victim"]).trim().is_empty(),
        "the exception ran git"
    );
}
