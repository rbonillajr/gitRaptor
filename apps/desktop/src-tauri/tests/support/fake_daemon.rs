//! A fake daemon for the tests of the engine supervisor (DS-US-DSK-003 T002): a real Unix socket
//! (`raptor.sock`) in a `0700` temporary folder that answers the channel's JSON-RPC lines with the
//! types of `gitraptor_api`. Scriptable (`hello`, snapshots, candidates), it records every
//! `(method, params)` it receives and can push notifications, drop its connections, stop (the
//! socket is deleted) and start again. Temporary folders only (NFR-01).
//!
//! Included with `#[path]` by the test files that need it; not a test target of its own. The
//! arrange of the handshake follows `support/fake_raptor.rs` (`answer`), which `engine_launcher`
//! runs today.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::actor::Actor;
use gitraptor_api::capability::{AcceptParams, AcceptResult};
use gitraptor_api::catalog::Layer;
use gitraptor_api::client::transport;
use gitraptor_api::discovery::{CandidateView, CandidatesResult};
use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    ConnectionProfile, DaemonView, EngineStateView, EngineView, HelloResult, IncompatibleData,
    McpSnapshot, ResyncNotification, ResyncReason, Snapshot,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{ErrorObject, Id, Notification, Request, Response, code};
use gitraptor_api::scope::{
    AutostartView, ConnectionRequester, GlobalSnapshot, RepoSnapshot, Scope,
    ScopeEventNotification, ScopeResyncNotification, ScopeSnapshot, ScopeSnapshotParams,
    ScopeSubscribeParams, ScopeSubscribeResult,
};
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, Link, SocketConnector};
use serde::Serialize;
use serde_json::{Value, json};

/// The four capabilities the app requires, plus the optional `discovery.events`.
pub const ALL_CAPABILITIES: [&str; 5] = [
    "scope.activity",
    "observation.tiers",
    "prediction.pairs",
    "connection.requester",
    "discovery.events",
];

/// What the fake answers. Mutate it with [`FakeDaemon::script`] between connections.
#[derive(Clone)]
pub struct Script {
    pub protocol: u32,
    pub binary_version: String,
    pub profile: ConnectionProfile,
    /// `None`: the handshake carries no requester.
    pub requester: Option<ConnectionRequester>,
    /// `None`: the handshake carries no capability list.
    pub capabilities: Option<Vec<String>>,
    /// Answer `hello` with `INCOMPATIBLE_PROTOCOL` and this data.
    pub incompatible: Option<IncompatibleData>,
    /// The global snapshot (its `run_id` and `scope_seq` are the stream position too).
    pub global: GlobalSnapshot,
    pub repos: HashMap<String, RepoSnapshot>,
    pub candidates: Vec<CandidateView>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            protocol: gitraptor_api::PROTOCOL_VERSION,
            binary_version: "0.1.0".to_owned(),
            profile: ConnectionProfile::Full,
            requester: Some(ConnectionRequester::Resolved {
                actor: Actor::Unattributed,
                layer: Layer::Cockpit,
                confirmable: true,
            }),
            capabilities: Some(ALL_CAPABILITIES.iter().map(|c| (*c).to_owned()).collect()),
            incompatible: None,
            global: GlobalSnapshot {
                run_id: "run-1".to_owned(),
                scope_seq: 5,
                engine: EngineView {
                    state: EngineStateView::Observing,
                    git_version: Some("2.45.1".to_owned()),
                },
                daemon: DaemonView {
                    pid: std::process::id(),
                    protocol: gitraptor_api::PROTOCOL_VERSION,
                    binary_version: "0.1.0".to_owned(),
                    started_wall_ms: 0,
                },
                autostart: AutostartView::Unknown,
                repos: Vec::new(),
            },
            repos: HashMap::new(),
            candidates: Vec::new(),
        }
    }
}

/// A request the fake received.
pub type Call = (String, Value);

struct Shared {
    script: Mutex<Script>,
    calls: Mutex<Vec<Call>>,
    changed: Condvar,
    conns: Mutex<Vec<Arc<Mutex<UnixStream>>>>,
    subscriptions: AtomicU32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Listening {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct FakeDaemon {
    dir: tempfile::TempDir,
    /// The runtime folder to give to `SocketConnector`.
    pub runtime: PathBuf,
    shared: Arc<Shared>,
    listening: Mutex<Option<Listening>>,
}

impl FakeDaemon {
    /// A daemon that is up, with the default script.
    pub fn start() -> Arc<Self> {
        let daemon = Self::stopped();
        daemon.restart();
        daemon
    }

    /// A daemon that is not running yet: the folder exists, the socket does not.
    pub fn stopped() -> Arc<Self> {
        let dir = tempfile::tempdir().expect("fake daemon: temporary folder");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("fake daemon: 0700");
        let runtime = dir.path().to_path_buf();
        Arc::new(Self {
            dir,
            runtime,
            shared: Arc::new(Shared {
                script: Mutex::new(Script::default()),
                calls: Mutex::new(Vec::new()),
                changed: Condvar::new(),
                conns: Mutex::new(Vec::new()),
                subscriptions: AtomicU32::new(0),
            }),
            listening: Mutex::new(None),
        })
    }

    /// Changes what the fake answers from the next request on.
    pub fn script(&self, change: impl FnOnce(&mut Script)) {
        change(&mut lock(&self.shared.script));
    }

    /// Sets the stream position the next global snapshot reports.
    pub fn set_global_position(&self, run_id: &str, scope_seq: u64) {
        self.script(|s| {
            run_id.clone_into(&mut s.global.run_id);
            s.global.scope_seq = scope_seq;
        });
    }

    /// Every `(method, params)` received so far, in order, across connections.
    pub fn calls(&self) -> Vec<Call> {
        lock(&self.shared.calls).clone()
    }

    /// The params of each call of `method`, in order.
    pub fn calls_of(&self, method: &str) -> Vec<Value> {
        self.calls()
            .into_iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p)
            .collect()
    }

    /// Waits until `method` was called at least `count` times; `false` at the deadline.
    pub fn wait_for_calls(&self, method: &str, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut calls = lock(&self.shared.calls);
        loop {
            if calls.iter().filter(|(m, _)| m == method).count() >= count {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            calls = self
                .shared
                .changed
                .wait_timeout(calls, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub fn is_running(&self) -> bool {
        lock(&self.listening).is_some()
    }

    /// Pushes a notification on the latest connection; `false` when there is none.
    pub fn push(&self, notification: &Notification) -> bool {
        let conn = lock(&self.shared.conns).last().cloned();
        conn.is_some_and(|conn| write_line(&conn, notification))
    }

    /// Pushes one `scope.event` of the global scope.
    pub fn push_global_event(&self, scope_seq: u64, kind: &str, data: impl Serialize) -> bool {
        let version = gitraptor_api::event::kind(kind).map_or(1, |k| k.version);
        self.push(&Notification::new(
            methods::NOTIFY_SCOPE_EVENT,
            ScopeEventNotification {
                subscription: 1,
                scope: Scope::Global,
                scope_seq,
                event: Event {
                    seq: scope_seq,
                    kind: kind.to_owned(),
                    version,
                    wall_ms: 0,
                    timings: None,
                    data: serde_json::to_value(data).expect("event data"),
                },
            },
        ))
    }

    /// Pushes `scope.resync` for the global scope.
    pub fn push_scope_resync(&self, reason: ResyncReason) -> bool {
        self.push(&Notification::new(
            methods::NOTIFY_SCOPE_RESYNC,
            ScopeResyncNotification {
                scope: Scope::Global,
                reason,
            },
        ))
    }

    /// Pushes `events.resync`.
    pub fn push_events_resync(&self, reason: ResyncReason) -> bool {
        self.push(&Notification::new(
            methods::NOTIFY_RESYNC,
            ResyncNotification { reason },
        ))
    }

    /// Closes every open connection; the socket keeps listening.
    pub fn drop_connection(&self) {
        for conn in lock(&self.shared.conns).drain(..) {
            drop(lock(&conn).shutdown(std::net::Shutdown::Both));
        }
    }

    /// Closes every connection and the listener, and deletes the socket.
    pub fn stop(&self) {
        if let Some(listening) = lock(&self.listening).take() {
            listening.stop.store(true, Ordering::SeqCst);
            drop(listening.thread.join());
        }
        drop(std::fs::remove_file(transport::socket_path(&self.runtime)));
        self.drop_connection();
    }

    /// Starts again (the script and the record are kept) when it is not running.
    pub fn restart(&self) {
        let mut listening = lock(&self.listening);
        if listening.is_some() {
            return;
        }
        let path = transport::socket_path(&self.runtime);
        drop(std::fs::remove_file(&path));
        let listener = UnixListener::bind(&path).expect("fake daemon: bind");
        listener
            .set_nonblocking(true)
            .expect("fake daemon: nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&self.shared);
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || accept_loop(&listener, &shared, &flag));
        *listening = Some(Listening { stop, thread });
    }

    /// A [`Connector`] to this daemon that records the `start` of every attempt and never starts it.
    pub fn connector(self: &Arc<Self>) -> TestConnector {
        TestConnector::new(self, usize::MAX)
    }

    /// A [`Connector`] to this daemon that starts it (the way `raptor daemon ensure` would) at the
    /// `nth` (1-based) `connect(true)`.
    pub fn launching_on_start_call(self: &Arc<Self>, nth: usize) -> TestConnector {
        TestConnector::new(self, nth)
    }

    pub fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: &UnixListener, shared: &Arc<Shared>, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let Ok(writer) = stream.try_clone() else {
                    continue;
                };
                let writer = Arc::new(Mutex::new(writer));
                lock(&shared.conns).push(Arc::clone(&writer));
                let shared = Arc::clone(shared);
                std::thread::spawn(move || serve(&shared, &writer, stream));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return,
        }
    }
}

fn write_line(conn: &Arc<Mutex<UnixStream>>, message: &impl Serialize) -> bool {
    let Ok(mut line) = serde_json::to_vec(message) else {
        return false;
    };
    line.push(b'\n');
    lock(conn).write_all(&line).is_ok()
}

fn serve(shared: &Shared, writer: &Arc<Mutex<UnixStream>>, stream: UnixStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let Ok(request) = serde_json::from_str::<Request>(&line) else {
            return;
        };
        let params = request.params.clone().unwrap_or(Value::Null);
        lock(&shared.calls).push((request.method.clone(), params.clone()));
        shared.changed.notify_all();
        let response = answer(shared, &request, params);
        if !write_line(writer, &response) {
            return;
        }
    }
}

fn invalid(id: &Id) -> Response {
    Response::err(
        Some(id.clone()),
        ErrorObject::new(code::INVALID_PARAMS, "invalid params"),
    )
}

fn answer(shared: &Shared, request: &Request, params: Value) -> Response {
    let id = request.id.clone();
    let script = lock(&shared.script).clone();
    match request.method.as_str() {
        methods::HELLO => {
            if let Some(data) = script.incompatible {
                return Response::err(
                    Some(id),
                    ErrorObject {
                        code: code::INCOMPATIBLE_PROTOCOL,
                        message: "incompatible protocol".to_owned(),
                        data: Some(json!(data)),
                    },
                );
            }
            Response::ok(
                id,
                HelloResult {
                    protocol: script.protocol,
                    binary_version: script.binary_version,
                    instance_id: "fake-instance".to_owned(),
                    daemon_pid: std::process::id(),
                    profile: script.profile,
                    max_message_bytes: 1_048_576,
                    methods: methods::METHODS.iter().map(|m| m.name.to_owned()).collect(),
                    requester: script.requester,
                    capabilities: script.capabilities,
                },
            )
        }
        methods::CONNECTION_ACCEPT => match serde_json::from_value::<AcceptParams>(params) {
            Ok(_) => Response::ok(
                id,
                AcceptResult {
                    capabilities: script.capabilities.unwrap_or_default(),
                },
            ),
            Err(_) => invalid(&id),
        },
        methods::SCOPE_SNAPSHOT => match serde_json::from_value::<ScopeSnapshotParams>(params) {
            Ok(ScopeSnapshotParams {
                scope: Scope::Global,
            }) => Response::ok(id, ScopeSnapshot::Global(script.global)),
            Ok(ScopeSnapshotParams {
                scope: Scope::Repo { repo_id },
            }) => match script.repos.get(&repo_id) {
                Some(repo) => Response::ok(id, ScopeSnapshot::Repo(repo.clone())),
                None => invalid(&id),
            },
            Err(_) => invalid(&id),
        },
        methods::SCOPE_SUBSCRIBE => match serde_json::from_value::<ScopeSubscribeParams>(params) {
            Ok(p) => {
                let subscription = shared.subscriptions.fetch_add(1, Ordering::SeqCst) + 1;
                Response::ok(
                    id,
                    ScopeSubscribeResult {
                        subscription,
                        scope: p.scope,
                        from_seq: p.from_seq.unwrap_or(script.global.scope_seq + 1),
                    },
                )
            }
            Err(_) => invalid(&id),
        },
        methods::ENGINE_SNAPSHOT => {
            let g = script.global;
            if script.profile == ConnectionProfile::Mcp {
                Response::ok(
                    id,
                    McpSnapshot {
                        run_id: g.run_id,
                        seq: g.scope_seq,
                        engine_state: g.engine.state,
                        caller_repo: None,
                    },
                )
            } else {
                Response::ok(
                    id,
                    Snapshot {
                        run_id: g.run_id,
                        seq: g.scope_seq,
                        engine: g.engine,
                        daemon: g.daemon,
                        repos: Vec::new(),
                    },
                )
            }
        }
        methods::DISCOVERY_CANDIDATES => Response::ok(
            id,
            CandidatesResult {
                candidates: script.candidates,
            },
        ),
        methods::SESSIONS_LIST => {
            Response::ok(id, json!({"detection_available": true, "sessions": []}))
        }
        _ => Response::err(
            Some(id),
            ErrorObject::new(code::METHOD_NOT_FOUND, "method not found"),
        ),
    }
}

/// The app's way to the fake: records the `start` of each attempt and, from the `nth` start, brings
/// the fake up like `raptor daemon ensure` would.
pub struct TestConnector {
    daemon: Arc<FakeDaemon>,
    starts: Arc<Mutex<Vec<bool>>>,
    launch_on: usize,
}

impl TestConnector {
    fn new(daemon: &Arc<FakeDaemon>, launch_on: usize) -> Self {
        Self {
            daemon: Arc::clone(daemon),
            starts: Arc::new(Mutex::new(Vec::new())),
            launch_on,
        }
    }

    /// The `start` argument of every `connect` so far; the handle stays valid after the connector
    /// moves into the supervisor.
    pub fn starts(&self) -> Arc<Mutex<Vec<bool>>> {
        Arc::clone(&self.starts)
    }
}

impl Connector for TestConnector {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        let start_calls = {
            let mut starts = lock(&self.starts);
            starts.push(start);
            starts.iter().filter(|s| **s).count()
        };
        if start && start_calls >= self.launch_on {
            self.daemon.restart();
        }
        SocketConnector {
            runtime: self.daemon.runtime.clone(),
        }
        .connect(start)
    }
}

/// A [`Connector`] that always fails the same way (what `BundleConnector` reports and a socket
/// cannot: an incompatibility with the daemon's path, a launch failure).
pub struct ScriptedConnector {
    pub failure: ConnectFailure,
    starts: Arc<Mutex<Vec<bool>>>,
}

impl ScriptedConnector {
    pub fn failing(failure: ConnectFailure) -> Self {
        Self {
            failure,
            starts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn starts(&self) -> Arc<Mutex<Vec<bool>>> {
        Arc::clone(&self.starts)
    }
}

impl Connector for ScriptedConnector {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        lock(&self.starts).push(start);
        Err(self.failure.clone())
    }
}

/// The common assertion of D3-READONLY: the app never calls `daemon.replace`, `daemon.stop`,
/// `repo.add`, a reserved method or one with `writes`, and only methods the contract knows.
pub fn assert_read_only(calls: &[Call]) {
    for (method, _) in calls {
        assert!(
            !matches!(
                method.as_str(),
                "daemon.replace" | "daemon.stop" | "repo.add"
            ),
            "D3-READONLY: the app called {method}"
        );
        match methods::spec(method) {
            Some(spec) => {
                assert!(
                    !spec.reserved && spec.writes == methods::RepoWrite::None,
                    "D3-READONLY: the app called {method}, which is reserved or writes"
                );
            }
            None => {
                panic!("D3-READONLY: the app called {method}, which the contract does not know")
            }
        }
    }
}
