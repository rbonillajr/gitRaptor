//! US-MCP-002 and US-MCP-003 end to end: the developer chooses the repos
//! the MCP may use, and `raptor-mcp`'s `status` answers only for the repo of
//! the session's folder, if it is enabled. The real `raptor` and
//! `raptor-mcp` binaries over a temporary machine (INF-GRP-001): temporary
//! repos and profile, never this repo nor the real profile (NFR-01).
//!
//! The developer runs reserved commands under a pty (`script`), as from
//! their own terminal. The simulated agent is a copy of this test binary
//! named `raptor-fake-agent` (see `fake_agent_entry`), declared to the
//! daemon through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`.
//!
//! macOS only: `script` options are the macOS ones, and the daemon reads the
//! caller's working folder with `proc_pidinfo`. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma (DEP-MCP-9).
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{ClientKind, RefusalReason, RefusedData};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";

/// Entry point of the simulated agent: when this test binary runs as
/// `raptor-fake-agent` with `RAPTOR_FAKE_AGENT_ARGV`, it runs that command
/// as its child and exits with its status. As a normal test it does nothing.
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

/// Entry point of a direct MCP client: when this test binary runs with
/// `RAPTOR_MCP_SNAPSHOT`, it connects to the engine with the `mcp` profile
/// from its working folder and prints `engine.snapshot`.
#[test]
fn mcp_snapshot_entry() {
    if std::env::var_os("RAPTOR_MCP_SNAPSHOT").is_none() {
        return;
    }
    let dirs = ProfileDirs::resolve().unwrap();
    let mut client = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let snapshot: Value = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    println!("{snapshot}");
}

/// `raptor-mcp` next to `raptor`; build it if this package was tested alone.
fn server() -> PathBuf {
    let server = PathBuf::from(RAPTOR).with_file_name("raptor-mcp");
    if !server.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "-q", "-p", "gitraptor-mcp", "--bin", "raptor-mcp"])
            .status()
            .unwrap();
        assert!(status.success());
    }
    server
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

    /// The developer, from their own terminal (a pty, not under an agent).
    fn developer(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `argv` run as a child of the simulated agent.
    fn as_agent(&self, argv: &[&str]) -> Output {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.developer(args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        text(&out)
    }

    /// `raptor mcp list`, as repo ids.
    fn allowlist(&self) -> Vec<String> {
        let out = self.raptor(&["mcp", "list"]);
        assert!(out.status.success(), "{}", text(&out));
        let shown = String::from_utf8_lossy(&out.stdout).into_owned();
        if shown.contains("no repo is enabled") {
            return Vec::new();
        }
        shown.lines().map(str::to_owned).collect()
    }

    fn repo_id(&self, path: &Path) -> String {
        let out = self.raptor(&["status", "--json"]);
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        let want = gitraptor_core::observe::canonical(path);
        status["repos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| {
                r["worktrees"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|w| Path::new(w["path"].as_str().unwrap()) == want)
            })
            .map(|r| r["repo_id"].as_str().unwrap().to_owned())
            .expect("an observed repo")
    }

    fn observed(&self) -> usize {
        let out = self.raptor(&["status", "--json"]);
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        status["repos"].as_array().unwrap().len()
    }

    fn running(&self) -> bool {
        running_pid(&self.dirs().state).unwrap().is_some()
    }

    /// One MCP session started in `cwd` that calls `status` (unless
    /// `call` is false): the tool result, or `Null`.
    fn mcp_status_with(&self, server: &Path, cwd: &Path, call: bool) -> Value {
        let mut child = Command::new(server)
            .env_clear()
            .envs(self.env())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut send = |message: Value| {
            writeln!(stdin, "{message}").unwrap();
            stdin.flush().unwrap();
        };
        let mut read = |id: u64| -> Value {
            loop {
                let mut line = String::new();
                assert!(stdout.read_line(&mut line).unwrap() > 0, "server closed");
                let message: Value = serde_json::from_str(&line).unwrap();
                if message["id"] == id {
                    return message;
                }
            }
        };
        send(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "claude-code", "version": "2.1.284"}
            }
        }));
        read(1);
        send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        let result = if call {
            send(json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "status", "arguments": {}}
            }));
            read(2)["result"].clone()
        } else {
            Value::Null
        };
        drop(send);
        drop(stdin);
        child.wait().unwrap();
        result
    }

    /// `engine.snapshot` over a direct `mcp` connection from `cwd`.
    fn mcp_snapshot(&self, cwd: &Path) -> Value {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "mcp_snapshot_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env("RAPTOR_MCP_SNAPSHOT", "1")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| {
                let start = l.find('{')?;
                serde_json::from_str::<Value>(&l[start..]).ok()
            })
            .expect("a snapshot")
    }

    fn mcp_status(&self, cwd: &Path) -> Value {
        self.mcp_status_with(&server(), cwd, true)
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

/// A refused tool call: an error with the reason and its action, and
/// nothing else (no path, branch or key of any repo).
fn assert_refused(result: &Value, reason: &str, action: &str, secrets: &[&str]) {
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(
        result["structuredContent"],
        json!({"reason": reason, "action": action}),
        "{result}"
    );
    let wire = result.to_string();
    for secret in secrets {
        assert!(!wire.contains(secret), "{secret} leaked: {wire}");
    }
}

fn shop() -> Machine {
    Machine::new(Fixture::with_commit(&git_from_path()))
}

// ---------------------------------------------------------------- US-MCP-002

/// El desarrollador habilita un repo observado.
#[test]
fn the_developer_enables_an_observed_repo() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    let id = m.repo_id(&m.f.repo);
    let head = m.f.git(&["rev-parse", "HEAD"]);

    let out = m.ok(&["mcp", "enable", path(&m.f.repo)]);
    assert!(out.contains("agents can now use"), "{out}");
    assert_eq!(m.allowlist(), std::slice::from_ref(&id));
    // Idempotent.
    let out = m.ok(&["mcp", "enable", path(&m.f.repo)]);
    assert!(out.contains("already enabled"), "{out}");
    assert_eq!(m.allowlist(), [id]);
    // Nothing changes in the repo.
    assert_eq!(m.f.git(&["status", "--porcelain"]), "");
    assert_eq!(m.f.git(&["rev-parse", "HEAD"]), head);
}

/// Observar un repo no lo habilita para el MCP.
#[test]
fn observing_a_repo_does_not_enable_it() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    assert!(m.allowlist().is_empty());
    let id = m.repo_id(&m.f.repo);
    // The MCP layer of the repo is inactive: `status` gives no data.
    let result = m.mcp_status(&m.f.repo);
    assert_refused(
        &result,
        "repo-not-enabled",
        "ask-the-developer-to-run-raptor-mcp-enable",
        &[&id, path(&m.f.repo), "main"],
    );
}

/// No se puede habilitar un repo que no se observa.
#[test]
fn a_repo_that_is_not_observed_cannot_be_enabled() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    let out = m.developer(&["mcp", "enable", path(&m.f.other_repo)]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("observe the repo first"),
        "{}",
        text(&out)
    );
    assert!(m.allowlist().is_empty());
}

/// Retirar un repo de la observación lo saca de la allowlist; volver a
/// observarlo no lo habilita.
#[test]
fn retiring_the_repo_takes_it_out_of_the_allowlist() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["mcp", "enable", path(&m.f.repo)]);

    let out = m.ok(&["repo", "retire", path(&m.f.repo)]);
    assert!(out.contains("also left the MCP allowlist"), "{out}");
    assert!(m.allowlist().is_empty());

    m.ok(&["repo", "add", path(&m.f.repo)]);
    assert!(m.allowlist().is_empty());
    assert_eq!(
        m.mcp_status(&m.f.repo)["structuredContent"]["reason"],
        "repo-not-enabled"
    );
}

/// El desarrollador quita un repo de la allowlist y sigue observado.
#[test]
fn disabling_keeps_the_repo_observed() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let out = m.ok(&["mcp", "disable", path(&m.f.repo)]);
    assert!(out.contains("can no longer use"), "{out}");
    assert!(m.allowlist().is_empty());
    assert_eq!(m.observed(), 1);
    // Retiring it now gives no MCP notice.
    let out = m.ok(&["repo", "retire", path(&m.f.repo)]);
    assert!(!out.contains("MCP allowlist"), "{out}");
}

/// Un agente no puede habilitar un repo: ni con la CLI desde su shell ni por
/// una conexión MCP.
#[test]
fn an_agent_cannot_enable_a_repo() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["repo", "add", path(&m.f.other_repo)]);

    for argv in [
        vec![RAPTOR, "mcp", "enable", path(&m.f.other_repo)],
        vec![
            "/usr/bin/script",
            "-q",
            "/dev/null",
            RAPTOR,
            "mcp",
            "enable",
            path(&m.f.other_repo),
        ],
    ] {
        let out = m.as_agent(&argv);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("only the developer can choose which repos agents use"),
            "{}",
            text(&out)
        );
    }

    let mut mcp = Client::connect(&m.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let err = mcp
        .call::<_, Value>(methods::MCP_ENABLE, json!({"path": path(&m.f.other_repo)}))
        .unwrap_err();
    let ClientError::Rpc(err) = err else {
        panic!("{err:?}")
    };
    assert_eq!(err.code, code::RESERVED_REFUSED);
    let data: RefusedData = serde_json::from_value(err.data.unwrap()).unwrap();
    assert_eq!(data.reason, RefusalReason::NotAvailableToMcp);
    // The allowlist is not readable over MCP either.
    let err = mcp
        .call::<_, Value>(methods::MCP_ALLOWLIST, json!({}))
        .unwrap_err();
    assert!(matches!(err, ClientError::Rpc(e) if e.code == code::METHOD_NOT_FOUND));
    drop(mcp);

    assert!(m.allowlist().is_empty());
}

// ---------------------------------------------------------------- US-MCP-003

/// A direct `mcp` client gets no key of a repo outside the allowlist from
/// `engine.snapshot` either (ADR-MCP-001 § 3, MCP02).
#[test]
fn the_mcp_snapshot_names_only_an_enabled_repo() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    let id = m.repo_id(&m.f.repo);
    let snapshot = m.mcp_snapshot(&m.f.repo);
    assert_eq!(snapshot["caller_repo"], Value::Null, "{snapshot}");
    assert!(!snapshot.to_string().contains(&id));

    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let snapshot = m.mcp_snapshot(&m.f.repo);
    assert_eq!(
        snapshot["caller_repo"]["repo_id"],
        id.as_str(),
        "{snapshot}"
    );
}

/// El agente pide el estado desde una subcarpeta de su worktree; un "sin
/// atribuir" también puede leer y recibe la acción para escribir.
#[test]
fn status_from_a_subfolder_names_the_repo_and_the_worktree() {
    let m = shop();
    m.f.git(&["branch", "feat-a"]);
    let feat = m.f.add_worktree("shop-feat-a", "feat-a");
    let name = feat.file_name().unwrap().to_str().unwrap().to_owned();
    let src = feat.join("src");
    std::fs::create_dir(&src).unwrap();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let id = m.repo_id(&m.f.repo);

    let result = m.mcp_status(&src);
    assert_eq!(result["isError"], false, "{result}");
    let status = &result["structuredContent"];
    assert_eq!(status["repo_id"], id.as_str());
    assert_eq!(status["repo_state"], "observed");
    assert_eq!(status["worktree"]["untrusted"], name.as_str());
    assert_eq!(status["main"], false);
    assert_eq!(status["requester"], json!({"actor": "unattributed"}));
    assert_eq!(status["action"], "register-to-write");
    // The text block carries the same JSON.
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, status);
    // No path in the answer (SEC-12).
    assert!(!result.to_string().contains(path(&m.f.root)), "{result}");

    // From the main worktree, the main one.
    let main = m.mcp_status(&m.f.repo);
    assert_eq!(main["structuredContent"]["main"], true, "{main}");
}

/// Un cambio de directorio del agente no cambia su ámbito: el ámbito es la
/// carpeta de `raptor-mcp`, que nunca cambia, no la de la shell del agente.
/// Un enlace simbólico hacia otro repo resuelve al repo real.
#[test]
fn the_scope_is_the_servers_real_folder() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["repo", "add", path(&m.f.other_repo)]);
    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let other = m.repo_id(&m.f.other_repo);
    // A link inside the enabled repo into a repo that is not enabled.
    let link = m.f.repo.join("into-other");
    std::os::unix::fs::symlink(&m.f.other_repo, &link).unwrap();

    let result = m.mcp_status(&link);
    assert_refused(
        &result,
        "repo-not-enabled",
        "ask-the-developer-to-run-raptor-mcp-enable",
        &[&other, path(&m.f.other_repo)],
    );
}

/// Fuera de un repo observado, ninguna herramienta da datos.
#[test]
fn outside_an_observed_repo_no_data_is_returned() {
    let m = shop();
    m.ok(&["repo", "add", path(&m.f.repo)]);
    m.ok(&["mcp", "enable", path(&m.f.repo)]);
    let id = m.repo_id(&m.f.repo);
    let downloads = m.f.root.join("descargas");
    std::fs::create_dir(&downloads).unwrap();
    let result = m.mcp_status(&downloads);
    assert_refused(
        &result,
        "not-in-observed-worktree",
        "start-the-session-inside-an-observed-repo",
        &[&id, path(&m.f.repo)],
    );
}

/// El motor arranca con la primera llamada, no con la sesión.
#[test]
fn the_engine_starts_with_the_first_call() {
    let m = shop();
    assert!(!m.running());
    m.mcp_status_with(&server(), &m.f.repo, false);
    assert!(!m.running(), "a session without tool calls starts nothing");

    let result = m.mcp_status(&m.f.repo);
    assert!(m.running(), "{result}");
    // No repo observed yet: an answer, without data.
    assert_eq!(
        result["structuredContent"]["reason"], "not-in-observed-worktree",
        "{result}"
    );
}

/// Si el motor no puede arrancar, el agente recibe la acción y ningún dato.
#[test]
fn an_engine_that_cannot_start_gives_the_action() {
    let m = shop();
    // A `raptor-mcp` with no `raptor` next to it: nothing can start the engine.
    let alone = m.f.root.join("alone");
    std::fs::create_dir(&alone).unwrap();
    let copy = alone.join("raptor-mcp");
    std::fs::copy(server(), &copy).unwrap();
    let start = Instant::now();
    let result = m.mcp_status_with(&copy, &m.f.repo, true);
    assert!(start.elapsed() < Duration::from_secs(30));
    assert_refused(
        &result,
        "engine-unavailable",
        "check-the-gitraptor-installation",
        &[path(&m.f.repo)],
    );
    assert!(!m.running());
}
