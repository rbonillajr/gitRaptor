//! TS-GRP-010 (DS-TS-GRP-010 § 9.4, file 3): an agent that uses the CLI or talks to the socket
//! directly sees what the MCP shows, never more (S-01, ADR-MCP-001 Validación 12); the developer
//! keeps the full view even with their cwd in the agent's worktree. The real `raptor` binary over
//! a temporary machine (INF-GRP-001): temporary repos and profile, never this repo nor the real
//! profile (NFR-01).
//!
//! The developer runs under a pty (`script`), as from their own terminal. The simulated agent is
//! a copy of this test binary named `raptor-fake-agent` (see `fake_agent_entry`), declared to the
//! daemon through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`.
//!
//! macOS only: `script` options are the macOS ones, and the daemon reads the caller's working
//! folder with `proc_pidinfo`. Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::daemon::{LOG_FILE, running_pid};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const RAW_CLIENT: &str = "RAPTOR_TS_GRP_010_RAW_CLIENT";
/// A distinctive client version: it must never reach the log.
const CLIENT_VERSION: &str = "zz-ts-grp-010";

/// Entry point of the simulated agent: when this test binary runs as `raptor-fake-agent` with
/// `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child and exits with its status. As a
/// normal test it does nothing.
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// Entry point of a direct JSON-RPC client: with `RAPTOR_TS_GRP_010_RAW_CLIENT` set, it opens
/// the profile's socket from its working folder, says hello as `cli`, asks `engine.snapshot` and
/// `mcp.status`, and prints each answer on its own line, prefixed with `RAW `.
#[test]
fn raw_cli_entry() {
    if std::env::var_os(RAW_CLIENT).is_none() {
        return;
    }
    let dirs = ProfileDirs::resolve().unwrap();
    let socket = gitraptor_core::client::socket_path(&dirs).unwrap();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
            "protocol": PROTOCOL_VERSION, "client": "cli", "client_version": CLIENT_VERSION}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": methods::ENGINE_SNAPSHOT, "params": {}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": methods::MCP_STATUS, "params": {}}),
    ];
    let mut answers = BufReader::new(stream.try_clone().unwrap());
    for request in requests {
        writeln!(stream, "{request}").unwrap();
        let mut line = String::new();
        if answers.read_line(&mut line).unwrap() == 0 {
            break;
        }
        println!("RAW {}", line.trim_end());
    }
}

struct Machine {
    f: Fixture,
}

impl Machine {
    fn new(f: Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { f }
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
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
        ]
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// The developer, from their own terminal (a pty, not under an agent), in `cwd`.
    fn developer_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.developer_in(&self.f.root, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        text(&out)
    }

    fn agent_binary(&self) -> PathBuf {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
        }
        agent
    }

    /// `argv` as a child of the simulated agent, in `cwd`, with `extra` on top of the machine's
    /// environment.
    fn agent_command(&self, cwd: &Path, argv: &[&str], extra: &[(&str, &str)]) -> Command {
        let mut command = Command::new(self.agent_binary());
        command
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .envs(extra.iter().copied())
            .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
            .current_dir(cwd)
            .stdin(Stdio::null());
        command
    }

    fn as_agent(&self, cwd: &Path, argv: &[&str]) -> Output {
        self.agent_command(cwd, argv, &[]).output().unwrap()
    }

    /// The direct JSON-RPC client of `raw_cli_entry`, run by the simulated agent in `cwd`.
    fn raw_as_agent(&self, cwd: &Path) -> Vec<Value> {
        let me = std::env::current_exe().unwrap();
        let out = self
            .agent_command(
                cwd,
                &[
                    me.to_str().unwrap(),
                    "raw_cli_entry",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ],
                &[(RAW_CLIENT, "1")],
            )
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.find("RAW ").map(|i| &l[i + 4..]))
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // The daemon is detached: never leave one behind.
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// A long-lived simulated agent, killed on drop.
struct LiveAgent(Child);

impl Drop for LiveAgent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// The first JSON object of a command's output (a pty may wrap it in other text).
fn json_of(out: &Output) -> Value {
    let shown = String::from_utf8_lossy(&out.stdout);
    let start = shown
        .find('{')
        .unwrap_or_else(|| panic!("no JSON: {}", text(out)));
    serde_json::Deserializer::from_str(&shown[start..])
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap()
}

/// An observed repo, not enabled for the MCP.
fn observed() -> Machine {
    let m = Machine::new(Fixture::with_commit(&git_from_path()));
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m
}

/// ADR-MCP-001 Validación 12: a direct client that declares `cli` under an agent gets the MCP
/// view and `repo-not-enabled` in a repo that is not enabled; the log says why, in a closed
/// vocabulary.
#[test]
fn direct_cli_client_under_an_agent_gets_the_mcp_view() {
    let m = observed();
    let answers = m.raw_as_agent(&m.f.repo);
    assert_eq!(answers.len(), 3, "{answers:#?}");

    let hello = &answers[0]["result"];
    assert_eq!(
        hello["profile"], "mcp",
        "a client under an agent gets mcp, whatever it declares: {hello}"
    );
    let offered: Vec<String> = serde_json::from_value(hello["methods"].clone()).unwrap();
    for method in [methods::SCOPE_SNAPSHOT, methods::AUDIT_LIST] {
        assert!(
            !offered.iter().any(|m| m == method),
            "{method} is not offered to mcp: {offered:?}"
        );
    }

    let snapshot = &answers[1]["result"];
    assert!(
        snapshot.is_object() && snapshot.get("repos").is_none(),
        "engine.snapshot is the bounded one, without repos: {}",
        answers[1]
    );

    let status = &answers[2];
    assert!(
        status.get("result").is_none(),
        "mcp.status in a repo not enabled gives no data: {status}"
    );
    assert_eq!(
        status["error"]["data"]["reason"], "not-allowlisted",
        "the refusal is the MCP's repo-not-enabled: {status}"
    );
    let wire = status.to_string();
    assert!(
        !wire.contains(path(&m.f.repo)),
        "no path of the repo: {wire}"
    );

    let log = m.log();
    let line = log
        .lines()
        .find(|l| l.contains("client_profile"))
        .unwrap_or_else(|| panic!("the daemon never logged client_profile; log: {log}"));
    for field in ["class=cli", "profile=mcp", "reason=agent-in-ancestry"] {
        assert!(
            line.contains(field),
            "client_profile must carry {field}: {line}"
        );
    }
    for secret in [CLIENT_VERSION, path(&m.f.repo), path(&m.f.profile)] {
        assert!(
            !line.contains(secret),
            "client_profile must not carry {secret}: {line}"
        );
    }
}

/// `raptor status --json` launched by an agent is the MCP's view: refused in a repo that is not
/// enabled, the worktree's view (no list of repos) once the developer enabled it, and an untrusted
/// worktree name never reaches the terminal raw.
#[test]
fn raptor_status_under_an_agent_is_the_mcp_view() {
    let m = observed();
    let out = m.as_agent(&m.f.repo, &[RAPTOR, "status", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(shown.contains("repo-not-enabled"), "{}", text(&out));
    assert!(!shown.contains(path(&m.f.repo)), "no path: {shown}");
    assert!(!shown.contains("\"main\""), "no branch: {shown}");

    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let out = m.as_agent(&m.f.repo, &[RAPTOR, "status", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let view = json_of(&out);
    assert!(view.get("repos").is_none(), "no list of repos: {view}");
    let worktree = m.f.repo.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        view["worktree"]["untrusted"], worktree,
        "the caller's worktree: {view}"
    );

    // A worktree whose name carries an escape sequence.
    m.f.git(&["branch", "feat-esc"]);
    let hostile = m.f.root.join("wt-\u{1b}[31mred");
    m.f.git(&["worktree", "add", "-q", path(&hostile), "feat-esc"]);
    for args in [&["status", "--json"][..], &["status"][..]] {
        let mut argv = vec![RAPTOR];
        argv.extend_from_slice(args);
        let out = m.as_agent(&hostile, &argv);
        assert!(
            !out.stdout.contains(&0x1b) && !out.stderr.contains(&0x1b),
            "{args:?}: an untrusted name must never reach the terminal raw: {}",
            text(&out)
        );
    }
}

/// The developer, from their own terminal with the cwd in the worktree where an agent works,
/// keeps the full view: the attribution comes from the process, never from the cwd.
#[test]
fn developer_in_the_agent_worktree_keeps_the_full_view() {
    let m = observed();
    let _agent = LiveAgent(
        m.agent_command(&m.f.repo, &["/bin/sleep", "60"], &[])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let out = m.developer_in(&m.f.repo, &["status", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let view = json_of(&out);
    let repos = view["repos"]
        .as_array()
        .unwrap_or_else(|| panic!("the developer keeps the full list of repos: {view}"));
    assert_eq!(repos.len(), 1, "{view}");
}
