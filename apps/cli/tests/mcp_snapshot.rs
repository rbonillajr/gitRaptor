//! US-MCP-008 end to end: an agent takes a manual recovery point of its own worktree through
//! `raptor-mcp`'s `snapshot` tool, the developer sees it in `raptor timeline`, and the
//! refusals (label, operation in progress, unattributed, quota) say what the story says. The
//! real `raptor` and `raptor-mcp` binaries over a temporary machine: temporary repo and
//! profile, never this repo nor the real profile (NFR-01).
//!
//! The developer runs reserved commands under a pty (`script`). The simulated agent is a copy
//! of this test binary named `raptor-fake-agent` (see `fake_agent_entry`), declared to the
//! daemon through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`; the `raptor-mcp` it runs
//! inherits its pipes, so the agent is the server's parent, as with Claude Code. A session
//! with no agent above it is "unattributed".
//!
//! Written against the wire JSON so it compiles before the tool exists. No fixed waits: every
//! read of a server answer has a deadline.
//!
//! macOS only: `script` options are the macOS ones, and the daemon reads the caller's working
//! folder with `proc_pidinfo`. Linux and Windows: Pendiente: etapa de validación
//! multiplataforma (DEP-MCP-9).
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const DIRECT_KIND: &str = "RAPTOR_DIRECT_KIND";
const DIRECT_WORKTREE: &str = "RAPTOR_DIRECT_WORKTREE";
const DEADLINE: Duration = Duration::from_secs(60);
const SPANISH: &[(&str, &str)] = &[("LANG", "es_ES.UTF-8")];

/// Entry point of the simulated agent: when this test binary runs as `raptor-fake-agent` with
/// `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child (sharing its pipes) and exits
/// with its status. As a normal test it does nothing.
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

/// Entry point of a direct JSON-RPC client (no `raptor-mcp`): with `RAPTOR_DIRECT_KIND`, a
/// comma-separated list of kinds (`cli`, `mcp` or `other`), it makes one attempt per kind FROM
/// THIS SAME PROCESS: it connects declaring that kind from its working folder, prepares a
/// `snapshot` plan and runs it, and prints how far it got as one JSON line per attempt. One
/// process is one requester, so the attempts share its quota whatever kind they declare. As a
/// normal test it does nothing.
#[test]
fn direct_snapshot_entry() {
    let Some(kinds) = std::env::var_os(DIRECT_KIND) else {
        return;
    };
    let dirs = ProfileDirs::resolve().unwrap();
    // Each answer starts a line of its own: libtest prints "test … ... " without a newline.
    println!();
    for kind in kinds.to_str().unwrap().split(',') {
        println!("{}", direct_attempt(&dirs, kind));
    }
}

/// One attempt of [`direct_snapshot_entry`]: how far a `snapshot` declared as `kind` got.
fn direct_attempt(dirs: &ProfileDirs, kind: &str) -> Value {
    let kind = match kind {
        "cli" => ClientKind::Cli,
        "mcp" => ClientKind::Mcp,
        _ => ClientKind::Other,
    };
    let mut client = Client::connect(dirs, kind, PROTOCOL_VERSION).unwrap();
    let mut params = json!({"operation": "snapshot", "args": {"label": "direct client"}});
    if kind != ClientKind::Mcp {
        params["surface"] = json!("cli");
        params["worktree"] = json!(std::env::var(DIRECT_WORKTREE).unwrap());
    }
    let failed = |stage: &str, e: ClientError| match e {
        ClientError::Rpc(e) => json!({"stage": stage, "code": e.code, "data": e.data}),
        other => json!({"stage": stage, "other": other.to_string()}),
    };
    match client.call::<_, Value>(methods::OPERATION_PREPARE, params) {
        Err(e) => failed("prepare", e),
        Ok(prepared) => {
            let run = json!({"plan_id": prepared["plan_id"]});
            match client.call::<_, Value>(methods::OPERATION_RUN, run) {
                Ok(done) => json!({"stage": "run", "ok": done}),
                Err(e) => failed("run", e),
            }
        }
    }
}

/// `raptor-mcp` next to `raptor`; built there if this package was tested alone.
fn server() -> PathBuf {
    gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp")
}

/// The repo `shop` and its linked worktree `shop-feat-a`, observed and enabled for the MCP.
struct Machine {
    f: Fixture,
    wt: PathBuf,
}

impl Machine {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::with_commit(&git_from_path());
        f.git(&["branch", "feat-a"]);
        let wt = f
            .add_worktree("shop-feat-a", "feat-a")
            .canonicalize()
            .unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let m = Self { f, wt };
        m.ok(&["repo", "add", path(&m.f.repo)]);
        m.ok(&["mcp", "enable", path(&m.f.repo)]);
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
            ("PATH", "/usr/bin:/bin".into()),
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
            .env("LANG", "en_US.UTF-8")
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

    /// The manual snapshots the developer sees in `raptor timeline --json`.
    fn points(&self) -> Vec<Value> {
        let out = self.developer_in(&self.wt, &["timeline", "--json"]);
        assert!(out.status.success(), "timeline: {}", text(&out));
        let shown = text(&out);
        let start = shown
            .find('{')
            .unwrap_or_else(|| panic!("no JSON: {shown}"));
        let timeline: Value = serde_json::from_str(shown[start..].trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {shown}"));
        timeline["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["origin"]["entry"] == "manual-snapshot")
            .cloned()
            .collect()
    }

    /// The simulated agent: a copy of this test binary named like an agent, with `argv` as its
    /// child. Its pipes are the child's, so `raptor-mcp` talks to the test through them.
    fn agent_command(&self, argv: &[String], extra: &[(&str, &str)]) -> Command {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
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

    /// An MCP session with no agent above it: unattributed.
    fn unattributed_session(&self, extra: &[(&str, &str)]) -> Mcp {
        let mut cmd = Command::new(server());
        cmd.env_clear()
            .envs(self.env())
            .envs(extra.iter().copied())
            .current_dir(&self.wt);
        Mcp::start(cmd)
    }

    /// One direct JSON-RPC client process, as a child of a fake agent of its own or with no
    /// agent above it, making one attempt per kind of the comma-separated `kinds`: how far each
    /// got, in order (see `direct_snapshot_entry`).
    fn direct_client(&self, kinds: &str, under_agent: bool) -> Vec<Value> {
        let exe = std::env::current_exe().unwrap();
        let entry = [
            "direct_snapshot_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ];
        let extra = [(DIRECT_KIND, kinds), (DIRECT_WORKTREE, path(&self.wt))];
        let out = if under_agent {
            let mut argv = vec![exe.to_str().unwrap().to_owned()];
            argv.extend(entry.iter().map(|a| (*a).to_owned()));
            self.agent_command(&argv, &extra)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        } else {
            Command::new(&exe)
                .args(entry)
                .env_clear()
                .envs(self.env())
                .envs(extra.iter().copied())
                .current_dir(&self.wt)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };
        let answers: Vec<Value> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                // Parse from the first brace, as the reader of `Mcp` does.
                let json = l.find('{').map_or("", |at| &l[at..]);
                serde_json::from_str::<Value>(json)
                    .ok()
                    .filter(|v| v.get("stage").is_some())
            })
            .collect();
        assert_eq!(
            answers.len(),
            kinds.split(',').count(),
            "one answer per attempt: {}",
            text(&out)
        );
        answers
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

/// One MCP session: the server's stdin and, through a reader thread, its stdout. Every read
/// has a deadline. Lines that are not JSON (the harness of the fake agent prints its own) are
/// skipped.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    next: u64,
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
        mcp.notify("notifications/initialized");
        mcp
    }

    fn write(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str) {
        self.write(json!({"jsonrpc": "2.0", "method": method}));
    }

    fn send(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    fn read(&mut self, id: u64) -> Value {
        loop {
            let message = self
                .lines
                .recv_timeout(DEADLINE)
                .unwrap_or_else(|e| panic!("no answer to request {id}: {e}"));
            if message["id"] == id {
                return message;
            }
        }
    }

    /// `tools/call` of `name`: the tool result.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let id = self.send("tools/call", json!({"name": name, "arguments": arguments}));
        self.read(id)["result"].clone()
    }

    fn snapshot(&mut self, label: &str) -> Value {
        self.call("snapshot", json!({ "label": label }))
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

/// The `{code, message, action, params?}` of a refused call, from its text block.
fn refusal(result: &Value) -> Value {
    assert_eq!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
        .unwrap_or_else(|e| panic!("the refusal is not JSON ({e}): {result}"))
}

/// A refused snapshot: the stable code, the sentence of the story in Spanish in the message
/// or the action, and nothing of the machine's paths.
fn assert_refused(m: &Machine, result: &Value, code: &str, says: &str) {
    let refused = refusal(result);
    assert_eq!(refused["code"], code, "{result}");
    let sentence = format!("{} {}", refused["message"], refused["action"]);
    assert!(sentence.contains(says), "{says:?} not in {result}");
    assert!(!result.to_string().contains(path(&m.f.root)), "{result}");
}

// ---------------------------------------------------------------------------- the points

/// El agente toma un punto con una etiqueta; el desarrollador lo ve con su actor y el canal.
#[test]
fn snapshot_with_a_label_is_in_the_timeline_with_actor_and_mcp_channel() {
    let m = Machine::new();
    let mut agent = m.agent_session(&[]);

    let result = agent.snapshot("antes de migrar");

    assert_eq!(result["isError"], false, "{result}");
    let point = &result["structuredContent"];
    let id = point["snapshot_id"].as_str().expect("the id of the point");
    // The worktree by its folder name and the label, both as untrusted data; nothing else.
    assert_eq!(point["worktree"]["untrusted"], "wt-shop-feat-a", "{result}");
    assert_eq!(point["label"]["untrusted"], "antes de migrar", "{result}");
    let mut keys: Vec<_> = point.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["label", "snapshot_id", "worktree"], "{result}");
    assert!(!result.to_string().contains(path(&m.f.root)), "{result}");

    let points = m.points();
    assert_eq!(points.len(), 1, "{points:#?}");
    let entry = &points[0];
    assert_eq!(entry["origin"]["snapshot_id"], id, "{entry}");
    assert_eq!(
        entry["origin"]["label"]["untrusted"], "antes de migrar",
        "{entry}"
    );
    assert_eq!(entry["origin"]["channel"], "mcp", "{entry}");
    assert_eq!(entry["actor"]["actor"], "agent", "{entry}");
}

/// Un worktree con HEAD separado también admite el punto.
#[test]
fn snapshot_with_detached_head_is_allowed() {
    let m = Machine::new();
    m.f.git_in(&m.wt, &["checkout", "-q", "--detach"]);
    let mut agent = m.agent_session(&[]);

    let result = agent.snapshot("antes de migrar");

    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(m.points().len(), 1);
}

/// Con un rebase de Git a medias se rechaza con "operación en curso" y no se crea nada.
#[test]
fn snapshot_with_a_rebase_in_progress_is_refused_and_creates_nothing() {
    let m = Machine::new();
    // `main` and `feat-a` change the same file: the rebase stops on a conflict.
    m.f.write("c.txt", "from main\n");
    m.f.git(&["add", "c.txt"]);
    m.f.git(&["commit", "-q", "-m", "main c"]);
    std::fs::write(m.wt.join("c.txt"), "from feat-a\n").unwrap();
    m.f.git_in(&m.wt, &["add", "c.txt"]);
    m.f.git_in(&m.wt, &["commit", "-q", "-m", "feat-a c"]);
    let rebase =
        m.f.git_command(&m.wt, &["rebase", "main"])
            .output()
            .unwrap();
    assert!(
        !rebase.status.success(),
        "the rebase must stop on the conflict"
    );
    assert!(
        m.f.git_in(&m.wt, &["status"])
            .contains("rebase in progress"),
        "the fixture has a rebase in progress"
    );
    let mut agent = m.agent_session(SPANISH);

    let result = agent.snapshot("antes de migrar");

    assert_refused(&m, &result, "operation-in-progress", "operación en curso");
    assert_eq!(refusal(&result)["params"]["kind"], "git", "{result}");
    assert!(m.points().is_empty());
}

// ---------------------------------------------------------------------------- the label

fn assert_invalid_label(m: &Machine, label: &str) {
    let mut agent = m.agent_session(SPANISH);
    let result = agent.snapshot(label);
    assert_refused(m, &result, "invalid-text", "texto no válido");
    let refused = refusal(&result);
    assert_eq!(refused["params"]["field"], "label", "{result}");
    assert_eq!(refused["params"]["max_chars"], 64, "{result}");
    assert!(m.points().is_empty(), "a refused label creates nothing");
}

/// Una etiqueta de más de 64 caracteres es "texto no válido".
#[test]
fn a_label_over_the_limit_is_invalid_text() {
    assert_invalid_label(&Machine::new(), &"x".repeat(65));
}

/// Una etiqueta con caracteres de control es "texto no válido".
#[test]
fn a_label_with_control_characters_is_invalid_text() {
    assert_invalid_label(&Machine::new(), "antes\u{1b}[31m de migrar");
}

/// Una etiqueta vacía es "texto no válido".
#[test]
fn an_empty_label_is_invalid_text() {
    assert_invalid_label(&Machine::new(), "");
}

// ------------------------------------------------------------------------- who asks

/// Un cliente sin atribuir no puede tomar un punto: se le dice cómo registrarse.
#[test]
fn an_unattributed_client_cannot_snapshot() {
    let m = Machine::new();
    let mut session = m.unattributed_session(SPANISH);

    let result = session.snapshot("antes de migrar");

    assert_refused(&m, &result, "unattributed", "usa register_agent");
    assert!(m.points().is_empty());
}

// ------------------------------------------------------------------------------- quota

/// Un agente en bucle recibe la espera real de la cuota, no la de 3 s del cubo de escrituras,
/// y conserva los cinco puntos.
#[test]
fn a_looping_agent_gets_the_real_wait_and_keeps_its_snapshots() {
    let m = Machine::new();
    let mut agent = m.agent_session(&[]);
    for n in 1..=5 {
        let result = agent.snapshot(&format!("point {n}"));
        assert_eq!(result["isError"], false, "attempt {n}: {result}");
    }

    let sixth = agent.snapshot("point 6");

    let refused = refusal(&sixth);
    assert_eq!(refused["code"], "quota-exceeded", "{sixth}");
    assert_eq!(refused["params"]["window"], "minute", "{sixth}");
    let wait = refused["params"]["retry_after_s"].as_u64().unwrap();
    assert!(
        (4..=60).contains(&wait),
        "the real wait of the quota, not the bucket's 3 s: {wait}"
    );
    assert_eq!(m.points().len(), 5, "the five points are still there");
}

/// Un cliente JSON-RPC directo bajo el agente, que se declara `cli`, comparte la cuota del
/// agente; un sin atribuir se rechaza sea cual sea el canal que declare.
#[test]
fn a_direct_client_under_the_agent_shares_the_quota() {
    let m = Machine::new();

    // One process under one agent is one requester: five attempts declaring `mcp` and `other`
    // fill its minute, and a sixth declaring `cli` meets the same quota (C2: the daemon keys it
    // on the requester it resolves, never on the declared channel).
    let answers = m.direct_client("mcp,other,mcp,other,mcp,cli", true);
    for (n, done) in answers[..5].iter().enumerate() {
        assert_eq!(done["stage"], "run", "attempt {}: {done}", n + 1);
        assert_eq!(done["ok"]["outcome"], "done", "attempt {}: {done}", n + 1);
    }
    let direct = &answers[5];
    assert_eq!(direct["code"], -33060, "{direct}");
    assert_eq!(direct["data"]["window"], "minute", "{direct}");
    assert_eq!(m.points().len(), 5, "the sixth attempt added nothing");

    // Without an agent above it, whatever channel it declares, it is refused before capture.
    for kind in ["cli", "mcp", "other"] {
        let alone = m.direct_client(kind, false).remove(0);
        assert_eq!(alone["stage"], "prepare", "{kind}: {alone}");
        assert_eq!(alone["code"], -32014, "{kind}: {alone}");
        assert_eq!(
            alone["data"]["reason"], "unattributed-without-cockpit",
            "{kind}: {alone}"
        );
    }
    assert_eq!(m.points().len(), 5);
}

/// D5's limit, written down: the quota is per agent session, so a new agent process gets a new
/// one; the worktree ceiling is what stops the rotation (ADR-MCP-001, Enmienda 2026-10-08).
#[test]
fn rotating_agent_processes_hit_the_worktree_ceiling() {
    let m = Machine::new();
    for n in 1..=gitraptor_core::timemachine::manual::PER_WORKTREE_DAY {
        let done = m.direct_client("mcp", true).remove(0);
        assert_eq!(done["ok"]["outcome"], "done", "process {n}: {done}");
    }
    let refused = m.direct_client("mcp", true).remove(0);
    assert_eq!(refused["code"], -33060, "{refused}");
    assert_eq!(refused["data"]["window"], "worktree-day", "{refused}");
}
