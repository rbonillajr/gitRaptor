//! DSK002-POLICY (TS-DSK-002 T002, SEC-13): `ensure_daemon_policy` with `ReplacePolicy::OnlyIf`
//! asks the policy before sending `daemon.replace`, and never sends it when the policy refuses.
//!
//! A test server in a private temporary folder (NFR-01) plays the daemon and records every
//! method it receives.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::client::{
    ClientError, Connect, Ensured, Launch, ReplacePolicy, ensure_daemon_policy, transport,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use serde_json::{Value, json};

/// How the test server answers `hello`.
#[derive(Clone, Copy)]
enum Daemon {
    /// Protocol 8: `hello` fails with `INCOMPATIBLE_PROTOCOL`.
    Older,
    /// This protocol, serving no capability.
    SameWithoutCapabilities,
}

/// A test server in `runtime`; the returned list holds every method received, in order.
fn serve(runtime: &Path, daemon: Daemon) -> Arc<Mutex<Vec<String>>> {
    let listener = UnixListener::bind(transport::socket_path(runtime)).unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let log = Arc::clone(&log);
            std::thread::spawn(move || answer(stream, daemon, &log));
        }
    });
    received
}

fn answer(stream: UnixStream, daemon: Daemon, log: &Mutex<Vec<String>>) {
    let mut writer = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            return;
        };
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        log.lock().unwrap().push(method.clone());
        let id = request["id"].clone();
        let reply = match (method.as_str(), daemon) {
            (methods::HELLO, Daemon::Older) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {
                    "code": code::INCOMPATIBLE_PROTOCOL,
                    "message": "incompatible protocol",
                    "data": { "daemon_protocol": 8, "binary_version": "0.0.8" }
                }
            }),
            (methods::HELLO, Daemon::SameWithoutCapabilities) => json!({
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
            // Refuses to step down: the client goes on (same protocol) or reports
            // the incompatibility (older protocol).
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

fn private_runtime() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    (tmp, runtime)
}

/// A launcher that counts its calls and never starts anything.
#[derive(Default)]
struct CountingLaunch {
    launches: usize,
}

impl Launch for CountingLaunch {
    fn launch(&mut self) -> Result<(), ClientError> {
        self.launches += 1;
        Err(ClientError::NotRunning)
    }

    fn wait_released(&mut self, _timeout: Duration) -> bool {
        false
    }
}

fn connect(runtime: &Path, capabilities: Vec<&'static str>) -> Connect {
    let mut connect = Connect::new(runtime.to_path_buf(), ClientKind::Cli);
    connect.start_timeout = Duration::from_secs(2);
    connect.capabilities = capabilities;
    connect
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
fn dsk002_policy_an_older_daemon_refused_by_the_policy_gets_no_replace() {
    let (_tmp, runtime) = private_runtime();
    let received = serve(&runtime, Daemon::Older);
    let mut launch = CountingLaunch::default();
    let mut refuse = |_pid: u32| false;
    let result = ensure_daemon_policy(
        &connect(&runtime, vec![]),
        &mut launch,
        &mut || {},
        ReplacePolicy::OnlyIf(&mut refuse),
    );
    match result {
        Err(ClientError::Incompatible(data)) => assert_eq!(data.daemon_protocol, 8),
        Err(other) => panic!("DSK002-POLICY: expected Incompatible, got {other:?}"),
        Ok(_) => panic!("DSK002-POLICY: expected Incompatible, got a client"),
    }
    assert!(
        received.lock().unwrap().iter().any(|m| m == methods::HELLO),
        "DSK002-POLICY: the test server was never greeted"
    );
    assert_eq!(
        replaces(&received),
        0,
        "DSK002-POLICY: daemon.replace was sent although the policy refused"
    );
    assert_eq!(launch.launches, 0, "DSK002-POLICY: a daemon was launched");
}

#[test]
fn dsk002_policy_an_older_daemon_accepted_by_the_policy_gets_replace() {
    let (_tmp, runtime) = private_runtime();
    let received = serve(&runtime, Daemon::Older);
    let mut launch = CountingLaunch::default();
    let mut accept = |_pid: u32| true;
    let _ = ensure_daemon_policy(
        &connect(&runtime, vec![]),
        &mut launch,
        &mut || {},
        ReplacePolicy::OnlyIf(&mut accept),
    );
    assert_eq!(
        replaces(&received),
        1,
        "DSK002-POLICY: daemon.replace was not sent although the policy accepted"
    );
}

#[test]
fn dsk002_policy_a_same_protocol_daemon_missing_a_capability_is_kept_when_refused() {
    let (_tmp, runtime) = private_runtime();
    let received = serve(&runtime, Daemon::SameWithoutCapabilities);
    let mut launch = CountingLaunch::default();
    let mut refuse = |_pid: u32| false;
    let wanted = methods::CAP_SCOPE_ACTIVITY.name;
    let result = ensure_daemon_policy(
        &connect(&runtime, vec![wanted]),
        &mut launch,
        &mut || {},
        ReplacePolicy::OnlyIf(&mut refuse),
    );
    let Ensured {
        client,
        started,
        replaced,
    } = match result {
        Ok(ensured) => ensured,
        Err(err) => panic!("DSK002-POLICY: expected the client as is, got {err:?}"),
    };
    assert!(!started, "DSK002-POLICY: started is true without a launch");
    assert!(
        !replaced,
        "DSK002-POLICY: replaced is true without a replace"
    );
    assert_eq!(
        client.missing_capabilities(&[wanted]),
        vec![wanted],
        "DSK002-POLICY: the client returned is not the one of the daemon missing the capability"
    );
    assert_eq!(
        replaces(&received),
        0,
        "DSK002-POLICY: daemon.replace was sent although the policy refused"
    );
    assert_eq!(launch.launches, 0, "DSK002-POLICY: a daemon was launched");
}
