//! A connection that never accepted `operation.snapshot` is told the operation is not
//! implemented for it, and that the story that implements it is `US-MCP-008`. Real daemon
//! (in-process) over a temporary profile (NFR-01).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use common::TempProfile;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, socket_path};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

fn call(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    stream.write_all(line.to_string().as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    loop {
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        let answer: Value = serde_json::from_str(&answer).unwrap();
        if answer["id"] == id {
            return answer;
        }
    }
}

#[test]
fn prepare_without_the_capability_is_not_implemented() {
    let tp = TempProfile::new();
    let dirs: ProfileDirs = tp.dirs();
    let env = DaemonEnv::from_vars(Vec::new());
    let daemon = Daemon::start(DaemonConfig {
        dirs: dirs.clone(),
        git: ResolveConfig {
            configured_path: None,
            path_env: None,
            known_locations: Vec::new(),
            shim_paths: Vec::new(),
            toolchain_gits: Vec::new(),
        },
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: Default::default(),
    })
    .unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    // The daemon is up once the library client can connect.
    let mut connected = None;
    for _ in 0..500 {
        if let Ok(c) = Client::connect(
            &dirs,
            gitraptor_api::messages::ClientKind::Cli,
            PROTOCOL_VERSION,
        ) {
            connected = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(connected.expect("the daemon accepts connections"));

    // A raw connection never sends `connection.accept`: it holds no capability.
    let mut stream = UnixStream::connect(socket_path(&dirs).unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    call(
        &mut stream,
        &mut reader,
        0,
        methods::HELLO,
        json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
    );
    let answer = call(
        &mut stream,
        &mut reader,
        1,
        methods::OPERATION_PREPARE,
        json!({
            "operation": "snapshot",
            "worktree": tp.root.path().join("w"),
            "args": {"label": "before"}
        }),
    );

    assert_eq!(answer["error"]["code"], code::NOT_IMPLEMENTED, "{answer}");
    assert_eq!(
        answer["error"]["data"]["implemented_by"], "US-MCP-008",
        "{answer}"
    );

    handle.request(StopCause::Signal("TERM"));
    let _ = join.join();
}
