//! US-MCP-010 end to end: `safe_commit` validates every path as a literal name inside the
//! worktree, never adds ignored files, adds untracked ones only when named, and in a shared
//! worktree asks for explicit paths. The same rig as `mcp_safe_commit.rs` (copied, not shared):
//! the real `raptor` daemon with the production `commit` arm, `raptor-mcp`, the real hooks and the
//! system Git over a temporary machine; never this repo nor the real profile (NFR-01).
//!
//! Every refusal checks with `gitraptor_testkit::fingerprint` that the repo, the worktree and its
//! `.git` did not change, and that no commit was made. No fixed waits: every wait has a deadline.
//!
//! macOS only (see `mcp_safe_commit.rs`). Linux and Windows: Pendiente: etapa de validación
//! multiplataforma (DEP-MCP-9).
#![cfg(target_os = "macos")]
#![cfg(debug_assertions)]
#![allow(dead_code)]

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

/// A background process under its own simulated agent (a second agent session in the worktree);
/// killed on drop.
struct Bystander(Child);

impl Drop for Bystander {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Machine {
    /// A second live agent (`raptor-fake-agent` with a long `sleep` child) in the worktree.
    fn second_agent(&self) -> Bystander {
        let argv = ["/bin/sleep", "600"].map(str::to_owned);
        Bystander(
            self.agent_command(&argv, &[])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    /// The sessions `raptor status --json` shows in the worktree.
    fn sessions(&self) -> usize {
        let status = self.json(&self.wt, &["status", "--json"]);
        status["repos"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|r| r["worktrees"].as_array().into_iter().flatten())
            .filter(|w| w["path"] == path(&self.wt))
            .map(|w| w["sessions"].as_array().map_or(0, Vec::len))
            .sum()
    }

    fn wait_for_sessions(&self, n: usize) {
        wait_until(&format!("{n} sessions in the worktree"), || {
            self.sessions() >= n
        });
    }
}

/// One refused call: the code, the story's sentence, the path as untrusted data when `at` is given,
/// and nothing changed.
fn assert_path_refusal(
    m: &Machine,
    paths: &[&str],
    code: &str,
    says: &str,
    at: Option<&str>,
) -> Value {
    let before = m.footprint();
    let commits = m.commits();
    let mut agent = m.agent_session(SPANISH);

    let result = agent.commit(&Machine::msg("paths"), Some(paths));

    let refused = assert_refused(&result, code, says);
    if let Some(at) = at {
        assert_eq!(
            refused["params"]["path"],
            json!({ "untrusted": at }),
            "{result}"
        );
    }
    assert_unchanged(m, &before, commits);
    refused
}

// ----------------------------------------------------------------------------- the paths

/// Escenario 1: one path outside the worktree refuses the whole call, the path as untrusted data.
#[test]
fn a_path_outside_the_worktree_refuses_the_whole_call() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");

    assert_path_refusal(
        &m,
        &["src/a.rs", "../otro-repo/secreto"],
        "path-outside-worktree",
        "ruta fuera del worktree",
        Some("../otro-repo/secreto"),
    );
}

/// Escenario 2: `/etc/passwd`, a UNC path and a path with control characters are "ruta no válida".
#[test]
fn invalid_paths_never_reach_git() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");

    for bad in [
        "/etc/passwd",
        r"\\servidor\compartido\a.rs",
        "src/a\u{1}.rs",
    ] {
        let refused = assert_path_refusal(&m, &[bad], "invalid-path", "ruta no válida", None);
        assert!(
            refused["params"]["path"]["untrusted"].is_string(),
            "{bad:?}: {refused}"
        );
    }
}

/// Escenario 2: a link that leaves the worktree, as the path or as a folder of it, is "ruta fuera
/// del worktree"; what it points at is never read or written.
#[test]
fn a_link_out_of_the_worktree_is_outside() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    let other = m.outside.path().join("other-repo");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("secret.txt"), "secret\n").unwrap();
    std::os::unix::fs::symlink(&other, m.wt.join("escape")).unwrap();
    std::os::unix::fs::symlink(other.join("secret.txt"), m.wt.join("link.txt")).unwrap();

    for linked in ["link.txt", "escape/secret.txt"] {
        assert_path_refusal(
            &m,
            &[linked],
            "path-outside-worktree",
            "ruta fuera del worktree",
            Some(linked),
        );
    }
    assert_eq!(
        std::fs::read_to_string(other.join("secret.txt")).unwrap(),
        "secret\n"
    );
}

/// Escenario 2: more paths than the limit (256) are "demasiadas rutas".
#[test]
fn too_many_paths_are_refused() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    let names: Vec<String> = (0..257).map(|n| format!("src/f{n:03}.rs")).collect();
    let paths: Vec<&str> = names.iter().map(String::as_str).collect();

    assert_path_refusal(&m, &paths, "too-many-paths", "demasiadas rutas", None);
}

/// Escenario 3: a pathspec-looking name is a literal name, and none exists: "ruta no encontrada".
#[test]
fn pathspec_magic_is_a_literal_name_not_found() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");

    for name in [":(glob)**", "SRC/A.rs"] {
        assert_path_refusal(
            &m,
            &[name],
            "path-not-found",
            "ruta no encontrada",
            Some(name),
        );
    }
}

/// Escenario 4: a new file enters only when it is named.
#[test]
fn a_new_file_enters_only_when_named() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    m.f.git_in(&m.wt, &["add", "src/a.rs"]);
    m.edit("src/nuevo.rs", "new\n");
    let mut agent = m.agent_session(&[]);

    let first = agent.commit(&Machine::msg("staged"), None);

    assert_committed(&first);
    assert_eq!(m.files_of("HEAD"), ["src/a.rs"], "{first}");

    let second = agent.commit(&Machine::msg("named"), Some(&["src/nuevo.rs"]));

    assert_committed(&second);
    assert_eq!(m.files_of("HEAD"), ["src/nuevo.rs"], "{second}");
}

/// Escenario 5: an ignored file is never committed, even when named.
#[test]
fn an_ignored_file_is_never_committed() {
    let m = Machine::new(None, Hook::None);
    let canary = "CANARY-9f3a-never-in-an-answer";
    m.edit(".env", &format!("TOKEN={canary}\n"));

    let refused = assert_path_refusal(
        &m,
        &[".env"],
        "path-ignored",
        "la ruta .env está ignorada por Git; no se añade",
        Some(".env"),
    );

    assert!(!refused.to_string().contains(canary), "{refused}");
    assert!(
        m.f.git_in(&m.wt, &["log", "--all", "--format=%H", "--", ".env"])
            .trim()
            .is_empty(),
        ".env entered a commit"
    );
}

// ------------------------------------------------------------------- a shared worktree

/// Escenario 6: with another agent session alive in the worktree, "all staged" is refused with the
/// action; with named paths the commit goes in with only those.
#[test]
fn a_shared_worktree_needs_named_paths() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    m.edit("src/b.rs", "b2\n");
    m.f.git_in(&m.wt, &["add", "src/b.rs"]);
    let mut agent = m.agent_session(SPANISH);
    let _other = m.second_agent();
    // N2: if the detector merges both agents into one session, the second session is seeded in the
    // temporary profile's store; today it tells two processes apart.
    m.wait_for_sessions(2);
    let before = m.footprint();
    let commits = m.commits();

    let all = agent.commit(&Machine::msg("all"), None);

    assert_refused(
        &all,
        "other-session-present",
        "indica tus rutas: el worktree es compartido",
    );
    assert_unchanged(&m, &before, commits);

    let named = agent.commit(&Machine::msg("named"), Some(&["src/a.rs"]));

    assert_committed(&named);
    assert_eq!(m.files_of("HEAD"), ["src/a.rs"], "{named}");
    assert_eq!(m.commits(), commits + 1);
}

/// Escenario 6: the developer with the TUI open in the worktree does not make it shared.
#[test]
fn the_developers_tui_does_not_make_the_worktree_shared() {
    let m = Machine::new(None, Hook::None);
    m.edit("src/a.rs", "a2\n");
    m.f.git_in(&m.wt, &["add", "src/a.rs"]);
    let mut agent = m.agent_session(&[]);
    // The TUI under a pty, stdin held open so it stays up.
    let mut tui = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null", RAPTOR])
        .env_clear()
        .envs(m.env())
        .current_dir(&m.wt)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let hold = tui.stdin.take();
    let _tui = Bystander(tui);
    let commits = m.commits();

    let result = agent.commit(&Machine::msg("tui"), None);

    assert_committed(&result);
    assert_eq!(m.files_of("HEAD"), ["src/a.rs"], "{result}");
    assert_eq!(m.commits(), commits + 1);
    drop(hold);
}
