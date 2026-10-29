//! US-GRD-016 (part A) end to end: `raptor-mcp`'s `safe_commit` applies `policy.commit-format`
//! like Git directo (US-GRD-009, the MCP scenario). A message out of format is refused with
//! `policy-denied`, naming the rule in `rules` and `reasons`; the branch and the index stay as
//! they were and the decision leaves one entry, layer `mcp`. Real `raptor` daemon, real
//! `raptor-hook` dispatchers and the system Git over a temporary machine (NFR-01). The harness is
//! the one of `guard_us_grd_016.rs` (fake agent `raptor-fake-agent` declared through the
//! debug-only `GITRAPTOR_AGENT_EXECUTABLES`). No fixed waits: every wait has a deadline.
//!
//! macOS only, like that suite: `script` options are the macOS ones and the daemon reads the
//! caller's working folder with `proc_pidinfo`. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]
#![cfg(debug_assertions)]
#![allow(dead_code)]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{clone_binary, git_from_path};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const DEADLINE: Duration = Duration::from_secs(60);
/// The trailer `agents-commit` (the default) asks of an agent's commit.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
const FORMAT: &str = r#"{"policies":{"commitFormat":{}}}"#;

/// The simulated agent: runs `RAPTOR_FAKE_AGENT_ARGV` as its child, sharing its pipes.
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

/// `raptor-mcp` next to `raptor`; built there if this package was tested alone.
fn server() -> PathBuf {
    gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp")
}

/// The repo `shop` (a bare remote as `origin`) and the linked worktree of `branch`, observed and,
/// as asked, enabled for the MCP and protected by Guardrails. `src/a.rs` and `src/b.rs` are
/// tracked; the team `settings` are committed on `main`.
struct Machine {
    f: Fixture,
    wt: PathBuf,
}

impl Machine {
    fn build(settings: Option<&str>, branch: &str, mcp: bool, install: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::with_commit(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("src/a.rs", "a1\n");
        f.write("src/b.rs", "b1\n");
        let mut add = vec!["add", "src"];
        if let Some(settings) = settings {
            f.write(".gitraptor/settings.json", settings);
            add.push(".gitraptor");
        }
        f.git(&add);
        f.git(&["commit", "-q", "-m", "shop base"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", path(&remote)]);
        f.git(&["remote", "add", "origin", path(&remote)]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["branch", branch]);
        f.git(&["push", "-q", "origin", branch]);
        let wt = f
            .add_worktree(&format!("shop-{}", branch.replace('/', "-")), branch)
            .canonicalize()
            .unwrap();
        let m = Self { f, wt };
        let repo = path(&m.f.repo).to_owned();
        m.ok(&["repo", "add", &repo]);
        if mcp {
            m.ok(&["mcp", "enable", &repo]);
        }
        if install {
            m.ok(&["guard", "install", "--yes", &repo]);
        }
        m
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            // Debug builds only: how often the daemon checks that the protection is in place.
            ("GITRAPTOR_TEST_GUARD_HEALTH_MS", "100".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// The developer, from their own terminal (a pty, not under an agent) in `cwd`.
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

    /// `raptor <args>` without a terminal (a command that is not reserved).
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

    /// The JSON `raptor <args>` prints under the pty.
    fn json(&self, args: &[&str]) -> Value {
        let shown = self.ok(args);
        let start = shown
            .find('{')
            .unwrap_or_else(|| panic!("no JSON: {shown}"));
        serde_json::from_str(shown[start..].trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {shown}"))
    }

    fn repo(&self) -> String {
        path(&self.f.repo).to_owned()
    }

    /// `raptor guard status --json` of the repo.
    fn status(&self) -> Value {
        let out = self.raptor(&["guard", "status", "--json", &self.repo()]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {}", text(&out)))
    }

    /// The entries of `raptor guard log --json` (with `extra` flags).
    fn log(&self, extra: &[&str]) -> Vec<Value> {
        let repo = self.repo();
        let mut args = vec!["guard", "log", "--json"];
        args.extend_from_slice(extra);
        args.push(&repo);
        self.json(&args)["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("no entries in the log"))
            .clone()
    }

    /// The decisions of the log (not the changes of state).
    fn decisions(&self, extra: &[&str]) -> Vec<Value> {
        self.log(extra)
            .into_iter()
            .filter(|e| e["kind"] != "protection-state")
            .collect()
    }

    /// The entries of the allowlist transitions of the log.
    fn allowlist_entries(&self) -> Vec<Value> {
        self.log(&[])
            .into_iter()
            .filter(|e| e["operation"]["kind"] == "mcp-allowlist")
            .collect()
    }

    fn commits(&self) -> usize {
        self.f
            .git_in(&self.wt, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }

    fn head(&self) -> String {
        self.f
            .git_in(&self.wt, &["rev-parse", "HEAD"])
            .trim()
            .to_owned()
    }

    fn edit(&self, rela: &str, content: &str) {
        let p = self.wt.join(rela);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// The simulated agent: a copy of this test binary named like an agent, with `argv` as its
    /// child. Its pipes are the child's, so `raptor-mcp` talks to the test through them.
    fn agent_command(&self, argv: &[String]) -> Command {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            clone_binary(&std::env::current_exe().unwrap(), &agent);
        }
        let mut cmd = Command::new(&agent);
        cmd.args([
            "fake_agent_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .envs(self.env())
        .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
        .current_dir(&self.wt);
        cmd
    }

    /// An MCP session of an attributed agent, started in its worktree.
    fn agent_session(&self) -> Mcp {
        let argv = vec![server().to_str().unwrap().to_owned()];
        Mcp::start(self.agent_command(&argv))
    }

    /// `sh -c line` run by the simulated agent, to the end.
    fn agent_sh(&self, line: &str) -> Output {
        let argv = ["/bin/sh", "-c", line].map(str::to_owned);
        self.agent_command(&argv)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A message the default policy lets through (the agent's trailer).
    fn msg(tag: &str) -> String {
        format!("fix: {tag}\n\n{CLAUDE}\n")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // The daemon is detached: never leave one behind.
        if let Ok(Some(pid)) = running_pid(&ProfileDirs::under_root(&self.f.profile).state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// One MCP session: the server's stdin and, through a reader thread, its stdout. Every read has
/// a deadline. Requests the server makes of the client are kept in `server_requests`.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    next: u64,
    server_requests: Vec<Value>,
}

impl Mcp {
    fn start(mut cmd: Command) -> Self {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                // The harness of the fake agent prints "test … ... " without a newline, and the
                // server's first answer lands on that line: parse from the first brace.
                let json = line.find('{').map_or("", |at| &line[at..]);
                if let Ok(message) = serde_json::from_str::<Value>(json) {
                    let _ = tx.send(message);
                }
            }
        });
        let mut mcp = Self {
            child,
            stdin,
            lines,
            next: 1,
            server_requests: Vec::new(),
        };
        let id = mcp.send(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "claude-code", "version": "2.1.284"}
            }),
        );
        mcp.read(id);
        mcp.notify("notifications/initialized", json!({}));
        mcp
    }

    fn write(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn send(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    fn keep(&mut self, message: &Value) {
        if message.get("method").is_some() && message.get("id").is_some() {
            self.server_requests.push(message.clone());
        }
    }

    fn read(&mut self, id: u64) -> Value {
        loop {
            let message = self
                .lines
                .recv_timeout(DEADLINE)
                .unwrap_or_else(|e| panic!("no answer to request {id}: {e}"));
            self.keep(&message);
            if message["id"] == id && message.get("method").is_none() {
                return message;
            }
        }
    }

    /// Everything the server sent that is waiting, kept as requests when it is one.
    fn drain(&mut self) {
        while let Ok(message) = self.lines.try_recv() {
            self.keep(&message);
        }
    }

    fn send_commit(&mut self, message: &str, paths: Option<&[&str]>) -> u64 {
        let mut arguments = json!({ "message": message });
        if let Some(paths) = paths {
            arguments["paths"] = json!(paths);
        }
        self.send(
            "tools/call",
            json!({"name": "safe_commit", "arguments": arguments}),
        )
    }

    /// `tools/call` of `safe_commit`: the tool result.
    fn commit(&mut self, message: &str, paths: Option<&[&str]>) -> Value {
        let id = self.send_commit(message, paths);
        let answer = self.read(id);
        assert!(
            answer.get("result").is_some(),
            "safe_commit is not served as a tool yet: {answer}"
        );
        answer["result"].clone()
    }

    fn cancel(&mut self, id: u64) {
        self.notify(
            "notifications/cancelled",
            json!({"requestId": id, "reason": "user interrupted"}),
        );
    }

    /// The client goes away: stdin closed, then the server and its agent killed.
    fn give_up(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
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

/// Polls `ready` until it holds, within [`DEADLINE`].
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < DEADLINE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The `{code, message, action, params?}` of a refused call, from its text block.
fn refusal(result: &Value) -> Value {
    assert_eq!(
        result["isError"], true,
        "safe_commit did not refuse (does the tool exist?): {result}"
    );
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or(""))
        .unwrap_or_else(|e| panic!("the refusal is not JSON ({e}): {result}"))
}

/// The staged and unstaged state of the agent's worktree.
fn index_state(m: &Machine) -> String {
    m.f.git_in(&m.wt, &["status", "--porcelain=v1"])
}

/// US-GRD-009 MCP scenario: a message out of format by `safe_commit` is refused with
/// `policy-denied` naming `policy.commit-format`; nothing moves and one `mcp` entry is left.
#[test]
fn safe_commit_out_of_format_is_policy_denied() {
    let m = Machine::build(Some(FORMAT), "feat-a", true, true);
    assert_eq!(m.status()["state"], "full", "the repo must be `full`");
    m.edit("src/a.rs", "a2\n");
    let commits = m.commits();
    let head = m.head();
    let index = index_state(&m);
    let mut agent = m.agent_session();

    let result = agent.commit(
        &format!("arreglos varios\n\n{CLAUDE}\n"),
        Some(&["src/a.rs"]),
    );

    let refused = refusal(&result);
    assert_eq!(refused["code"], "policy-denied", "{result}");
    assert!(
        refused["params"]["rules"]
            .as_array()
            .is_some_and(|r| r.iter().any(|x| x == "policy.commit-format")),
        "{result}"
    );
    assert!(
        refused["params"]["reasons"].as_array().is_some_and(|r| r
            .iter()
            .any(|x| x["rule"] == "policy.commit-format" && x["level"] == "floor")),
        "the refusal must carry the rule and its level: {result}"
    );
    assert_eq!(m.commits(), commits, "a refusal made a commit");
    assert_eq!(m.head(), head, "the branch moved");
    assert_eq!(index_state(&m), index, "the index changed");

    let entries = m.decisions(&[]);
    assert_eq!(entries.len(), 1, "one decision: {entries:#?}");
    assert_eq!(entries[0]["layer"], "mcp", "{entries:#?}");
}
