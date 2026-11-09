//! DSK002-ENSURE-CLI (TS-DSK-002 T004): `raptor daemon ensure --json` with temporary profiles
//! (`GITRAPTOR_PROFILE_DIR`, honored only in debug builds). Never this repo nor the real profile
//! (NFR-01); without `GITRAPTOR_AUTOSTART_DIR` the autostart of the user is never touched.
//!
//! "A daemon of another path" is a test server of this test process in the profile's runtime
//! folder: another executable than `raptor`, so `ensure` must not ask it to step down, and the
//! server records every method it receives.
//!
//! Unix only: the channel is a Unix socket.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::client::transport;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_core::daemon::LOG_FILE;
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// Executable name of the simulated agent (`GITRAPTOR_AGENT_EXECUTABLES`): the Claude Code
/// session that may run these tests is not taken for an agent.
const FAKE_AGENT: &str = "raptor-fake-agent";

/// Exit codes of `ensure` (DS-TS-DSK-002 § Informe de `ensure`).
const EXIT_READY: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_INCOMPATIBLE: i32 = 3;
const EXIT_NOT_RUNNING: i32 = 5;

struct Fixture {
    tmp: tempfile::TempDir,
    /// Daemons to stop when the test ends.
    daemons: Vec<u32>,
    children: Vec<Child>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for pid in &self.daemons {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
            daemons: Vec::new(),
            children: Vec::new(),
        }
    }

    fn profile_root(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(self.profile_root())
    }

    fn runtime(&self) -> PathBuf {
        self.dirs().runtime.expect("a runtime folder")
    }

    /// `raptor <args>` with exactly the profile override, the agent classifier and `PATH`.
    fn command(&self, program: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GITRAPTOR_PROFILE_DIR", self.profile_root())
            .env("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn ensure(&self, args: &[&str]) -> Output {
        let mut argv = vec!["daemon", "ensure"];
        argv.extend_from_slice(args);
        self.command(Path::new(RAPTOR), &argv).output().unwrap()
    }

    /// `raptor daemon` in the background, waiting until it serves its socket.
    fn start_daemon(&mut self) -> u32 {
        let child = self
            .command(Path::new(RAPTOR), &["daemon"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        let socket = transport::socket_path(&self.runtime());
        let log = self.dirs().state.join(LOG_FILE);
        let start = Instant::now();
        loop {
            let logged = std::fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("daemon_started");
            if logged && socket.exists() && UnixStream::connect(&socket).is_ok() {
                return pid;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "the test daemon did not start serving its socket"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// A private runtime folder for a test server (the profile's root too: 0700).
    fn private_runtime(&self) -> PathBuf {
        let runtime = self.runtime();
        std::fs::create_dir_all(&runtime).unwrap();
        for dir in [self.profile_root(), runtime.clone()] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        runtime
    }
}

/// How a test server answers `hello`.
#[derive(Clone, Copy)]
enum Server {
    /// Protocol 8: `hello` fails with `INCOMPATIBLE_PROTOCOL`.
    Older,
    /// This protocol, serving no capability.
    SameWithoutCapabilities,
}

/// A test server in `runtime`; the returned list holds every method received, in order.
fn serve(runtime: &Path, server: Server) -> Arc<Mutex<Vec<String>>> {
    let listener = UnixListener::bind(transport::socket_path(runtime)).unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let log = Arc::clone(&log);
            std::thread::spawn(move || answer(stream, server, &log));
        }
    });
    received
}

fn answer(stream: UnixStream, server: Server, log: &Mutex<Vec<String>>) {
    let mut writer = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            return;
        };
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        log.lock().unwrap().push(method.clone());
        let id = request["id"].clone();
        let reply = match (method.as_str(), server) {
            (methods::HELLO, Server::Older) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {
                    "code": code::INCOMPATIBLE_PROTOCOL,
                    "message": "incompatible protocol",
                    "data": { "daemon_protocol": 8, "binary_version": "0.0.8" }
                }
            }),
            (methods::HELLO, Server::SameWithoutCapabilities) => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocol": gitraptor_api::PROTOCOL_VERSION,
                    "binary_version": "0.1.0",
                    "instance_id": "test-instance",
                    "daemon_pid": std::process::id(),
                    "profile": "full",
                    "max_message_bytes": 1_048_576,
                    "methods": [],
                    "capabilities": []
                }
            }),
            (methods::DAEMON_REPLACE, _) => json!({
                "jsonrpc": "2.0", "id": id, "result": { "stopping": false }
            }),
            _ => json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method not found" }
            }),
        };
        let mut out = serde_json::to_vec(&reply).unwrap();
        out.push(b'\n');
        if writer.write_all(&out).is_err() {
            return;
        }
    }
}

/// The single JSON line `ensure` wrote, as an object.
fn report(out: &Output) -> serde_json::Map<String, Value> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "DSK002-ENSURE-CLI: `raptor daemon ensure` did not write exactly one report line \
         (exit {:?}); stdout: {stdout:?}; stderr: {:?}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    match serde_json::from_str::<Value>(lines[0]) {
        Ok(Value::Object(map)) => map,
        _ => panic!("DSK002-ENSURE-CLI: the report line is not a JSON object: {stdout:?}"),
    }
}

/// The keys of an object, and of its nested objects as `outer.inner`.
fn keys(map: &serde_json::Map<String, Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (key, value) in map {
        out.insert(key.clone());
        if let Value::Object(inner) = value {
            for k in inner.keys() {
                out.insert(format!("{key}.{k}"));
            }
        }
    }
    out
}

fn fixture_keys(name: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/api/tests/fixtures/ensure")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    match serde_json::from_str::<Value>(&text).unwrap() {
        Value::Object(map) => keys(&map),
        _ => panic!("fixture {name} is not an object"),
    }
}

fn assert_same_keys(report: &serde_json::Map<String, Value>, fixture: &str) {
    assert_eq!(
        keys(report),
        fixture_keys(fixture),
        "DSK002-ENSURE-CLI: the report line does not have the keys of {fixture}"
    );
}

fn pid_of(report: &serde_json::Map<String, Value>) -> Option<u32> {
    report
        .get("daemon_pid")
        .and_then(Value::as_u64)
        .and_then(|p| u32::try_from(p).ok())
}

fn replaces(received: &Mutex<Vec<String>>) -> usize {
    received
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.as_str() == methods::DAEMON_REPLACE)
        .count()
}

#[test]
fn dsk002_ensure_cli_stopped_engine_with_no_start_is_not_running_exit_5() {
    let fx = Fixture::new();
    let out = fx.ensure(&["--json", "--no-start"]);
    let report = report(&out);
    assert_eq!(
        report.get("outcome"),
        Some(&json!("not-running")),
        "DSK002-ENSURE-CLI: a stopped engine with --no-start is not `not-running`"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_NOT_RUNNING),
        "DSK002-ENSURE-CLI: `not-running` does not exit with 5"
    );
    assert_same_keys(&report, "not-running.json");
}

#[test]
fn dsk002_ensure_cli_stopped_engine_is_started_and_ready() {
    let mut fx = Fixture::new();
    let out = fx.ensure(&["--json"]);
    let report = report(&out);
    if let Some(pid) = pid_of(&report) {
        fx.daemons.push(pid);
    }
    assert_eq!(
        report.get("outcome"),
        Some(&json!("ready")),
        "DSK002-ENSURE-CLI: a stopped engine is not started to `ready`"
    );
    assert_eq!(
        report.get("started"),
        Some(&json!(true)),
        "DSK002-ENSURE-CLI: `started` is not true after starting the engine"
    );
    assert!(
        pid_of(&report).is_some(),
        "DSK002-ENSURE-CLI: `daemon_pid` of a ready engine is missing"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_READY),
        "DSK002-ENSURE-CLI: `ready` does not exit with 0"
    );
    assert_same_keys(&report, "ready-started.json");
}

#[test]
fn dsk002_ensure_cli_running_engine_is_ready_without_starting() {
    let mut fx = Fixture::new();
    let daemon = fx.start_daemon();
    let out = fx.ensure(&["--json"]);
    let report = report(&out);
    assert_eq!(
        report.get("outcome"),
        Some(&json!("ready")),
        "DSK002-ENSURE-CLI: a running engine is not `ready`"
    );
    assert_eq!(
        report.get("started"),
        Some(&json!(false)),
        "DSK002-ENSURE-CLI: `started` is true although the engine was running"
    );
    assert_eq!(
        pid_of(&report),
        Some(daemon),
        "DSK002-ENSURE-CLI: `daemon_pid` is not the running engine's"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_READY),
        "DSK002-ENSURE-CLI: `ready` does not exit with 0"
    );
    assert_same_keys(&report, "ready.json");
}

#[test]
fn dsk002_ensure_cli_an_older_daemon_of_another_path_is_daemon_older_and_never_replaced() {
    let fx = Fixture::new();
    let runtime = fx.private_runtime();
    let received = serve(&runtime, Server::Older);
    let out = fx.ensure(&["--json"]);
    let report = report(&out);
    assert_eq!(
        report.get("outcome"),
        Some(&json!("incompatible")),
        "DSK002-ENSURE-CLI: an older daemon is not `incompatible`"
    );
    assert_eq!(
        report.get("incompatible").and_then(|i| i.get("which")),
        Some(&json!("daemon-older")),
        "DSK002-ENSURE-CLI: an older daemon is not `daemon-older`"
    );
    assert_eq!(
        report.get("daemon_version"),
        Some(&json!("0.0.8")),
        "DSK002-ENSURE-CLI: `daemon_version` is not the IncompatibleData's"
    );
    assert_eq!(
        report.get("daemon_protocol"),
        Some(&json!(8)),
        "DSK002-ENSURE-CLI: `daemon_protocol` is not the daemon's"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_INCOMPATIBLE),
        "DSK002-ENSURE-CLI: `incompatible` does not exit with 3"
    );
    assert!(
        received.lock().unwrap().iter().any(|m| m == methods::HELLO),
        "DSK002-ENSURE-CLI: `ensure` never greeted the test server"
    );
    assert_eq!(
        replaces(&received),
        0,
        "DSK002-ENSURE-CLI: `ensure` sent daemon.replace to a daemon of another path"
    );
    assert_same_keys(&report, "daemon-older.json");
}

#[test]
fn dsk002_ensure_cli_a_daemon_of_another_path_missing_a_required_capability_is_never_replaced() {
    let fx = Fixture::new();
    let runtime = fx.private_runtime();
    let received = serve(&runtime, Server::SameWithoutCapabilities);
    let wanted = methods::CAP_SCOPE_ACTIVITY.name;
    let out = fx.ensure(&["--json", "--require", wanted]);
    let report = report(&out);
    assert_eq!(
        report.get("outcome"),
        Some(&json!("incompatible")),
        "DSK002-ENSURE-CLI: a daemon missing a required capability is not `incompatible`"
    );
    assert_eq!(
        report.get("incompatible").and_then(|i| i.get("which")),
        Some(&json!("missing-capabilities")),
        "DSK002-ENSURE-CLI: a daemon missing a required capability is not `missing-capabilities`"
    );
    assert_eq!(
        report.get("incompatible").and_then(|i| i.get("missing")),
        Some(&json!([wanted])),
        "DSK002-ENSURE-CLI: `missing` is not the required capability the daemon lacks"
    );
    assert_eq!(
        out.status.code(),
        Some(EXIT_INCOMPATIBLE),
        "DSK002-ENSURE-CLI: `incompatible` does not exit with 3"
    );
    assert!(
        received.lock().unwrap().iter().any(|m| m == methods::HELLO),
        "DSK002-ENSURE-CLI: `ensure` never greeted the test server"
    );
    assert_eq!(
        replaces(&received),
        0,
        "DSK002-ENSURE-CLI: `ensure` sent daemon.replace to a daemon of another path"
    );
    assert_same_keys(&report, "missing-capabilities.json");
}

#[test]
fn dsk002_ensure_cli_without_json_is_a_usage_error() {
    let fx = Fixture::new();
    let without = fx.ensure(&["--no-start"]);
    assert_eq!(
        without.status.code(),
        Some(EXIT_USAGE),
        "DSK002-ENSURE-CLI: `daemon ensure` without --json does not exit with 2"
    );
    assert!(
        without.stdout.is_empty(),
        "DSK002-ENSURE-CLI: `daemon ensure` without --json wrote a report"
    );
    // The 2 comes from the missing --json, not from an unknown subcommand: with it, the same
    // call is accepted.
    let with = fx.ensure(&["--json", "--no-start"]);
    assert_ne!(
        with.status.code(),
        Some(EXIT_USAGE),
        "DSK002-ENSURE-CLI: `daemon ensure --json --no-start` is a usage error: {:?}",
        String::from_utf8_lossy(&with.stderr)
    );
}
