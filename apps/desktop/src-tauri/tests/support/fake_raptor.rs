//! A fake `raptor` for the tests of `BundleConnector` (TS-DSK-002 T006): a `#!/bin/sh` script
//! (0755) in a temporary folder that records its argv and its environment and prints a fixture
//! of `crates/api/tests/fixtures/ensure/` with `runtime` replaced by the folder of a test socket
//! server of its own, which answers the `hello` handshake. Temporary folders only (NFR-01).
//!
//! Included with `#[path]` by the test files that need it; not a test target of its own.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gitraptor_api::client::transport;
use serde_json::{Value, json};

/// Variables `/bin/sh` creates by itself: not part of what the script was given.
pub const SHELL_OWN: &[&str] = &["PWD", "SHLVL", "_", "OLDPWD"];

/// Marks the start of one run in the argv record.
const CALL: &str = "--call--";

/// The `daemon_pid` the test server reports in its `hello`.
pub fn server_pid() -> u32 {
    std::process::id()
}

pub struct FakeRaptor {
    dir: tempfile::TempDir,
    /// The script to pass to `BundleConnector::for_tests`.
    pub raptor: PathBuf,
    /// The runtime folder of the test server, as written in the report.
    pub runtime: PathBuf,
    /// Connections the test server accepted.
    connections: Arc<AtomicUsize>,
}

impl FakeRaptor {
    /// Prints the fixture `name` with `runtime` replaced by the test server's folder.
    pub fn printing_fixture(name: &str) -> Self {
        let fake = Self::empty();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/api/tests/fixtures/ensure")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
        let mut report: Value = serde_json::from_str(&text).unwrap();
        report["runtime"] = json!(fake.runtime.to_str().unwrap());
        let mut line = serde_json::to_string(&report).unwrap();
        line.push('\n');
        fake.write_script(&format!(
            "/bin/cat '{}'\n",
            fake.output_file(&line).display()
        ));
        fake
    }

    /// Prints `text` as is.
    pub fn printing(text: &str) -> Self {
        let fake = Self::empty();
        fake.write_script(&format!(
            "/bin/cat '{}'\n",
            fake.output_file(text).display()
        ));
        fake
    }

    /// Prints nothing and sleeps `secs` seconds.
    pub fn sleeping(secs: u32) -> Self {
        let fake = Self::empty();
        fake.write_script(&format!("exec /bin/sleep {secs}\n"));
        fake
    }

    /// A temporary folder with the test server running in its private runtime folder.
    fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let connections = serve(&runtime);
        let raptor = dir.path().join("bin").join("raptor");
        std::fs::create_dir(raptor.parent().unwrap()).unwrap();
        Self {
            dir,
            raptor,
            runtime,
            connections,
        }
    }

    fn argv_log(&self) -> PathBuf {
        self.dir.path().join("argv.log")
    }

    fn env_log(&self) -> PathBuf {
        self.dir.path().join("env.log")
    }

    fn output_file(&self, text: &str) -> PathBuf {
        let path = self.dir.path().join("output.txt");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn write_script(&self, body: &str) {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' '{CALL}' \"$@\" >> '{argv}'\n/usr/bin/env > '{env}'\n{body}",
            argv = self.argv_log().display(),
            env = self.env_log().display(),
        );
        std::fs::write(&self.raptor, script).unwrap();
        std::fs::set_permissions(&self.raptor, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The argv of every run of the script, in order.
    pub fn calls(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.argv_log()).unwrap_or_default();
        let mut calls: Vec<Vec<String>> = Vec::new();
        for line in text.lines() {
            if line == CALL {
                calls.push(Vec::new());
            } else if let Some(last) = calls.last_mut() {
                last.push(line.to_owned());
            }
        }
        calls
    }

    /// The environment of the last run, without the variables `/bin/sh` creates itself.
    pub fn env(&self) -> BTreeMap<String, String> {
        let text = std::fs::read_to_string(self.env_log()).unwrap_or_default();
        text.lines()
            .filter_map(|l| l.split_once('='))
            .filter(|(k, _)| !SHELL_OWN.contains(k))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    /// Connections the test server accepted.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// A test server in `runtime` that answers `hello` like a daemon of this protocol.
fn serve(runtime: &Path) -> Arc<AtomicUsize> {
    let listener = UnixListener::bind(transport::socket_path(runtime)).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&connections);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            count.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || answer(stream));
        }
    });
    connections
}

fn answer(stream: UnixStream) {
    let mut writer = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            return;
        };
        let id = request["id"].clone();
        let reply = if request["method"] == gitraptor_api::methods::HELLO {
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocol": gitraptor_api::PROTOCOL_VERSION,
                    "binary_version": "0.1.0",
                    "instance_id": "test-instance",
                    "daemon_pid": server_pid(),
                    "profile": "full",
                    "max_message_bytes": 1_048_576,
                    "methods": []
                }
            })
        } else {
            json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method not found" }
            })
        };
        let mut out = serde_json::to_vec(&reply).unwrap();
        out.push(b'\n');
        if writer.write_all(&out).is_err() {
            return;
        }
    }
}
