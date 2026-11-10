//! TS-GRP-009 (DS-TS-GRP-009 § 9.4, file 3): the desktop class over the wire, with the daemon
//! in-process over a temporary profile (NFR-01). `ChannelConfig.test_app_binaries` is the file
//! name of this test's executable, so the in-process client is of the class. That client is the
//! daemon's own process, so the class always resolves to `mcp` with `daemon-descendant`: rows 1, 3
//! and 4 are driven with a process-table double in `ts_grp_009_desktop_profile.rs`.
//!
//! The arrange (daemon, raw connection, log poll) is the one of `channel_capabilities.rs`.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

const CLIENT_VERSION: &str = "zz-ts-grp-009";

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig) -> Self {
        let env = DaemonEnv::from_vars(Vec::new());
        let config = DaemonConfig {
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
        let running = Self {
            dirs,
            handle,
            join: Some(join),
        };
        running.wait_serving();
        running
    }

    /// The daemon of the class: the executable of this test is the app's.
    fn desktop(dirs: ProfileDirs) -> Self {
        let exe = std::env::current_exe().unwrap();
        Self::start(
            dirs,
            ChannelConfig {
                test_app_binaries: Some(vec![
                    exe.file_name().unwrap().to_string_lossy().into_owned(),
                ]),
                ..ChannelConfig::default()
            },
        )
    }

    /// A daemon without the test field: this test's executable is not of the class.
    fn plain(dirs: ProfileDirs) -> Self {
        Self::start(dirs, ChannelConfig::default())
    }

    fn wait_serving(&self) {
        let socket = gitraptor_core::client::socket_path(&self.dirs).unwrap();
        let start = Instant::now();
        while UnixStream::connect(&socket).is_err() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "daemon never served"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn raw(&self) -> Raw {
        Raw::open(&gitraptor_core::client::socket_path(&self.dirs).unwrap())
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs.state.join("daemon.log")).unwrap_or_default()
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
        // Notifications that arrive before the answer are skipped.
        loop {
            let mut answer = String::new();
            self.reader.read_line(&mut answer).unwrap();
            let answer: Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    }

    fn hello_as(&mut self, protocol: u32, client: &str) -> Value {
        self.call(
            0,
            methods::HELLO,
            json!({"protocol": protocol, "client": client, "client_version": CLIENT_VERSION}),
        )
    }

    fn hello(&mut self) -> Value {
        self.hello_as(PROTOCOL_VERSION, "cli")
    }
}

fn method_names(hello: &Value) -> BTreeSet<String> {
    hello["result"]["methods"]
        .as_array()
        .unwrap_or_else(|| panic!("hello has no methods: {hello}"))
        .iter()
        .map(|m| m.as_str().unwrap().to_owned())
        .collect()
}

fn keys(hello: &Value) -> BTreeSet<String> {
    hello["result"]
        .as_object()
        .unwrap_or_else(|| panic!("hello has no result: {hello}"))
        .keys()
        .cloned()
        .collect()
}

/// The methods the mcp profile offers on `protocol` (hello itself is never offered).
fn mcp_methods(protocol: u32) -> BTreeSet<String> {
    methods::METHODS
        .iter()
        .filter(|m| m.mcp && m.exists_in(protocol) && m.name != methods::HELLO)
        .map(|m| m.name.to_owned())
        .collect()
}

fn today_hello_keys(with_requester: bool) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = [
        "protocol",
        "binary_version",
        "instance_id",
        "daemon_pid",
        "profile",
        "max_message_bytes",
        "methods",
        "capabilities",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if with_requester {
        keys.insert("requester".to_owned());
    }
    keys
}

#[test]
fn desktop_class_is_narrowed_by_the_daemon() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let hello = r.raw().hello();
    assert_eq!(
        hello["result"]["profile"], "mcp",
        "a cli hello from the app's executable must be narrowed to mcp by the daemon: {hello}"
    );
    assert!(
        hello["result"]["requester"].is_object(),
        "the class must keep receiving the requester in hello: {hello}"
    );
    let allowed: BTreeSet<String> = mcp_methods(PROTOCOL_VERSION)
        .into_iter()
        .chain([methods::CONNECTION_DESKTOP.to_owned()])
        .collect();
    let extra: Vec<_> = method_names(&hello).difference(&allowed).cloned().collect();
    assert!(
        extra.is_empty(),
        "the class must see no method outside the mcp allowlist but connection.desktop: {extra:?}"
    );
}

#[test]
fn connection_desktop_reports_reason_and_identity() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let mut raw = r.raw();
    let hello = raw.hello();
    assert!(
        method_names(&hello).contains(methods::CONNECTION_DESKTOP),
        "hello.methods of the class must offer connection.desktop: {hello}"
    );
    let answer = raw.call(1, methods::CONNECTION_DESKTOP, json!({}));
    assert_eq!(
        answer["result"],
        json!({"reason": "daemon-descendant", "identity": "none"}),
        "connection.desktop must publish why the profile was fixed: {answer}"
    );
}

/// I-01: the class gets the requester a `cli` of the same process gets.
#[test]
fn desktop_requester_equals_cli_requester() {
    let class_tp = TempProfile::new();
    let class_hello = {
        let r = Running::desktop(class_tp.dirs());
        r.raw().hello()
    };
    assert_eq!(
        class_hello["result"]["profile"], "mcp",
        "fixture: the class must be in force (narrowed to mcp): {class_hello}"
    );
    let cli_tp = TempProfile::new();
    let cli_hello = {
        let r = Running::plain(cli_tp.dirs());
        r.raw().hello()
    };
    assert!(
        cli_hello["result"]["requester"].is_object(),
        "fixture: a cli has a requester: {cli_hello}"
    );
    assert_eq!(
        class_hello["result"]["requester"], cli_hello["result"]["requester"],
        "the class must receive exactly the requester a cli of the same process receives"
    );
}

/// I-01: the class never has a method a `cli` of the same process lacks.
#[test]
fn desktop_methods_never_exceed_cli_methods() {
    let class_tp = TempProfile::new();
    let class_methods = {
        let r = Running::desktop(class_tp.dirs());
        let hello = r.raw().hello();
        assert_eq!(
            hello["result"]["profile"], "mcp",
            "fixture: the class must be in force (narrowed to mcp): {hello}"
        );
        method_names(&hello)
    };
    assert!(
        class_methods.contains(methods::CONNECTION_DESKTOP),
        "fixture: the class must be offered connection.desktop: {class_methods:?}"
    );
    let cli_tp = TempProfile::new();
    let cli_methods = {
        let r = Running::plain(cli_tp.dirs());
        method_names(&r.raw().hello())
    };
    let extra: Vec<_> = class_methods
        .iter()
        .filter(|m| m.as_str() != methods::CONNECTION_DESKTOP && !cli_methods.contains(*m))
        .collect();
    assert!(
        extra.is_empty(),
        "the class must not offer more than a cli does, besides connection.desktop: {extra:?}"
    );
}

#[test]
fn profile_is_fixed_at_hello() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let mut raw = r.raw();
    let first = raw.hello();
    assert_eq!(
        first["result"]["profile"], "mcp",
        "fixture: the class must be narrowed at hello: {first}"
    );
    let before = raw.call(1, methods::CONNECTION_DESKTOP, json!({}));
    assert!(
        before["result"].is_object(),
        "fixture: connection.desktop must answer before the second hello: {before}"
    );

    let again = raw.hello();
    assert_eq!(again["error"]["code"], code::INVALID_REQUEST, "{again}");
    assert_eq!(
        again["error"]["message"], "already greeted",
        "a second hello must be rejected: {again}"
    );
    let after = raw.call(2, methods::CONNECTION_DESKTOP, json!({}));
    assert_eq!(
        after["result"], before["result"],
        "the reason must not change after a second hello"
    );
    let outside = methods::METHODS
        .iter()
        .find(|m| {
            !m.mcp && !m.reserved && m.exists_in(PROTOCOL_VERSION) && m.name != methods::HELLO
        })
        .expect("the contract has a method outside the mcp allowlist");
    let refused = raw.call(3, outside.name, json!({}));
    assert_eq!(
        refused["error"]["code"],
        code::METHOD_NOT_FOUND,
        "{} is outside the mcp allowlist and must stay not found: {refused}",
        outside.name
    );
}

#[test]
fn declared_mcp_from_the_app_keeps_mcp() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let mut raw = r.raw();
    let hello = raw.hello_as(PROTOCOL_VERSION, "mcp");
    assert_eq!(hello["result"]["profile"], "mcp", "{hello}");
    let answer = raw.call(1, methods::CONNECTION_DESKTOP, json!({}));
    assert_eq!(
        answer["result"]["reason"], "declared-mcp",
        "a class connection that declares mcp must report declared-mcp: {answer}"
    );
}

#[test]
fn desktop_class_is_logged_without_paths_or_client_text() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let mut raw = r.raw();
    raw.hello();
    let start = Instant::now();
    let lines = loop {
        let lines: Vec<String> = r
            .log()
            .lines()
            .filter(|l| l.contains("desktop_client"))
            .map(str::to_owned)
            .collect();
        if !lines.is_empty() {
            break lines;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the daemon never logged desktop_client; log: {}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let exe = std::env::current_exe().unwrap();
    let profile_root = tp.root.path().to_string_lossy().into_owned();
    for line in &lines {
        for field in [
            "class=desktop",
            "identity=none",
            "profile=mcp",
            "reason=daemon-descendant",
        ] {
            assert!(
                line.contains(field),
                "desktop_client must carry {field}: {line}"
            );
        }
        assert!(
            !line.contains(CLIENT_VERSION),
            "the log must not carry the client's version: {line}"
        );
        assert!(
            !line.contains(exe.to_string_lossy().as_ref()),
            "the log must not carry the executable's path: {line}"
        );
        assert!(
            !line.contains(&profile_root),
            "the log must not carry a path of the profile: {line}"
        );
        assert!(
            !line.contains("pid"),
            "the log must not carry a pid: {line}"
        );
    }
}

#[test]
fn other_clients_keep_their_shapes() {
    let tp = TempProfile::new();
    let r = Running::plain(tp.dirs());
    let desktop_method = |raw: &mut Raw| raw.call(9, methods::CONNECTION_DESKTOP, json!({}));

    let mut cli = r.raw();
    let hello = cli.hello_as(PROTOCOL_VERSION, "cli");
    assert_eq!(hello["result"]["profile"], "full", "{hello}");
    assert_eq!(keys(&hello), today_hello_keys(true), "cli HelloResult keys");
    assert!(hello["result"]["requester"].is_object(), "{hello}");
    assert!(!method_names(&hello).contains(methods::CONNECTION_DESKTOP));
    assert_eq!(
        desktop_method(&mut cli)["error"]["code"],
        code::METHOD_NOT_FOUND,
        "a cli must get -32601 for connection.desktop"
    );

    let mut other = r.raw();
    let hello = other.hello_as(PROTOCOL_VERSION, "other");
    assert_eq!(hello["result"]["profile"], "full", "{hello}");
    assert_eq!(
        keys(&hello),
        today_hello_keys(false),
        "other HelloResult keys"
    );
    assert!(!method_names(&hello).contains(methods::CONNECTION_DESKTOP));
    assert_eq!(
        desktop_method(&mut other)["error"]["code"],
        code::METHOD_NOT_FOUND,
        "an other client must get -32601 for connection.desktop"
    );

    let mut mcp = r.raw();
    let hello = mcp.hello_as(PROTOCOL_VERSION, "mcp");
    assert_eq!(hello["result"]["profile"], "mcp", "{hello}");
    assert_eq!(
        keys(&hello),
        today_hello_keys(false),
        "mcp HelloResult keys"
    );
    assert!(!method_names(&hello).contains(methods::CONNECTION_DESKTOP));
    assert_eq!(
        desktop_method(&mut mcp)["error"]["code"],
        code::METHOD_NOT_FOUND,
        "an mcp client must get -32601 for connection.desktop"
    );

    assert!(
        !r.log().contains("desktop_client"),
        "a daemon without the class must log no desktop_client: {}",
        r.log()
    );
}

#[test]
fn desktop_method_needs_protocol_9() {
    let tp = TempProfile::new();
    let r = Running::desktop(tp.dirs());
    let mut raw = r.raw();
    let hello = raw.hello_as(8, "cli");
    assert_eq!(
        hello["result"]["profile"], "mcp",
        "the class on protocol 8 must be narrowed the same: {hello}"
    );
    assert!(
        !method_names(&hello).contains(methods::CONNECTION_DESKTOP),
        "protocol 8 must not offer connection.desktop: {hello}"
    );
    assert_eq!(
        raw.call(1, methods::CONNECTION_DESKTOP, json!({}))["error"]["code"],
        code::METHOD_NOT_FOUND,
        "protocol 8 must get -32601 for connection.desktop"
    );
}
