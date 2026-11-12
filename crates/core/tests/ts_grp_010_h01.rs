//! TS-GRP-010 H-01 (D12, security review of 2026-11-12): a connection narrowed to `mcp` whose
//! requester is NOT an agent (here an unmarked child of the in-process daemon; in production
//! `setsid`, a `node` in the chain or a detached multiplexer) meets the MCP's rules for writes:
//! the channel that authorizes is `mcp`, so an unattributed requester never undoes nor redoes,
//! whatever channel it declares. Real daemon in-process over a temporary profile (NFR-01).
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

const CHILD_SOCKET: &str = "GITRAPTOR_TEST_TS_GRP_010_H01_SOCKET";

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
                "h01_child_entry",
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

/// Run as a child of the daemon: says hello as `cli`, then asks `timemachine.undo` and
/// `timemachine.redo` as the CLI would (with the `cli` surface), and prints the answers.
#[test]
#[ignore = "run by the tests of ts_grp_010_h01"]
fn h01_child_entry() {
    let socket = std::env::var_os(CHILD_SOCKET).expect("run by its parent test");
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
            "protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "x"}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": methods::TM_UNDO,
            "params": {"surface": "cli"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": methods::TM_REDO,
            "params": {"surface": "cli"}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": methods::REQUESTER_RESOLVE,
            "params": {"surface": "hook"}}),
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

/// An unattributed requester narrowed to `mcp` never undoes nor redoes, as over MCP: refused
/// with `unattributed-over-mcp` before anything is computed, whatever channel it declares.
#[test]
fn an_unattributed_narrowed_cli_never_undoes_nor_redoes() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
            ..ChannelConfig::default()
        },
    );
    drop(r.client());
    let answers = r.child_answers();
    assert_eq!(answers.len(), 4, "{answers:#?}");
    assert!(
        answers[0]["result"]["requester"]
            .get("confirmable")
            .is_none_or(|c| c == false),
        "L-04: a narrowed connection never learns it could confirm: {}",
        answers[0]
    );
    assert_eq!(
        answers[3]["error"]["code"], -32602,
        "L-03: a narrowed connection may not label itself as the hook: {}",
        answers[3]
    );
    assert_eq!(
        answers[0]["result"]["profile"], "mcp",
        "fixture: an unmarked child of the daemon is narrowed: {}",
        answers[0]
    );
    for (answer, what) in [(&answers[1], "undo"), (&answers[2], "redo")] {
        assert_eq!(
            answer["error"]["data"]["reason"], "unattributed-over-mcp",
            "H-01: an unattributed narrowed {what} must be refused as over MCP: {answer}"
        );
    }
}
