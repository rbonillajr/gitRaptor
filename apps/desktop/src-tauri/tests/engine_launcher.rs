//! DSK002-LAUNCHER (TS-DSK-002 T006): `BundleConnector`, the real `Connector` of the app, runs
//! the bundle's `raptor daemon ensure` with a fixed argv and an empty environment plus `PATH`,
//! reads its `EnsureReport` and connects to the runtime folder it names.
//!
//! The `raptor` is a fake script (`support/fake_raptor.rs`) injected with
//! `BundleConnector::for_tests`; the daemon is a test socket server. Temporary folders only
//! (NFR-01).
#![cfg(unix)]

#[path = "support/fake_raptor.rs"]
mod fake_raptor;

use std::path::{Path, PathBuf};

use fake_raptor::{FakeRaptor, server_pid};
use gitraptor_desktop::engine::launcher::BundleConnector;
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, IncompatibilityKind, Link};

/// The fixed `PATH` of `run_ensure` (DS-TS-DSK-002 T006).
const FIXED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

const REQUIRES: [&str; 8] = [
    "--require",
    "scope.activity",
    "--require",
    "observation.tiers",
    "--require",
    "prediction.pairs",
    "--require",
    "connection.requester",
];

/// Every peer is taken as an installed `raptor`: the test server is this test process.
fn any_peer(_pid: u32) -> bool {
    true
}

fn connector(fake: &FakeRaptor) -> BundleConnector {
    BundleConnector::for_tests(fake.raptor.clone(), any_peer)
}

fn name(failure: &ConnectFailure) -> &'static str {
    match failure {
        ConnectFailure::NotRunning => "NotRunning",
        ConnectFailure::LaunchFailed => "LaunchFailed",
        ConnectFailure::Incompatible(_) => "Incompatible",
        ConnectFailure::Rejected => "Rejected",
        ConnectFailure::Unsupported => "Unsupported",
        ConnectFailure::Lost => "Lost",
    }
}

fn failure(result: Result<Box<dyn Link>, ConnectFailure>, what: &str) -> ConnectFailure {
    match result {
        Err(failure) => failure,
        Ok(_) => panic!("DSK002-LAUNCHER: {what}: connected instead of failing"),
    }
}

fn linked(result: Result<Box<dyn Link>, ConnectFailure>, what: &str) -> Box<dyn Link> {
    match result {
        Ok(link) => link,
        Err(failure) => panic!("DSK002-LAUNCHER: {what}: failed with {}", name(&failure)),
    }
}

fn argv(extra: &[&str]) -> Vec<String> {
    ["daemon", "ensure", "--json"]
        .iter()
        .chain(extra)
        .chain(REQUIRES.iter())
        .map(|s| (*s).to_owned())
        .collect()
}

#[test]
fn dsk002_launcher_starting_runs_ensure_with_the_exact_argv() {
    let fake = FakeRaptor::printing_fixture("ready-started.json");
    let _ = connector(&fake).connect(true);
    assert_eq!(
        fake.calls(),
        vec![argv(&[])],
        "DSK002-LAUNCHER: the argv of `raptor` when starting is not exact"
    );
}

#[test]
fn dsk002_launcher_connecting_without_starting_passes_no_start() {
    let fake = FakeRaptor::printing_fixture("ready.json");
    let _ = connector(&fake).connect(false);
    assert_eq!(
        fake.calls(),
        vec![argv(&["--no-start"])],
        "DSK002-LAUNCHER: the argv of `raptor` without starting is not exact (--no-start)"
    );
}

#[test]
fn dsk002_launcher_the_environment_is_only_path() {
    let fake = FakeRaptor::printing_fixture("ready.json");
    let _ = connector(&fake).connect(true);
    assert_eq!(
        fake.calls().len(),
        1,
        "DSK002-LAUNCHER: `raptor` did not run once"
    );
    let env = fake.env();
    let expected = std::collections::BTreeMap::from([("PATH".to_owned(), FIXED_PATH.to_owned())]);
    assert_eq!(
        env, expected,
        "DSK002-LAUNCHER: the environment of `raptor` is not only the fixed PATH"
    );
}

#[test]
fn dsk002_launcher_a_ready_report_connects_to_its_runtime() {
    let fake = FakeRaptor::printing_fixture("ready.json");
    let link = linked(connector(&fake).connect(true), "ready");
    assert_eq!(
        link.hello().daemon_pid,
        server_pid(),
        "DSK002-LAUNCHER: the link is not greeted by the daemon of the report's runtime"
    );
    assert!(fake.connections() >= 1);
}

#[test]
fn dsk002_launcher_a_daemon_older_report_is_incompatible_with_its_path() {
    let fake = FakeRaptor::printing_fixture("daemon-older.json");
    match failure(connector(&fake).connect(true), "daemon-older") {
        ConnectFailure::Incompatible(inc) => {
            assert!(
                matches!(inc.which, IncompatibilityKind::DaemonOlder),
                "DSK002-LAUNCHER: daemon-older is not DaemonOlder"
            );
            assert_eq!(
                inc.daemon_path.as_deref(),
                Some("/opt/homebrew/bin/raptor"),
                "DSK002-LAUNCHER: daemon_path is not the report's"
            );
            assert_eq!(
                inc.daemon_version.as_deref(),
                Some("0.0.9"),
                "DSK002-LAUNCHER: daemon_version is not the report's"
            );
        }
        other => panic!(
            "DSK002-LAUNCHER: daemon-older gave {}, not Incompatible",
            name(&other)
        ),
    }
}

#[test]
fn dsk002_launcher_a_not_running_report_is_not_running() {
    let fake = FakeRaptor::printing_fixture("not-running.json");
    let got = failure(connector(&fake).connect(false), "not-running");
    assert_eq!(
        name(&got),
        "NotRunning",
        "DSK002-LAUNCHER: not-running is not NotRunning"
    );
}

#[test]
fn dsk002_launcher_an_output_that_is_not_json_is_launch_failed() {
    let fake = FakeRaptor::printing("raptor: something went wrong\n");
    let got = failure(connector(&fake).connect(true), "output not JSON");
    assert_eq!(
        name(&got),
        "LaunchFailed",
        "DSK002-LAUNCHER: an output that is not JSON is not LaunchFailed"
    );
}

#[test]
fn dsk002_launcher_a_raptor_that_does_not_answer_in_time_is_launch_failed() {
    let fake = FakeRaptor::sleeping(11);
    let got = failure(connector(&fake).connect(true), "script sleeping 11 s");
    assert_eq!(
        name(&got),
        "LaunchFailed",
        "DSK002-LAUNCHER: a `raptor` sleeping 11 s is not LaunchFailed"
    );
}

#[test]
fn dsk002_launcher_without_raptor_next_to_the_executable_is_launch_failed() {
    let exe = std::env::current_exe().unwrap();
    assert!(
        !exe.with_file_name("raptor").exists(),
        "precondition: no `raptor` next to the test executable {}",
        exe.display()
    );
    let got = failure(BundleConnector::new().connect(true), "no bundled raptor");
    assert_eq!(
        name(&got),
        "LaunchFailed",
        "DSK002-LAUNCHER: without `raptor` next to the executable it is not LaunchFailed"
    );
}

#[test]
fn dsk002_launcher_a_second_connection_without_starting_does_not_run_raptor() {
    let fake = FakeRaptor::printing_fixture("ready.json");
    let mut connector = connector(&fake);
    drop(linked(connector.connect(true), "first connection"));
    let link = linked(connector.connect(false), "second connection");
    assert_eq!(link.hello().daemon_pid, server_pid());
    assert_eq!(
        fake.calls().len(),
        1,
        "DSK002-LAUNCHER: the second connection without starting ran `raptor` again"
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn dsk002_launcher_for_tests_is_only_called_from_tests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let launcher = root.join("src/engine/launcher.rs");
    let definition = std::fs::read_to_string(&launcher).unwrap_or_default();
    assert!(
        definition.contains("pub fn for_tests("),
        "DSK002-LAUNCHER: {} does not define `pub fn for_tests(`",
        launcher.display()
    );
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.push(root.join("build.rs"));
    let mut offenders = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !code.contains("for_tests(") {
                continue;
            }
            if file == launcher && code.contains("fn for_tests(") {
                continue;
            }
            offenders.push(format!("{}:{}", file.display(), n + 1));
        }
    }
    assert!(
        offenders.is_empty(),
        "DSK002-LAUNCHER: `for_tests(` is used outside tests/: {offenders:?}"
    );
}
