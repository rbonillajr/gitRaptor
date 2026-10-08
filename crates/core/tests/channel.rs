//! TS-GRP-004: the channel with a real daemon (in-process) and the real
//! client library, over a temporary profile (NFR-01). The process-level
//! scenarios (on-demand start, simulated agent, pty, permissions, network)
//! are in `apps/cli/tests/channel_process.rs`.
#![cfg(target_os = "macos")]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::event::{ENGINE_STATE, RESERVED_AUDIT};
use gitraptor_api::messages::{
    AuditEntry, AuditListResult, AuditOutcome, ClientKind, McpSnapshot, RefusalReason, RefusedData,
    Snapshot, SubscribeResult,
};
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, Timings, clock, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig, EventBus};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{DaemonRun, ProfileDirs};
use gitraptor_git::resolve::ResolveConfig;
use serde_json::json;

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

/// This test binary's executable name: as the simulated agent, every
/// reserved command it sends is refused by ancestry, deterministically,
/// whoever runs the tests.
fn self_as_agent() -> AgentMatcher {
    let exe = std::env::current_exe().unwrap();
    AgentMatcher::only(vec![
        exe.file_name().unwrap().to_string_lossy().into_owned(),
    ])
}

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    bus: std::sync::Arc<EventBus>,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig) -> Self {
        let config = DaemonConfig {
            dirs: dirs.clone(),
            env: DaemonEnv::from_vars(Vec::new()),
            git: no_git(),
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel,
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            dirs,
            handle,
            bus,
            join: Some(join),
        }
    }

    fn client(&self) -> Client {
        connect(&self.dirs, ClientKind::Cli)
    }

    fn stop(mut self) -> StopReport {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap()
    }

    fn join(mut self) -> StopReport {
        self.join.take().unwrap().join().unwrap()
    }

    fn socket(&self) -> PathBuf {
        gitraptor_core::client::socket_path(&self.dirs).unwrap()
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

fn connect(dirs: &ProfileDirs, kind: ClientKind) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(dirs, kind, PROTOCOL_VERSION) {
            Ok(client) => return client,
            Err(err) if start.elapsed() < Duration::from_secs(5) => {
                let _ = err;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("connect: {err}"),
        }
    }
}

fn running() -> (TempProfile, Running) {
    let tp = TempProfile::new();
    let r = Running::start(tp.dirs(), ChannelConfig::default());
    (tp, r)
}

fn rpc_code(err: ClientError) -> i64 {
    match err {
        ClientError::Rpc(e) => e.code,
        other => panic!("expected an RPC error, got {other}"),
    }
}

fn refusal(err: ClientError) -> RefusalReason {
    match err {
        ClientError::Rpc(e) => {
            assert_eq!(e.code, code::RESERVED_REFUSED, "{e}");
            serde_json::from_value::<RefusedData>(e.data.unwrap())
                .unwrap()
                .reason
        }
        other => panic!("expected a refusal, got {other}"),
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
        // A connection past the limit may already be closed by the daemon,
        // and macOS refuses options on it (EINVAL); its reads end at once.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let reader = BufReader::new(stream.try_clone().unwrap());
        Self { stream, reader }
    }

    fn send(&mut self, line: &str) {
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    /// Next message, or `None` at end of stream / error.
    fn recv(&mut self) -> Option<serde_json::Value> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).unwrap()),
        }
    }

    fn hello(&mut self) {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"hello","params":{{"protocol":{PROTOCOL_VERSION},"client":"cli","client_version":"t"}}}}"#
        ));
        let resp = self.recv().unwrap();
        assert!(resp.get("result").is_some(), "{resp}");
    }
}

// ---------------------------------------------------------------- SEC-01

/// Socket 0600 in a 0700 folder; the handshake presents the profile's
/// instance id.
#[test]
fn socket_is_private_and_handshake_presents_the_instance() {
    use std::os::unix::fs::PermissionsExt;
    let (tp, r) = running();
    let client = r.client();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&r.socket()), 0o600);
    assert_eq!(mode(r.dirs.runtime.as_deref().unwrap()), 0o700);
    assert_eq!(client.hello().protocol, PROTOCOL_VERSION);
    assert_eq!(client.hello().instance_id, tp.open().instance_id());
    assert!(!client.hello().methods.contains(&"hello".to_owned()));
}

/// A client of another user is closed before it can send anything. The
/// test cannot switch users without root, so the daemon is told to expect
/// another uid: the real client is then "the other user".
#[test]
fn a_client_of_another_user_is_rejected() {
    let tp = TempProfile::new();
    let me = rustix::process::geteuid().as_raw();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            expected_uid: Some(me + 1),
            ..ChannelConfig::default()
        },
    );
    std::thread::sleep(Duration::from_millis(100));
    match Client::connect(&r.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
        Ok(_) => panic!("another user's client was accepted"),
        Err(ClientError::Io(_) | ClientError::NotRunning) => {}
        Err(other) => panic!("unexpected: {other}"),
    }
    let start = Instant::now();
    while !r.log().contains("client_rejected reason=uid") {
        assert!(start.elapsed() < Duration::from_secs(5), "{}", r.log());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_precreated_open_runtime_folder_stops_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let runtime = dirs.runtime.clone().unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = DaemonConfig {
        dirs,
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        tm_capture: Default::default(),
    };
    assert!(Daemon::start(config).is_err());
    assert!(!runtime.join("raptor.sock").exists());
    // Not "fixed" with chmod.
    let mode = std::fs::metadata(&runtime).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

/// macOS `sun_path` holds 104 bytes; the profile can live deeper.
#[test]
fn a_profile_path_beyond_the_socket_limit_still_connects() {
    let tmp = tempfile::tempdir().unwrap();
    let deep = tmp.path().join("d".repeat(60)).join("e".repeat(60));
    let dirs = ProfileDirs::under_root(&deep);
    assert!(
        gitraptor_core::client::socket_path(&dirs)
            .unwrap()
            .as_os_str()
            .len()
            > 104
    );
    let r = Running::start(dirs.clone(), ChannelConfig::default());
    let mut client = connect(&dirs, ClientKind::Cli);
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
    drop(r);
}

// ---------------------------------------------------------------- SEC-02

/// Oversized, unknown fields, batches, too deep, a UNC path or a non-hello
/// first message are refused and the daemon keeps serving.
#[test]
fn malformed_input_is_refused_without_stopping_the_daemon() {
    let (_tp, r) = running();

    let mut raw = Raw::open(&r.socket());
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
    assert_eq!(
        raw.recv().unwrap()["error"]["code"],
        code::HANDSHAKE_REQUIRED
    );
    assert!(raw.recv().is_none(), "closed after a missing handshake");

    let mut raw = Raw::open(&r.socket());
    raw.hello();
    raw.send(&format!(r#"{{"x":"{}"}}"#, "a".repeat(1024 * 1024 + 10)));
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::INVALID_REQUEST);
    raw.send(r#"{"jsonrpc":"2.0","id":2,"method":"ping","extra":1}"#);
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::INVALID_REQUEST);
    raw.send(r#"[{"jsonrpc":"2.0","id":3,"method":"ping"}]"#);
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::INVALID_REQUEST);
    raw.send(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"ping","params":{}{}}}"#,
        "[".repeat(64),
        "]".repeat(64)
    ));
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::INVALID_REQUEST);
    raw.send(r#"{"jsonrpc":"2.0","id":5,"method":"events.subscribe","params":{"from_seq":1,"tty":true}}"#);
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::INVALID_PARAMS);
    raw.send(r#"{"jsonrpc":"2.0","id":6,"method":"repo.add","params":{"path":"\\\\attacker\\share\\repo"}}"#);
    let resp = raw.recv().unwrap();
    assert_eq!(resp["error"]["code"], code::INVALID_PARAMS, "{resp}");
    assert_eq!(resp["error"]["message"], "UNC or device path");
    raw.send(r#"{"jsonrpc":"2.0","id":7,"method":"no.such"}"#);
    assert_eq!(raw.recv().unwrap()["error"]["code"], code::METHOD_NOT_FOUND);
    raw.send(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#);
    assert_eq!(raw.recv().unwrap()["result"], "pong");

    // A silent client is closed once the handshake times out.
    let mut silent = Raw::open(&r.socket());
    let start = Instant::now();
    assert!(silent.recv().is_none());
    assert!(start.elapsed() < Duration::from_secs(4));

    let mut client = r.client();
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

// ---------------------------------------------------------- stream, DEP-CKP-6

fn change_timings(batch: u64) -> Timings {
    let now = clock::monotonic_ns();
    Timings {
        batch_id: batch,
        t_recv: now,
        t_flush: now,
        t_computed: now,
        t_persisted: now,
        t_published: 0,
    }
}

/// Snapshot at N, subscribe from N + 1: events arrive in sequence order,
/// none lost or repeated, and each change event carries its timings on the
/// clock the client also reads (ADR-GRP-011 § 3).
#[test]
fn stream_is_ordered_gapless_and_timed() {
    let (_tp, r) = running();
    let mut client = r.client();
    let snap: Snapshot = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    assert!(snap.seq >= 1, "engine.state is published at start");
    // Published between the snapshot and the subscription: must arrive.
    r.bus.publish(
        "git.event",
        json!({"n": 0}),
        Some(change_timings(1)),
        |_| {},
    );
    let sub: SubscribeResult = client
        .call(
            methods::EVENTS_SUBSCRIBE,
            json!({"from_seq": snap.seq + 1, "run_id": snap.run_id}),
        )
        .unwrap();
    assert_eq!(sub.from_seq, snap.seq + 1);
    for n in 1..=50 {
        r.bus.publish(
            "git.event",
            json!({"n": n}),
            Some(change_timings(2)),
            |_| {},
        );
    }
    let mut seqs = Vec::new();
    while seqs.len() < 51 {
        let note = client
            .next_notification(Duration::from_secs(5))
            .unwrap()
            .expect("event");
        let received = clock::monotonic_ns();
        let event = &note.params["event"];
        seqs.push(event["seq"].as_u64().unwrap());
        let t = &event["timings"];
        let published = t["t_published"].as_u64().unwrap();
        assert!(published >= t["t_persisted"].as_u64().unwrap());
        assert!(received >= published, "client clock behind the daemon's");
        assert!(received - published < 1_000_000_000);
    }
    let expected: Vec<u64> = (snap.seq + 1..=snap.seq + 51).collect();
    assert_eq!(seqs, expected);
}

#[test]
fn a_restarted_daemon_asks_for_a_resync() {
    let (_tp, r) = running();
    let mut client = r.client();
    let err = client
        .call::<_, SubscribeResult>(
            methods::EVENTS_SUBSCRIBE,
            json!({"from_seq": 1, "run_id": "another-run"}),
        )
        .unwrap_err();
    assert_eq!(rpc_code(err), code::RESYNC_REQUIRED);
    let note = client
        .next_notification(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(note.method, methods::NOTIFY_RESYNC);
    assert_eq!(note.params["reason"], "daemon-restarted");
}

// ---------------------------------------------------------------- SEC-08

/// A client that never reads and 100 simultaneous connections do not starve
/// the others: the good client gets every event and the slow one a resync.
/// The IPC budget of ADR-GRP-011 (≤ 25 ms p95) under this load is gated in
/// release by the `channel_flood` bench (INF-GRP-002), not here.
#[test]
fn slow_client_and_connection_flood_do_not_starve_the_others() {
    let (_tp, r) = running();
    let mut good = r.client();
    let _: SubscribeResult = good.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();

    // Subscribes and never reads again.
    let mut slow = Raw::open(&r.socket());
    slow.hello();
    slow.send(r#"{"jsonrpc":"2.0","id":1,"method":"events.subscribe"}"#);
    let deadline = Instant::now() + Duration::from_secs(5);
    while r.bus.subscriber_count() < 2 {
        assert!(
            Instant::now() < deadline,
            "the slow client never subscribed"
        );
        std::thread::yield_now();
    }

    // 100 more connections; past the limit they are told so and closed.
    let flood: Vec<Raw> = (0..100).map(|_| Raw::open(&r.socket())).collect();
    std::thread::sleep(Duration::from_millis(100));

    let mut received = 0;
    let mut slow_messages = None;
    let payload = "x".repeat(2048);
    for i in 0..3000 {
        r.bus.publish(
            "git.event",
            json!({"n": i, "pad": payload}),
            Some(change_timings(3)),
            |_| {},
        );
        // The bus just dropped the slow one: it overflowed its outbox. Its
        // writer has been blocked on a full socket since the first events
        // and gives up after `write_timeout`, so the slow client reads now,
        // not after the whole run, or the resync never reaches it.
        if slow_messages.is_none() && r.bus.subscriber_count() < 2 {
            slow_messages = Some(std::iter::from_fn(|| slow.recv()).collect::<Vec<_>>());
        }
        if i % 10 == 0 {
            // Drain the good client as a real one would.
            while let Some(note) = good.next_notification(Duration::from_millis(1)).unwrap() {
                if note.params["event"]["timings"]["t_published"].is_u64() {
                    received += 1;
                }
            }
        }
    }
    let start = Instant::now();
    while received < 3000 && start.elapsed() < Duration::from_secs(10) {
        if let Some(note) = good.next_notification(Duration::from_millis(50)).unwrap()
            && note.params["event"]["timings"]["t_published"].is_u64()
        {
            received += 1;
        }
    }
    assert_eq!(received, 3000, "the good client lost events");

    // The slow one was told to resync and dropped (it never read: its
    // socket buffer may still hold events before the resync).
    let slow_messages = slow_messages.expect("the slow client never overflowed");
    let last = slow_messages.last().expect("the slow client got nothing");
    assert_eq!(last["method"], methods::NOTIFY_RESYNC, "{last}");
    assert_eq!(last["params"]["reason"], "slow-consumer");
    let limited = flood
        .into_iter()
        .filter_map(|mut raw| {
            // A socket the daemon already closed refuses new options.
            let _ = raw
                .stream
                .set_read_timeout(Some(Duration::from_millis(200)));
            raw.recv()
        })
        .filter(|m| m["error"]["code"] == code::LIMIT_REACHED)
        .count();
    assert!(limited > 0, "the connection limit never applied");
    let pong: String = good.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

#[test]
fn requests_beyond_the_rate_limit_are_refused_not_disconnected() {
    let (_tp, r) = running();
    let mut raw = Raw::open(&r.socket());
    raw.hello();
    let mut limited = 0;
    for i in 0..400 {
        raw.send(&format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"ping"}}"#,
            i + 1
        ));
    }
    for _ in 0..400 {
        let msg = raw.recv().unwrap();
        if msg["error"]["code"] == code::RATE_LIMITED {
            limited += 1;
        }
    }
    assert!(limited > 0);
    std::thread::sleep(Duration::from_millis(100));
    raw.send(r#"{"jsonrpc":"2.0","id":999,"method":"ping"}"#);
    assert_eq!(raw.recv().unwrap()["result"], "pong");
}

// ------------------------------------------------------ SEC-03, SEC-13, SEC-14

fn audit(client: &mut Client) -> Vec<AuditEntry> {
    let list: AuditListResult = client.call(methods::AUDIT_LIST, json!({})).unwrap();
    list.entries
}

/// A direct JSON-RPC client that descends from an agent is refused for
/// every reserved command, and each attempt is audited once.
#[test]
fn reserved_commands_from_an_agent_descendant_are_refused_and_audited() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: self_as_agent(),
            ..ChannelConfig::default()
        },
    );
    let mut client = r.client();
    let mut watcher = r.client();
    let _: SubscribeResult = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();

    let attempts = [
        (methods::DAEMON_STOP, json!({})),
        (methods::REPO_ADD, json!({"path": "/tmp/some/repo"})),
        (methods::REPO_RETIRE, json!({"repo_id": "abc-123"})),
        (methods::ATTRIBUTION_CORRECT, json!({})),
        (
            methods::REGISTRATION_WITHDRAW,
            json!({"worktree": "/tmp/some/repo", "agent": {"kind": "other", "name": "Codex"}}),
        ),
    ];
    for (method, params) in &attempts {
        let err = client
            .call::<_, serde_json::Value>(method, params)
            .unwrap_err();
        assert_eq!(refusal(err), RefusalReason::AgentAncestry, "{method}");
    }
    let entries = audit(&mut client);
    assert_eq!(entries.len(), attempts.len());
    for (entry, (method, _)) in entries.iter().zip(&attempts) {
        assert_eq!(entry.operation, *method);
        assert_eq!(entry.outcome, AuditOutcome::Rejected);
        assert_eq!(entry.reason, Some(RefusalReason::AgentAncestry));
        assert!(entry.client.agent_ancestor);
        assert_eq!(entry.client.pid, std::process::id());
    }
    assert_eq!(entries[2].repo_id.as_deref(), Some("abc-123"));
    // Every attempt is announced on the stream too.
    let mut announced = 0;
    while let Some(n) = watcher
        .next_notification(Duration::from_millis(200))
        .unwrap()
    {
        if n.params["event"]["kind"] == RESERVED_AUDIT {
            announced += 1;
        }
    }
    assert_eq!(announced, attempts.len());
    // The daemon is still running.
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
    assert!(
        r.log()
            .contains("reserved_command op=daemon.stop outcome=rejected reason=agent-ancestry")
    );
}

/// The audit cannot be rewritten: not through the channel (no method) and
/// not even through SQL.
#[test]
fn the_audit_is_append_only() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: self_as_agent(),
            ..ChannelConfig::default()
        },
    );
    let mut client = r.client();
    let _ = client.stop_daemon();
    assert_eq!(audit(&mut client).len(), 1);
    drop(client);
    r.stop();
    let conn = rusqlite::Connection::open(tp.dirs().data.join("index.sqlite")).unwrap();
    assert!(
        conn.execute("UPDATE reserved_audit SET outcome = 'accepted'", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM reserved_audit", []).is_err());
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM reserved_audit", [], |row| row.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

/// `raptor-mcp` gets the MCP allowlist: reserved commands and the audit do
/// not exist for it, and the snapshot carries only allowlisted fields.
#[test]
fn mcp_connections_get_the_allowlist_only() {
    let (_tp, r) = running();
    let mut mcp = connect(&r.dirs, ClientKind::Mcp);
    for m in &mcp.hello().methods {
        let spec = methods::spec(m).unwrap();
        assert!(spec.mcp && !spec.reserved, "{m} offered to MCP");
    }
    let err = mcp.stop_daemon().unwrap_err();
    assert_eq!(refusal(err), RefusalReason::NotAvailableToMcp);
    let err = mcp
        .call::<_, AuditListResult>(methods::AUDIT_LIST, json!({}))
        .unwrap_err();
    assert_eq!(rpc_code(err), code::METHOD_NOT_FOUND);
    let snap: serde_json::Value = mcp.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    let mut keys: Vec<_> = snap.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["caller_repo", "engine_state", "run_id", "seq"]);
    let _: McpSnapshot = serde_json::from_value(snap).unwrap();
    // The refusal is audited.
    let mut cli = r.client();
    let entries = audit(&mut cli);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reason, Some(RefusalReason::NotAvailableToMcp));
}

// ---------------------------------------------------------------- versions

/// An older daemon steps down for the newer *installed* binary: the file at
/// the daemon's launch path changed and the caller is that file. The stop
/// is recorded as attributed, with the client.
#[test]
fn a_newer_installed_binary_replaces_an_older_daemon() {
    let tp = TempProfile::new();
    let bin = tempfile::tempdir().unwrap();
    let launch = bin.path().join("raptor");
    std::os::unix::fs::symlink("/usr/bin/true", &launch).unwrap();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            launch_exe: Some(launch.clone()),
            agents: self_as_agent(),
            protocol: PROTOCOL_VERSION,
            ..ChannelConfig::default()
        },
    );
    drop(r.client());
    // "Upgrade": the launch path now resolves to this test binary.
    std::fs::remove_file(&launch).unwrap();
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &launch).unwrap();

    match Client::connect(&r.dirs, ClientKind::Cli, PROTOCOL_VERSION + 1) {
        Err(ClientError::Incompatible(data)) => assert_eq!(data.daemon_protocol, PROTOCOL_VERSION),
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("versions should not match"),
    }
    let mut raw = Raw::open(&r.socket());
    raw.send(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"hello","params":{{"protocol":{},"client":"cli","client_version":"t"}}}}"#,
        PROTOCOL_VERSION + 1
    ));
    assert_eq!(
        raw.recv().unwrap()["error"]["code"],
        code::INCOMPATIBLE_PROTOCOL
    );
    raw.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"daemon.replace","params":{{"protocol":{}}}}}"#,
        PROTOCOL_VERSION + 1
    ));
    assert_eq!(raw.recv().unwrap()["result"]["stopping"], true);
    let report = r.join();
    assert!(matches!(report.cause, StopCause::Replace { .. }));
    let profile = tp.open();
    match profile.daemon_run().unwrap() {
        DaemonRun::Stopped {
            cause,
            requested_by,
            ..
        } => {
            assert_eq!(cause, format!("replace:{PROTOCOL_VERSION}"));
            assert!(requested_by.unwrap().starts_with("client:"));
        }
        other => panic!("{other:?}"),
    }
    let rows = profile.audit(0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.operation, "daemon.replace");
    assert_eq!(rows[0].1.outcome, "accepted");
}

/// The same request from any other executable is a reserved stop, refused
/// for an agent descendant (SEC-13).
#[test]
fn replace_from_another_executable_is_a_reserved_stop() {
    let tp = TempProfile::new();
    let bin = tempfile::tempdir().unwrap();
    let launch = bin.path().join("raptor");
    std::os::unix::fs::symlink("/usr/bin/true", &launch).unwrap();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            launch_exe: Some(launch),
            agents: self_as_agent(),
            ..ChannelConfig::default()
        },
    );
    let mut raw = Raw::open(&r.socket());
    raw.hello();
    raw.send(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"daemon.replace","params":{{"protocol":{}}}}}"#,
        PROTOCOL_VERSION + 1
    ));
    let resp = raw.recv().unwrap();
    assert_eq!(resp["error"]["code"], code::RESERVED_REFUSED, "{resp}");
    assert_eq!(resp["error"]["data"]["reason"], "agent-ancestry");
    let mut client = r.client();
    let entries = audit(&mut client);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].operation, "daemon.replace");
    assert_eq!(entries[0].outcome, AuditOutcome::Rejected);
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

/// A client older than the daemon's compatibility window is told to
/// update (DS-TS-GRP-004 E-D1).
#[test]
fn a_newer_daemon_tells_an_old_client_to_update() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            protocol: PROTOCOL_VERSION + 1,
            min_protocol: PROTOCOL_VERSION + 1,
            ..ChannelConfig::default()
        },
    );
    std::thread::sleep(Duration::from_millis(50));
    match Client::connect(&r.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
        Err(ClientError::ClientTooOld(data)) => {
            assert_eq!(data.daemon_protocol, PROTOCOL_VERSION + 1)
        }
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("versions should not match"),
    }
}

// ---------------------------------------------------------------- SEC-12

/// A repo path with OSC escapes travels marked as untrusted and comes out
/// clean once sanitized.
#[test]
fn repo_text_is_marked_untrusted_in_the_contract() {
    let tp = TempProfile::new();
    let parent = tp.root.path().join("repos");
    std::fs::create_dir_all(&parent).unwrap();
    let repo = init_repo(&parent, "evil\u{1b}]52;c;cHduZWQ=\u{7}", true);
    tp.open().add_repo(&common_dir(&repo), None, 1).unwrap();
    let r = Running::start(tp.dirs(), ChannelConfig::default());
    let mut client = r.client();
    let raw: serde_json::Value = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    let path = &raw["repos"][0]["path"];
    assert!(path["untrusted"].as_str().unwrap().contains('\u{1b}'));
    let snap: Snapshot = serde_json::from_value(raw).unwrap();
    let clean = snap.repos[0].path.sanitized();
    assert!(!clean.contains('\u{1b}') && !clean.contains('\u{7}'));
    assert!(clean.contains("evil"));
}

#[test]
fn engine_state_is_the_first_event_of_a_run() {
    let (_tp, r) = running();
    let mut client = r.client();
    let _: SubscribeResult = client
        .call(methods::EVENTS_SUBSCRIBE, json!({"from_seq": 1}))
        .unwrap();
    let note = client
        .next_notification(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(note.params["event"]["kind"], ENGINE_STATE);
    assert_eq!(note.params["event"]["seq"], 1);
    assert!(note.params["event"].get("timings").is_none());
}

// ---------------------------------------------------------------- DEP-MCP-3

/// A process the daemon spawned (here: the in-process daemon is this test
/// process, and `sh`/`nc` are its children, as a hook run by the future
/// operation executor would be) cannot use a reserved command.
#[test]
fn a_child_of_the_daemon_cannot_use_reserved_commands() {
    let tp = TempProfile::new();
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: AgentMatcher::only(vec!["no-such-agent".into()]),
            ..ChannelConfig::default()
        },
    );
    drop(r.client());
    let script = format!(
        "(printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\",\"params\":{{\"protocol\":{PROTOCOL_VERSION},\"client\":\"cli\",\"client_version\":\"x\"}}}}' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"daemon.stop\"}}'; sleep 1) | /usr/bin/nc -U {}",
        r.socket().display()
    );
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("daemon-descendant"), "{text}");
    let mut client = r.client();
    let entries = audit(&mut client);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reason, Some(RefusalReason::DaemonDescendant));
    assert!(entries[0].client.daemon_descendant);
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

/// Another process of the user removes the socket and binds its own: on its
/// next heartbeat the daemon takes the channel back.
#[test]
fn a_replaced_socket_is_taken_back() {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let config = DaemonConfig {
        dirs: dirs.clone(),
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_millis(100),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let instance = connect(&dirs, ClientKind::Cli).hello().instance_id.clone();
    let socket = gitraptor_core::client::socket_path(&dirs).unwrap();
    std::fs::remove_file(&socket).unwrap();
    let impostor = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let log_path = dirs.state.join("daemon.log");
    let start = Instant::now();
    // Detected, and served again (a second `channel_serving`).
    while {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        !(log.contains("channel_socket_replaced") && log.matches("channel_serving").count() >= 2)
    } {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "not detected: {}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // The real daemon answers again on the socket path.
    assert_eq!(
        connect(&dirs, ClientKind::Cli).hello().instance_id,
        instance
    );
    drop(impostor);
    handle.request(StopCause::Signal("TERM"));
    join.join().unwrap();
}
