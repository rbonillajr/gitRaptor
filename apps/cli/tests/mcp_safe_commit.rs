//! US-MCP-009 end to end: an agent commits the paths it names through `raptor-mcp`'s `safe_commit`
//! tool, over a real `raptor` daemon (production `commit` arm: no `GITRAPTOR_TEST_EXECUTOR_ARMS`),
//! the real `raptor-hook` dispatchers and the system Git, over a temporary machine: temporary repo,
//! HOME and profile, never this repo nor the real profile (NFR-01).
//!
//! The developer runs reserved commands under a pty (`script`). The simulated agent is a copy of
//! this test binary named `raptor-fake-agent`, declared to the daemon through the debug-only
//! `GITRAPTOR_AGENT_EXECUTABLES`; the `raptor-mcp` it runs inherits its pipes, so the agent is the
//! server's parent, as with Claude Code. Written against the wire JSON so it compiles before the
//! tool exists. No fixed waits: every wait has a deadline.
//!
//! macOS only: `script` options are the macOS ones and the daemon reads the caller's working
//! folder with `proc_pidinfo`. Linux and Windows: Pendiente: etapa de validación multiplataforma
//! (DEP-MCP-9).
#![cfg(target_os = "macos")]
#![cfg(debug_assertions)]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::ClientKind;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fingerprint::{Scope, Snapshot};
use gitraptor_testkit::fixture::{clone_binary, git_from_path};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";
const DEADLINE: Duration = Duration::from_secs(60);
const SPANISH: &[(&str, &str)] = &[("LANG", "es_ES.UTF-8")];
/// The trailer `agents-commit` (the default) asks of an agent's commit.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
const WARN_AGENT_COMMIT: &str =
    r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;
const ASK_COMMIT: &str = r#"{"permissions":{"ask":["commit"]}}"#;

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

/// A direct JSON-RPC client: with `RAPTOR_TEST_RPC` = `{"root", "call": [method, params]}` it
/// prints the answer as one `RPC-ANSWER <json>` line. As a normal test it does nothing.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let call = &spec["call"];
    let answer = match client.call::<_, Value>(call[0].as_str().unwrap(), call[1].clone()) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    println!("RPC-ANSWER {answer}");
}

/// `raptor-mcp` next to `raptor`; built there if this package was tested alone.
fn server() -> PathBuf {
    gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp")
}

/// The user's own `pre-commit` (chained by Guardrails' dispatcher).
#[derive(Clone, Copy, PartialEq)]
enum Hook {
    None,
    /// Writes a marker outside the repo.
    Ran,
    /// Writes "started", then waits for the release file.
    Slow,
}

/// The repo `shop` and its linked worktree `shop-feat-a`, observed, protected by Guardrails and
/// enabled for the MCP. `src/a.rs` and `src/b.rs` are tracked; `.env` is ignored.
struct Machine {
    f: Fixture,
    wt: PathBuf,
    outside: tempfile::TempDir,
}

impl Machine {
    fn new(settings: Option<&str>, hook: Hook) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::with_commit(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("src/a.rs", "a1\n");
        f.write("src/b.rs", "b1\n");
        f.write(".gitignore", ".env\n");
        let mut add = vec!["add", "src", ".gitignore"];
        if let Some(settings) = settings {
            f.write(".gitraptor/settings.json", settings);
            add.push(".gitraptor");
        }
        f.git(&add);
        f.git(&["commit", "-q", "-m", "shop base"]);
        f.git(&["branch", "feat-a"]);
        let wt = f
            .add_worktree("shop-feat-a", "feat-a")
            .canonicalize()
            .unwrap();
        let outside = tempfile::tempdir().unwrap();
        let m = Self { f, wt, outside };
        let body = match hook {
            Hook::None => None,
            Hook::Ran => Some(format!("echo ran >> '{}'", path(&m.ran()))),
            Hook::Slow => Some(format!(
                "echo started > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done",
                path(&m.started()),
                path(&m.release())
            )),
        };
        if let Some(body) = body {
            gitraptor_testkit::hooks::write_script(
                &m.f.repo.join(".git/hooks/pre-commit"),
                &format!("#!/bin/sh\n{body}\nexit 0\n"),
            );
        }
        let repo = path(&m.f.repo).to_owned();
        m.ok(&["repo", "add", &repo]);
        m.ok(&["mcp", "enable", &repo]);
        m.ok(&["guard", "install", "--yes", &repo]);
        m
    }

    fn ran(&self) -> PathBuf {
        self.outside.path().join("hook-ran")
    }

    fn started(&self) -> PathBuf {
        self.outside.path().join("hook-started")
    }

    fn release(&self) -> PathBuf {
        self.outside.path().join("hook-release")
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

    /// The JSON `raptor <args>` prints under the pty.
    fn json(&self, cwd: &Path, args: &[&str]) -> Value {
        let out = self.developer_in(cwd, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        let shown = text(&out);
        let start = shown
            .find('{')
            .unwrap_or_else(|| panic!("no JSON: {shown}"));
        serde_json::from_str(shown[start..].trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {shown}"))
    }

    /// The operations `raptor timeline --json` shows from the worktree.
    fn operations(&self) -> Vec<Value> {
        self.json(&self.wt, &["timeline", "--json"])["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["origin"]["entry"] == "operation")
            .cloned()
            .collect()
    }

    /// The decisions of the Guardrails log of `shop`, with their layers (the install's own change
    /// of state is not one).
    fn decisions(&self) -> Vec<Value> {
        let me = std::env::current_exe().unwrap();
        let spec = json!({"root": self.f.profile,
            "call": [gitraptor_api::methods::GUARD_LOG, {"path": self.f.repo}]});
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null"])
            .arg(&me)
            .args([
                "rpc_client_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(RPC_ENV, spec.to_string())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let shown = text(&out);
        let line = shown
            .lines()
            .find_map(|l| {
                l.split_once("RPC-ANSWER ")
                    .map(|(_, j)| j.trim().to_owned())
            })
            .unwrap_or_else(|| panic!("no answer: {shown}"));
        let log: Value = serde_json::from_str(&line).unwrap();
        log["ok"]["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("no log: {log}"))
            .iter()
            .filter(|e| e["kind"] != "protection-state")
            .cloned()
            .collect()
    }

    /// What a refusal must leave as it was: the repo and the worktree, file by file.
    fn footprint(&self) -> Snapshot {
        Snapshot::take(
            &[Scope::new("repo", &self.f.repo), Scope::new("wt", &self.wt)],
            &Default::default(),
        )
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

    /// Files a commit touched.
    fn files_of(&self, rev: &str) -> Vec<String> {
        self.f
            .git_in(&self.wt, &["show", "--format=", "--name-only", rev])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn edit(&self, rela: &str, content: &str) {
        let p = self.wt.join(rela);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// The simulated agent: a copy of this test binary named like an agent, with `argv` as its
    /// child. Its pipes are the child's, so `raptor-mcp` talks to the test through them.
    fn agent_command(&self, argv: &[String], extra: &[(&str, &str)]) -> Command {
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
        .envs(extra.iter().copied())
        .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
        .current_dir(&self.wt);
        cmd
    }

    /// An MCP session of an attributed agent, started in its worktree.
    fn agent_session(&self, extra: &[(&str, &str)]) -> Mcp {
        let argv = vec![server().to_str().unwrap().to_owned()];
        Mcp::start(self.agent_command(&argv, extra))
    }

    /// `sh -c line` run by the simulated agent, to the end.
    fn agent_sh(&self, line: &str) -> Output {
        let argv = ["/bin/sh", "-c", line].map(str::to_owned);
        self.agent_command(&argv, &[])
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
        std::fs::write(self.release(), "").ok();
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

/// A refused `safe_commit`: the stable code and the sentence of the story, in Spanish, in the
/// message or the action.
fn assert_refused(result: &Value, code: &str, says: &str) -> Value {
    let refused = refusal(result);
    assert_eq!(refused["code"], code, "{result}");
    let sentence = format!("{} {}", refused["message"], refused["action"]).to_lowercase();
    assert!(
        sentence.contains(&says.to_lowercase()),
        "{says:?} not in {result}"
    );
    refused
}

/// A refusal leaves the repo, the worktree and its commits as they were.
fn assert_unchanged(m: &Machine, before: &Snapshot, commits: usize) {
    let changes = diff(before, &m.footprint());
    assert!(
        changes.is_empty(),
        "a refusal changed the machine: {changes:#?}"
    );
    assert_eq!(m.commits(), commits, "a refusal made a commit");
}

fn assert_committed(result: &Value) -> &Value {
    assert_eq!(
        result["isError"], false,
        "safe_commit did not commit (does the tool exist?): {result}"
    );
    &result["structuredContent"]
}

// ----------------------------------------------------------------------- the happy path

/// Escenario 1: previous point before the commit, the repo's hooks ran, the answer has the oid, the
/// path and the worktree and no message, and the timeline has the commit with its actor.
#[test]
fn safe_commit_commits_the_named_path_after_a_prior_with_the_hooks() {
    let m = Machine::new(None, Hook::Ran);
    m.edit("src/a.rs", "a2\n");
    let before = m.commits();
    let mut agent = m.agent_session(&[]);

    let result = agent.commit(&Machine::msg("a"), Some(&["src/a.rs"]));

    let done = assert_committed(&result);
    let oid = done["oid"].as_str().expect("the oid of the new commit");
    assert_eq!(oid, m.head(), "{result}");
    assert_eq!(
        done["paths"],
        json!([{"untrusted": "src/a.rs"}]),
        "{result}"
    );
    assert_eq!(done["paths_total"], 1, "{result}");
    assert_eq!(done["truncated"], false, "{result}");
    assert_eq!(done["worktree"]["untrusted"], "wt-shop-feat-a", "{result}");
    assert!(done["operation_id"].is_string(), "{result}");
    let mut keys: Vec<_> = done.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "operation_id",
            "oid",
            "paths",
            "paths_total",
            "truncated",
            "worktree"
        ],
        "{result}"
    );
    assert_eq!(m.commits(), before + 1);
    assert!(m.ran().exists(), "the repo's pre-commit did not run");

    // The timeline: an operation of the agent, protected by a point taken before it.
    let ops = m.operations();
    assert_eq!(ops.len(), 1, "{ops:#?}");
    assert_eq!(ops[0]["actor"]["actor"], "agent", "{:#?}", ops[0]);
    assert_eq!(
        ops[0]["protection"]["level"], "guaranteed-prior",
        "{:#?}",
        ops[0]
    );
    assert!(ops[0]["protection"]["snapshot_id"].is_string());
}

/// Escenario 1: the answer never carries the message.
#[test]
fn the_answer_never_carries_the_message() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    let mut agent = m.agent_session(&[]);

    let result = agent.commit(
        &Machine::msg("zq-subject-never-echoed"),
        Some(&["src/a.rs"]),
    );

    assert_committed(&result);
    assert!(
        !result.to_string().contains("zq-subject-never-echoed"),
        "the answer carries the commit message: {result}"
    );
    assert!(!result.to_string().contains("Co-Authored-By"), "{result}");
}

/// D17: named paths commit their working-tree content with `--only`; other staged work stays
/// staged.
#[test]
fn named_paths_commit_their_working_tree_content_and_leave_other_staged_work_staged() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a-staged\n");
    m.f.git_in(&m.wt, &["add", "src/a.rs"]);
    m.edit("src/a.rs", "a-working-tree\n");
    m.edit("src/b.rs", "b-staged\n");
    m.f.git_in(&m.wt, &["add", "src/b.rs"]);
    let mut agent = m.agent_session(&[]);

    let result = agent.commit(&Machine::msg("only a"), Some(&["src/a.rs"]));

    assert_committed(&result);
    assert_eq!(m.files_of("HEAD"), ["src/a.rs"], "{result}");
    assert_eq!(
        m.f.git_in(&m.wt, &["show", "HEAD:src/a.rs"]),
        "a-working-tree\n",
        "the commit must carry the working-tree content of a named path"
    );
    assert_eq!(
        m.f.git_in(&m.wt, &["diff", "--cached", "--name-only"])
            .trim(),
        "src/b.rs",
        "the other staged work must stay staged"
    );
}

// --------------------------------------------------------------------------- Guardrails

/// Escenario 2: the authorship policy asks for the agent's trailer; the commit without it is denied
/// with the rule codes of the hook's denial for the same raw `git commit`, one log entry of layer
/// `mcp`, and the worktree unchanged.
#[test]
fn a_commit_without_the_agent_trailer_is_denied_like_the_hook() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(&[]);

    let result = agent.commit("fix: no trailer", Some(&["src/a.rs"]));

    let refused = refusal(&result);
    assert_eq!(refused["code"], "policy-denied", "{result}");
    let rules = refused["params"]["rules"].as_array().expect("params.rules");
    assert!(rules.iter().any(|r| r == "agents-commit"), "{result}");
    assert!(refused["action"].is_string(), "{result}");
    assert_unchanged(&m, &before, commits);
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["layer"], "mcp", "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial", "{entries:#?}");
    let logged: Vec<&Value> = entries[0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["rule"])
        .collect();
    for rule in rules {
        assert!(
            logged.contains(&rule),
            "{rule} not in the log: {entries:#?}"
        );
    }
    assert_eq!(entries[0]["decisionId"], refused["params"]["decision_id"]);

    // The same agent, the same message, raw: the hook denies with the same rule.
    m.f.git_in(&m.wt, &["add", "src/a.rs"]);
    let raw = m.agent_sh(&format!(
        "'{}' commit -q -m 'fix: no trailer'",
        path(&m.f.git)
    ));
    assert!(!raw.status.success(), "{}", text(&raw));
    assert!(text(&raw).contains("\"agents-commit\""), "{}", text(&raw));
}

/// D10: the hooks that run under an allowed `safe_commit` reuse the decision bound to it: one entry
/// of layer `mcp`, none added by the `pre-commit` and `commit-msg` dispatchers.
#[test]
fn the_hooks_under_a_safe_commit_reuse_the_bound_decision_and_log_once() {
    let m = Machine::new(Some(WARN_AGENT_COMMIT), Hook::Ran);
    m.edit("src/a.rs", "a2\n");
    let before = m.commits();
    let mut agent = m.agent_session(&[]);

    // `warn`: the commit goes in and the decision leaves one notice.
    let result = agent.commit("fix: warned, no trailer", Some(&["src/a.rs"]));

    assert_committed(&result);
    assert_eq!(m.commits(), before + 1);
    assert!(m.ran().exists(), "the repo's pre-commit did not run");
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["layer"], "mcp", "{entries:#?}");
    assert_eq!(entries[0]["count"], 1, "{entries:#?}");
}

/// Escenario 7: "ask" with no queue is refused with the developer's action; the worktree does not
/// change and the decision is logged once, with layer `mcp`.
#[test]
fn a_commit_in_ask_is_refused_with_the_developers_action() {
    let m = Machine::new(Some(ASK_COMMIT), Hook::None);
    m.edit("src/a.rs", "a2\n");
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);

    let result = agent.commit(&Machine::msg("ask"), Some(&["src/a.rs"]));

    let refused = assert_refused(
        &result,
        "confirmation-required",
        "requiere confirmación del desarrollador",
    );
    assert!(
        refused["action"]
            .as_str()
            .unwrap()
            .contains("desarrollador"),
        "the action must say the decision is the developer's: {result}"
    );
    assert_unchanged(&m, &before, commits);
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["layer"], "mcp", "{entries:#?}");
}

/// Escenario 8: asking again gives the same refusal.
#[test]
fn retrying_an_ask_gets_the_same_refusal() {
    let m = Machine::new(Some(ASK_COMMIT), Hook::None);
    m.edit("src/a.rs", "a2\n");
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);
    let first = agent.commit(&Machine::msg("ask"), Some(&["src/a.rs"]));
    let first = assert_refused(
        &first,
        "confirmation-required",
        "requiere confirmación del desarrollador",
    );

    let second = agent.commit(&Machine::msg("ask"), Some(&["src/a.rs"]));

    let second = assert_refused(
        &second,
        "confirmation-required",
        "requiere confirmación del desarrollador",
    );
    assert_eq!(first["message"], second["message"]);
    assert_eq!(first["action"], second["action"]);
    assert_unchanged(&m, &before, commits);
}

/// Certification: the server never asks the client anything (no elicitation, BR-MCP-WF-004), not
/// even when the decision is "ask".
#[test]
fn a_safe_commit_session_never_receives_a_server_request() {
    let m = Machine::new(Some(ASK_COMMIT), Hook::None);
    m.edit("src/a.rs", "a2\n");
    let mut agent = m.agent_session(SPANISH);

    let result = agent.commit(&Machine::msg("ask"), Some(&["src/a.rs"]));

    assert_refused(
        &result,
        "confirmation-required",
        "requiere confirmación del desarrollador",
    );
    agent.drain();
    assert!(
        agent.server_requests.is_empty(),
        "the server asked the client: {:#?}",
        agent.server_requests
    );
}

// ------------------------------------------------------------------------------ the message

fn assert_invalid_message(message: &str, says: &str) {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);

    let result = agent.commit(message, Some(&["src/a.rs"]));

    let refused = assert_refused(&result, "invalid-text", says);
    assert_eq!(refused["params"]["field"], "message", "{result}");
    assert_eq!(refused["params"]["max_bytes"], 16384, "{result}");
    assert_unchanged(&m, &before, commits);
}

/// Escenario 3: an empty message is "el mensaje es obligatorio".
#[test]
fn an_empty_message_is_required() {
    assert_invalid_message("", "el mensaje es obligatorio");
}

/// Escenario 3: a message over the limit is "mensaje demasiado largo".
#[test]
fn a_message_over_the_limit_is_too_long() {
    assert_invalid_message(
        &format!("fix: {}", "x".repeat(16384)),
        "mensaje demasiado largo",
    );
}

/// Escenario 3: a message with control characters is "texto no válido".
#[test]
fn a_message_with_control_characters_is_invalid_text() {
    assert_invalid_message("fix: a\u{1b}[31m red", "texto no válido");
}

// -------------------------------------------------------------------------- nothing to do

/// Escenario 4: with no change to commit, "nada que commitear" and no commit.
#[test]
fn nothing_to_commit_creates_no_commit() {
    let m = Machine::new(None, Hook::None);
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);

    let result = agent.commit(&Machine::msg("nothing"), None);

    assert_refused(&result, "nothing-to-commit", "nada que commitear");
    assert_unchanged(&m, &before, commits);
}

// ----------------------------------------------------------------------------- the client

/// The operations of the timeline once the commit landed (the worktree's tip moved).
fn wait_for_the_commit(m: &Machine, commits: usize) -> Value {
    wait_until("the commit to land", || m.commits() == commits + 1);
    let mut ops = Vec::new();
    wait_until("the commit in the timeline", || {
        ops = m.operations();
        !ops.is_empty()
    });
    assert_eq!(ops.len(), 1, "{ops:#?}");
    ops.remove(0)
}

/// A commit held in its hook: the call is sent and the hook has started. An answer before that
/// (the tool refused or is missing) fails at once.
fn slow_commit(m: &Machine, agent: &mut Mcp) -> u64 {
    let id = agent.send_commit(&Machine::msg("slow"), Some(&["src/a.rs"]));
    wait_until("the hook to start", || {
        if let Ok(message) = agent.lines.try_recv() {
            agent.keep(&message);
            assert!(
                message["id"] != id,
                "safe_commit answered before its hook started (is the tool served?): {message}"
            );
        }
        m.started().exists()
    });
    id
}

/// Escenario 6: the client cancels the call; the operation ends all the same and is in the
/// timeline with its actor.
#[test]
fn a_cancelled_call_still_commits_and_is_in_the_timeline() {
    let m = Machine::new(None, Hook::Slow);
    m.edit("src/a.rs", "a2\n");
    let commits = m.commits();
    let mut agent = m.agent_session(&[]);
    let id = slow_commit(&m, &mut agent);

    agent.cancel(id);
    std::fs::write(m.release(), "").unwrap();

    let entry = wait_for_the_commit(&m, commits);
    assert_eq!(entry["actor"]["actor"], "agent", "{entry}");
}

/// Escenario 6: the session closes while the hook waits; the commit finishes all the same.
#[test]
fn a_closed_session_still_finishes_the_commit() {
    let m = Machine::new(None, Hook::Slow);
    m.edit("src/a.rs", "a2\n");
    let commits = m.commits();
    let mut agent = m.agent_session(&[]);
    slow_commit(&m, &mut agent);

    agent.give_up();
    std::fs::write(m.release(), "").unwrap();

    let entry = wait_for_the_commit(&m, commits);
    assert_eq!(entry["actor"]["actor"], "agent", "{entry}");
}

/// Certification (D2): a client that gives up early (cancels, then goes away) still gets the commit
/// in the timeline.
#[test]
fn a_client_that_gives_up_early_still_gets_the_commit_in_the_timeline() {
    let m = Machine::new(None, Hook::Slow);
    m.edit("src/a.rs", "a2\n");
    let commits = m.commits();
    let mut agent = m.agent_session(&[]);
    let id = slow_commit(&m, &mut agent);

    agent.cancel(id);
    agent.give_up();
    std::fs::write(m.release(), "").unwrap();

    let entry = wait_for_the_commit(&m, commits);
    assert_eq!(entry["actor"]["actor"], "agent", "{entry}");
    assert_eq!(entry["origin"]["entry"], "operation", "{entry}");
}

/// Escenario 6: a retry while the commit runs is `operation-in-progress` (write), pointing at
/// `status`; the first commit still lands, once.
#[test]
fn a_retry_while_the_commit_runs_is_operation_in_progress() {
    let m = Machine::new(None, Hook::Slow);
    m.edit("src/a.rs", "a2\n");
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);
    let first = slow_commit(&m, &mut agent);

    let retry = agent.commit(&Machine::msg("retry"), Some(&["src/a.rs"]));

    let refused = assert_refused(&retry, "operation-in-progress", "status");
    assert_eq!(refused["params"]["kind"], "write", "{retry}");
    std::fs::write(m.release(), "").unwrap();
    let done = agent.read(first)["result"].clone();
    assert_committed(&done);
    assert_eq!(m.commits(), commits + 1, "the retry made a second commit");
}
