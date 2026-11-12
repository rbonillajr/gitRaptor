//! TS-GRP-010 (DS-TS-GRP-010 § 9.4, file 2): the per-requester profile on the wire, with a real
//! daemon (in-process) over a temporary profile (NFR-01) and a client that is a child of it: this
//! test binary run again on an ignored entry, over a raw socket. An unmarked child of the daemon
//! is narrowed to `mcp` (D1 e) and keeps its declared channel and its own reserved verdict (D8);
//! the debug-only test knob (D3) keeps it `full`.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::messages::{AuditEntry, AuditListResult, ClientKind, RefusalReason};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

const CHILD_SOCKET: &str = "GITRAPTOR_TEST_TS_GRP_010_SOCKET";

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig) -> Self {
        let config = DaemonConfig {
            dirs: dirs.clone(),
            env: DaemonEnv::from_vars(Vec::new()),
            git: ResolveConfig {
                configured_path: None,
                path_env: None,
                known_locations: Vec::new(),
                shim_paths: Vec::new(),
                toolchain_gits: Vec::new(),
            },
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel,
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            power: Default::default(),
            predict: Default::default(),
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            dirs,
            handle,
            join: Some(join),
        }
    }

    /// A client of the daemon's own process (rule c: full).
    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    fn socket(&self) -> PathBuf {
        gitraptor_core::client::socket_path(&self.dirs).unwrap()
    }

    /// The child client's answers, one JSON value per request, in order.
    fn child_answers(&self) -> Vec<Value> {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child_client_entry",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_SOCKET, self.socket())
            .output()
            .unwrap();
        // The test harness writes text of its own around the answers.
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.find('{').map(|i| &l[i..]))
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v.get("jsonrpc").is_some())
            .collect()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

/// A matcher with no real agent: the Claude Code session that runs the tests is not one.
fn no_agents() -> AgentMatcher {
    AgentMatcher::only(vec!["raptor-fake-agent".into()])
}

/// Run as a child of the daemon by the tests of this file: over a raw socket, says hello as
/// `cli`, asks `requester.resolve` with the `tui` surface and `daemon.stop`, and prints the
/// answers, one per line.
#[test]
#[ignore = "run by the tests of ts_grp_010_channel"]
fn child_client_entry() {
    let socket = std::env::var_os(CHILD_SOCKET).expect("run by its parent test");
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
            "protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "zz-ts-grp-010"}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": methods::REQUESTER_RESOLVE,
            "params": {"surface": "tui"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": methods::DAEMON_STOP}),
    ];
    let mut answers = BufReader::new(stream.try_clone().unwrap());
    for request in requests {
        writeln!(stream, "{request}").unwrap();
        let mut line = String::new();
        if answers.read_line(&mut line).unwrap() == 0 {
            break;
        }
        print!("{line}");
    }
}

#[test]
fn narrowed_child_keeps_its_channel_and_reserved_verdict() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: no_agents(),
            ..ChannelConfig::default()
        },
    );
    drop(r.client());
    let answers = r.child_answers();
    assert_eq!(
        answers.len(),
        3,
        "the child must get three answers: {answers:#?}"
    );

    let hello = &answers[0]["result"];
    assert_eq!(
        hello["profile"], "mcp",
        "an unmarked child of the daemon is narrowed to mcp (D1 e): {hello}"
    );
    assert!(
        hello.get("requester").is_some_and(|r| !r.is_null()),
        "a narrowed cli keeps hello.requester (D8 iv): {hello}"
    );

    let resolved = &answers[1]["result"];
    assert_eq!(
        resolved["channel"], "tui",
        "a narrowed connection keeps its declared channel, never mcp (D8 i): {}",
        answers[1]
    );
    assert!(
        resolved.get("via").is_none() && resolved.get("confirmable").is_none(),
        "the mcp view of requester.resolve carries no via nor confirmable: {resolved}"
    );

    let stop = &answers[2];
    assert_eq!(
        stop["error"]["data"]["reason"], "daemon-descendant",
        "a reserved command gets its own verdict, not not-available-to-mcp (D8 ii): {stop}"
    );

    let mut client = r.client();
    let audit: AuditListResult = client
        .call(methods::AUDIT_LIST, json!({}))
        .expect("the daemon's own client keeps the audit");
    let entries: Vec<&AuditEntry> = audit
        .entries
        .iter()
        .filter(|e| e.operation == methods::DAEMON_STOP)
        .collect();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].reason, Some(RefusalReason::DaemonDescendant));
    assert!(
        entries[0].client.daemon_descendant,
        "the audit row keeps the walk of a narrowed connection: {:?}",
        entries[0]
    );
}

#[test]
fn test_knob_keeps_daemon_children_full() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: no_agents(),
            test_daemon_children_as_self: true,
            ..ChannelConfig::default()
        },
    );
    let own = r.client();
    let own_methods = own.hello().methods.clone();
    drop(own);
    let answers = r.child_answers();
    let hello = &answers[0]["result"];
    assert_eq!(
        hello["profile"], "full",
        "with the test knob an unmarked child of the daemon keeps full (D3): {hello}"
    );
    let child_methods: Vec<String> = serde_json::from_value(hello["methods"].clone()).unwrap();
    assert_eq!(
        child_methods, own_methods,
        "the child sees the methods of a cli of the daemon itself"
    );
}
