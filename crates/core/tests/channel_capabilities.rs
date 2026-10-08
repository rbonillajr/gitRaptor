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
use gitraptor_api::messages::ClientKind;
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
        Self::start_with(dirs, channel, false)
    }

    /// With `git`, the system Git of the environment: the engine observes its repos.
    fn start_with(dirs: ProfileDirs, channel: ChannelConfig, git: bool) -> Self {
        let protocol = channel.protocol;
        let env = DaemonEnv::from_vars(if git {
            std::env::vars_os().collect()
        } else {
            Vec::new()
        });
        let config = DaemonConfig {
            dirs: dirs.clone(),
            git: if git {
                env.git_resolve_config(None)
            } else {
                ResolveConfig {
                    configured_path: None,
                    path_env: None,
                    known_locations: Vec::new(),
                    shim_paths: Vec::new(),
                    toolchain_gits: Vec::new(),
                }
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
    // Raw connections: the library client already accepts what it knows on connecting.
    let mut client = r.raw();
    client.hello(PROTOCOL_VERSION);
    let answer = client.call(
        1,
        methods::CONNECTION_ACCEPT,
        json!({"capabilities": ["events.git-reset", "scope.activity", "nobody.knows-this"]}),
    );
    let granted: AcceptResult = serde_json::from_value(answer["result"].clone()).unwrap();
    for name in ["events.git-reset", "connection.requester", "scope.activity"] {
        assert!(granted.capabilities.contains(&name.to_owned()), "{name}");
    }
    assert!(!granted.capabilities.iter().any(|c| c.starts_with("nobody")));
    let again = client.call(2, methods::CONNECTION_ACCEPT, json!({"capabilities": []}));
    assert_eq!(again["error"]["code"], code::INVALID_REQUEST, "{again}");

    let mut subscribed = r.raw();
    subscribed.hello(PROTOCOL_VERSION);
    subscribed.call(1, methods::EVENTS_SUBSCRIBE, json!({"from_seq": 1}));
    let late = subscribed.call(2, methods::CONNECTION_ACCEPT, json!({"capabilities": []}));
    assert_eq!(late["error"]["code"], code::INVALID_REQUEST, "{late}");

    let mut old = r.connect(8);
    let missing = old
        .call::<_, AcceptResult>(methods::CONNECTION_ACCEPT, json!({"capabilities": []}))
        .unwrap_err();
    assert_eq!(rpc_code(missing), code::METHOD_NOT_FOUND);

    let mut flood = r.raw();
    flood.hello(PROTOCOL_VERSION);
    let names: Vec<String> = (0..=gitraptor_api::capability::MAX_ACCEPTED)
        .map(|i| format!("x.{i}"))
        .collect();
    let refused = flood.call(
        1,
        methods::CONNECTION_ACCEPT,
        json!({ "capabilities": names }),
    );
    assert_eq!(refused["error"]["code"], code::INVALID_PARAMS, "{refused}");
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

/// The scope of one repo.
fn repo_scope(repo_id: &str) -> Value {
    serde_json::to_value(gitraptor_api::scope::Scope::Repo {
        repo_id: repo_id.to_owned(),
    })
    .unwrap()
}

/// The next `worktree.state` of a repo scope, as the connection receives it.
fn next_worktree_state(next: &mut dyn FnMut() -> Option<Value>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(n) = next()
            && n["method"] == methods::NOTIFY_SCOPE_EVENT
            && n["params"]["event"]["kind"] == gitraptor_api::event::WORKTREE_STATE
        {
            return n["params"]["event"]["data"].clone();
        }
    }
    panic!("no worktree.state");
}

/// DEP-CKP-4 (ADR-GRP-013, amendment 2026-10-04): with `scope.activity` a connection sees the
/// repo's last fetch and, once a worktree changes, its last activity; without it, neither
/// field reaches it (its types reject unknown fields), in the snapshot or in `worktree.state`.
#[test]
fn scope_activity_carries_the_last_fetch_and_activity_only_to_who_accepted_it() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let wt = init_repo(tmp.path(), "shop", true);
    std::fs::write(common_dir(&wt).join("FETCH_HEAD"), "").unwrap();
    let (entry, _) = tp.open().add_repo(&common_dir(&wt), None, 1).unwrap();
    let repo_id = entry.repo_id;
    let r = Running::start_with(tp.dirs(), ChannelConfig::default(), true);

    // The library client accepts every capability it knows when it connects.
    let mut with = r.connect(PROTOCOL_VERSION);
    let mut without = r.raw();
    without.hello(PROTOCOL_VERSION);

    let scope = repo_scope(&repo_id);
    let snap: Value = with
        .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
        .unwrap();
    assert!(snap["repo"]["fetched_utc_ms"].is_i64(), "{snap}");
    // Nothing changed yet in this run: no activity is known.
    assert!(
        snap["repo"]["worktrees"][0]
            .get("last_activity_utc_ms")
            .is_none()
    );
    let next = |snap: &Value| json!({"scope": scope, "from_seq": snap["scope_seq"].as_u64().unwrap() + 1, "run_id": snap["run_id"]});
    let _: Value = with.call(methods::SCOPE_SUBSCRIBE, next(&snap)).unwrap();
    let bare = without.call(2, methods::SCOPE_SNAPSHOT, json!({ "scope": scope }));
    assert!(
        bare["result"]["repo"].get("fetched_utc_ms").is_none(),
        "{bare}"
    );
    let sub = without.call(3, methods::SCOPE_SUBSCRIBE, next(&bare["result"]));
    assert!(sub["result"].is_object(), "{sub}");

    std::fs::write(wt.join("new.txt"), "x").unwrap();
    let seen = next_worktree_state(&mut || {
        with.next_notification(Duration::from_millis(200))
            .unwrap()
            .map(|n| serde_json::to_value(n).unwrap())
    });
    assert!(
        seen["worktrees"][0]["last_activity_utc_ms"].is_i64(),
        "{seen}"
    );
    assert!(seen["fetched_utc_ms"].is_i64(), "{seen}");
    let plain = next_worktree_state(&mut || {
        let mut line = String::new();
        without.reader.read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    });
    assert!(
        plain["worktrees"][0].get("last_activity_utc_ms").is_none(),
        "{plain}"
    );
    assert!(plain.get("fetched_utc_ms").is_none(), "{plain}");
}

/// Dogfooding 2026-10-06 (cold start): after a restart the last activity is seeded with the
/// latest Git event each worktree has in the store, with the gap mark when that event is
/// linked to a gap (ADR-GRP-013 § 6); a worktree without events stays absent.
#[test]
fn a_restarted_engine_seeds_the_last_activity_from_the_stored_events() {
    use gitraptor_core::profile::{GapCause, NewEvent, WriteOp};

    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let wt = init_repo(tmp.path(), "shop", true);
    let linked = tmp.path().join("shop-linked");
    let quiet = tmp.path().join("shop-quiet");
    git(
        &wt,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    git(
        &wt,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "quiet",
            quiet.to_str().unwrap(),
        ],
    );
    let (main, linked) = (wt.canonicalize().unwrap(), linked.canonicalize().unwrap());
    let (entry, _) = tp.open().add_repo(&common_dir(&wt), None, 1).unwrap();
    let repo_id = entry.repo_id;

    // What a previous run of the engine left in the store.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let (main_at, linked_at) = (now - 120_000, now - 60_000);
    let stored = |path: &Path, at: i64, gap_id: Option<&str>| {
        WriteOp::AppendEvent(NewEvent {
            observed: ts(at),
            gap_id: gap_id.map(str::to_owned),
            ..match event(path, None, "{}") {
                WriteOp::AppendEvent(e) => e,
                _ => unreachable!(),
            }
        })
    };
    let (mut store, _) = tp.open().open_store(&repo_id).unwrap();
    let mut ops = Vec::new();
    for path in [&main, &linked] {
        ops.push(WriteOp::UpsertWorktree {
            path: path.clone(),
            admin_name: None,
            seen_ms: 1,
        });
    }
    ops.extend([
        stored(&main, main_at - 1_000, None),
        stored(&main, main_at, None),
        WriteOp::OpenGap {
            gap_id: "g1".into(),
            started_ms: linked_at - 1_000,
            cause: GapCause::DaemonDown,
            requested_by: None,
        },
        stored(&linked, linked_at, Some("g1")),
    ]);
    store.write_batch(&ops).unwrap();
    drop(store);

    let r = Running::start_with(tp.dirs(), ChannelConfig::default(), true);
    let mut client = r.connect(PROTOCOL_VERSION);
    let snap: Value = client
        .call(
            methods::SCOPE_SNAPSHOT,
            json!({ "scope": repo_scope(&repo_id) }),
        )
        .unwrap();
    let worktree = |path: &Path| {
        snap["repo"]["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["path"]["untrusted"].as_str().map(Path::new) == Some(path))
            .cloned()
            .unwrap_or_else(|| panic!("{path:?} in {snap}"))
    };
    let main_view = worktree(&main);
    assert_eq!(main_view["last_activity_utc_ms"], json!(main_at), "{snap}");
    assert!(main_view.get("last_activity_in_gap").is_none(), "{snap}");
    let linked_view = worktree(&linked);
    assert_eq!(
        linked_view["last_activity_utc_ms"],
        json!(linked_at),
        "{snap}"
    );
    assert_eq!(linked_view["last_activity_in_gap"], json!(true), "{snap}");
    let quiet_view = worktree(&quiet.canonicalize().unwrap());
    assert!(quiet_view.get("last_activity_utc_ms").is_none(), "{snap}");
    assert!(quiet_view.get("last_activity_in_gap").is_none(), "{snap}");
}
