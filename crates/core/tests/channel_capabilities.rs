//! ADR-GRP-016 § 1: capabilities in the handshake, `connection.accept` and
//! the replacement of a daemon of the same protocol, with a real daemon
//! (in-process) over a temporary profile (NFR-01).
#![cfg(target_os = "macos")]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::capability::{AcceptResult, CAPABILITIES_PROTOCOL};
use gitraptor_api::messages::{ClientKind, SubscribeResult};
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig) -> Self {
        let protocol = channel.protocol;
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
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        let r = Self {
            dirs,
            handle,
            join: Some(join),
        };
        drop(r.connect(protocol));
        r
    }

    fn connect(&self, protocol: u32) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, protocol) {
                Ok(client) => return client,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    fn raw(&self) -> Raw {
        Raw::open(&gitraptor_core::client::socket_path(&self.dirs).unwrap())
    }

    fn join(mut self) -> StopReport {
        self.join.take().unwrap().join().unwrap()
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

/// A raw connection: a line out, a line in.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Raw {
    fn open(path: &Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        Self { stream, reader }
    }

    fn call(&mut self, id: u64, method: &str, params: Value) -> Value {
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        let mut answer = String::new();
        self.reader.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap()
    }

    fn hello(&mut self, protocol: u32) -> Value {
        self.call(
            0,
            methods::HELLO,
            json!({"protocol": protocol, "client": "cli", "client_version": "t"}),
        )
    }
}

fn rpc_code(err: ClientError) -> i64 {
    match err {
        ClientError::Rpc(e) => e.code,
        other => panic!("expected an RPC error, got {other}"),
    }
}

/// Only a connection of protocol 9 hears of capabilities: 5 to 8 reject
/// unknown fields in the handshake's result.
#[test]
fn only_protocol_9_hears_of_capabilities() {
    let tp = TempProfile::new();
    let r = Running::start(tp.dirs(), ChannelConfig::default());
    for protocol in 5..CAPABILITIES_PROTOCOL {
        let hello = r.raw().hello(protocol);
        let result = &hello["result"];
        assert!(result.is_object(), "{hello}");
        assert!(result.get("capabilities").is_none(), "{protocol}: {hello}");
    }
    let hello = r.raw().hello(CAPABILITIES_PROTOCOL);
    let announced: Vec<&str> = hello["result"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    for c in gitraptor_api::capability::all() {
        assert!(announced.contains(&c.name), "{}", c.name);
    }
    let methods: Vec<&str> = hello["result"]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert!(methods.contains(&methods::CONNECTION_ACCEPT));
}

/// `connection.accept` grants what the daemon serves, ignores the rest, is
/// taken once and only before the first subscription; protocol 8 does not
/// have it.
#[test]
fn accept_grants_what_the_daemon_serves_once_before_subscribing() {
    let tp = TempProfile::new();
    let r = Running::start(tp.dirs(), ChannelConfig::default());
    let mut client = r.connect(PROTOCOL_VERSION);
    let granted: AcceptResult = client
        .call(
            methods::CONNECTION_ACCEPT,
            json!({"capabilities": ["events.git-reset", "nobody.knows-this"]}),
        )
        .unwrap();
    assert!(
        granted
            .capabilities
            .contains(&"events.git-reset".to_owned())
    );
    assert!(
        granted
            .capabilities
            .contains(&"connection.requester".to_owned())
    );
    assert!(!granted.capabilities.iter().any(|c| c.starts_with("nobody")));
    let again = client
        .call::<_, AcceptResult>(methods::CONNECTION_ACCEPT, json!({"capabilities": []}))
        .unwrap_err();
    assert_eq!(rpc_code(again), code::INVALID_REQUEST);

    let mut subscribed = r.connect(PROTOCOL_VERSION);
    let _: SubscribeResult = subscribed
        .call(methods::EVENTS_SUBSCRIBE, json!({"from_seq": 1}))
        .unwrap();
    let late = subscribed
        .call::<_, AcceptResult>(methods::CONNECTION_ACCEPT, json!({"capabilities": []}))
        .unwrap_err();
    assert_eq!(rpc_code(late), code::INVALID_REQUEST);

    let mut old = r.connect(8);
    let missing = old
        .call::<_, AcceptResult>(methods::CONNECTION_ACCEPT, json!({"capabilities": []}))
        .unwrap_err();
    assert_eq!(rpc_code(missing), code::METHOD_NOT_FOUND);

    let mut flood = r.connect(PROTOCOL_VERSION);
    let names: Vec<String> = (0..=gitraptor_api::capability::MAX_ACCEPTED)
        .map(|i| format!("x.{i}"))
        .collect();
    let refused = flood
        .call::<_, AcceptResult>(methods::CONNECTION_ACCEPT, json!({"capabilities": names}))
        .unwrap_err();
    assert_eq!(rpc_code(refused), code::INVALID_PARAMS);
}

/// A client knows which capabilities of its own an older daemon of its
/// protocol lacks; a daemon of 5 to 8 announces none, so none is missing.
#[test]
fn a_client_sees_what_an_older_daemon_of_its_protocol_lacks() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            capabilities: vec!["connection.requester"],
            ..ChannelConfig::default()
        },
    );
    let known = ["connection.requester", "events.git-reset"];
    let client = r.connect(PROTOCOL_VERSION);
    assert_eq!(client.missing_capabilities(&known), ["events.git-reset"]);
    let old = r.connect(8);
    assert!(old.missing_capabilities(&known).is_empty());
}

/// `ensure_daemon` asks such a daemon to step down; one that refuses (the
/// caller is not the installed binary) keeps serving the connection with
/// what it has, without a loop.
#[test]
fn a_daemon_that_refuses_the_replacement_keeps_serving() {
    let tp = TempProfile::new();
    let bin = tempfile::tempdir().unwrap();
    let launch = bin.path().join("raptor");
    std::os::unix::fs::symlink("/usr/bin/true", &launch).unwrap();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            launch_exe: Some(launch),
            capabilities: Vec::new(),
            ..ChannelConfig::default()
        },
    );
    let mut options = ClientOptions::new(r.dirs.clone(), ClientKind::Cli);
    options.capabilities = vec!["events.git-reset"];
    let mut client = ensure_daemon(&options).unwrap();
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

/// From protocol 9 the installed binary replaces a daemon of its own
/// protocol (an upgrade that only added capabilities); before 9, and from
/// anything but the installed binary, the same protocol is refused.
#[test]
fn the_installed_binary_replaces_a_daemon_of_its_protocol_from_9() {
    let tp = TempProfile::new();
    let bin = tempfile::tempdir().unwrap();
    let launch = bin.path().join("raptor");
    std::os::unix::fs::symlink("/usr/bin/true", &launch).unwrap();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            launch_exe: Some(launch.clone()),
            ..ChannelConfig::default()
        },
    );
    // Not an upgrade yet: refused, and the connection goes on.
    let mut raw = r.raw();
    assert!(raw.hello(PROTOCOL_VERSION)["result"].is_object());
    let refused = raw.call(
        1,
        methods::DAEMON_REPLACE,
        json!({"protocol": PROTOCOL_VERSION}),
    );
    assert_eq!(refused["error"]["code"], code::INVALID_PARAMS, "{refused}");
    let pong = raw.call(2, methods::PING, json!({}));
    assert_eq!(pong["result"], "pong");

    // "Upgrade": the launch path now resolves to this test binary.
    std::fs::remove_file(&launch).unwrap();
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &launch).unwrap();
    let accepted = raw.call(
        3,
        methods::DAEMON_REPLACE,
        json!({"protocol": PROTOCOL_VERSION}),
    );
    assert_eq!(accepted["result"]["stopping"], true, "{accepted}");
    let report = r.join();
    assert!(matches!(report.cause, StopCause::Replace { .. }));
}

/// A daemon of protocol 8 keeps the old rule: only a newer protocol.
#[test]
fn before_9_the_same_protocol_never_replaces() {
    let tp = TempProfile::new();
    let bin = tempfile::tempdir().unwrap();
    let launch = bin.path().join("raptor");
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &launch).unwrap();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            launch_exe: Some(launch),
            protocol: 8,
            ..ChannelConfig::default()
        },
    );
    let mut raw = r.raw();
    assert!(raw.hello(8)["result"].is_object());
    let refused = raw.call(1, methods::DAEMON_REPLACE, json!({"protocol": 8}));
    assert_eq!(refused["error"]["code"], code::INVALID_PARAMS, "{refused}");
}
