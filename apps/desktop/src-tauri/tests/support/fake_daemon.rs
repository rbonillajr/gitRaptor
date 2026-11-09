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
use gitraptor_api::guard::{
    ExceptionPlanView, GuardLogEntry, GuardLogResult, GuardLogSummary, GuardStatus,
    LoggedOperation, LoggedRef, PendingAction, PendingKind, Permission, ProtectionState, RefChange,
    Rule,
};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, ConnectionProfile, DaemonView,
    DivergenceView, EngineStateView, EngineView, HeadView, HelloResult, IncompatibleData,
    McpSnapshot, RepoStateView, RepoTier, RepoView, ResyncNotification, ResyncReason,
    SessionStateView, SessionView, SessionsListParams, Snapshot, UnsubscribeParams, WorktreeStatus,
    WorktreeView,
};
use gitraptor_api::methods;
use gitraptor_api::prediction::{
    self, ConflictFileView, ConflictKindView, ConflictView, LimitCode, LineRange,
    ListPredictionParams, MergeBaseKind, PairDetail, PairLevel, PairReason, PairSideView,
    PairStateView, PairSummary, PredictionSummary,
};
use gitraptor_api::rpc::{ErrorObject, Id, Notification, Request, Response, code};
use gitraptor_api::scope::{
    AttentionView, AutostartView, ConnectionRequester, GlobalSnapshot, RepoSnapshot,
    RepoSummaryView, Scope, ScopeEventNotification, ScopeResyncNotification, ScopeSnapshot,
    ScopeSnapshotParams, ScopeSubscribeParams, ScopeSubscribeResult,
};
use gitraptor_api::{Untrusted, UntrustedName};
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
    /// `sessions.list` per repo: `(detection_available, sessions)`. A repo without an entry
    /// answers `(true, [])`.
    pub sessions: HashMap<String, (bool, Vec<SessionView>)>,
    pub candidates: Vec<CandidateView>,
    /// `prediction.list` per repo: the details the fake pages with `prediction::page`, in order
    /// (DS-US-DSK-008 T001). A repo without an entry answers an empty page.
    pub prediction: HashMap<String, Vec<PairDetail>>,
    /// Answer `prediction.list` with `METHOD_NOT_FOUND`, like a daemon that lacks it.
    pub refuse_list: bool,
    /// Push `scope.resync` (slow consumer) of the repo of the `nth` (1-based) `prediction.list`
    /// call right before answering it.
    pub resync_during_list: Option<usize>,
    /// `guard.status` answer; `None` answers `METHOD_NOT_FOUND` (DS-US-DSK-007 T001).
    pub guard_status: Option<GuardStatus>,
    /// `guard.log` answer; `None` answers `METHOD_NOT_FOUND`.
    pub guard_log: Option<GuardLogResult>,
    /// Answer `guard.status` with `INTERNAL` "guardrails unavailable".
    pub guard_status_fails: bool,
    /// Answer `guard.log` with `INTERNAL` "guardrails unavailable".
    pub guard_log_fails: bool,
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
            sessions: HashMap::new(),
            candidates: Vec::new(),
            prediction: HashMap::new(),
            refuse_list: false,
            resync_during_list: None,
            guard_status: None,
            guard_log: None,
            guard_status_fails: false,
            guard_log_fails: false,
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
    /// The subscription id the fake gave to each scope, so a pushed `scope.event` carries it.
    scope_subscriptions: Mutex<HashMap<Scope, u32>>,
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
                scope_subscriptions: Mutex::new(HashMap::new()),
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

    /// Pushes one `scope.event` of the repo scope `repo_id`, with the subscription the fake gave
    /// that scope (the global one when it never subscribed).
    pub fn push_repo_event(
        &self,
        repo_id: &str,
        scope_seq: u64,
        kind: &str,
        data: impl Serialize,
    ) -> bool {
        let scope = Scope::Repo {
            repo_id: repo_id.to_owned(),
        };
        let subscription = lock(&self.shared.scope_subscriptions)
            .get(&scope)
            .copied()
            .unwrap_or(1);
        let version = gitraptor_api::event::kind(kind).map_or(1, |k| k.version);
        self.push(&Notification::new(
            methods::NOTIFY_SCOPE_EVENT,
            ScopeEventNotification {
                subscription,
                scope,
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

    /// Pushes `scope.resync` for the repo scope `repo_id`.
    pub fn push_repo_resync(&self, repo_id: &str, reason: ResyncReason) -> bool {
        self.push(&Notification::new(
            methods::NOTIFY_SCOPE_RESYNC,
            ScopeResyncNotification {
                scope: Scope::Repo {
                    repo_id: repo_id.to_owned(),
                },
                reason,
            },
        ))
    }

    /// The subscription id the fake gave to the repo scope, `None` before it subscribed.
    pub fn repo_subscription(&self, repo_id: &str) -> Option<u32> {
        lock(&self.shared.scope_subscriptions)
            .get(&Scope::Repo {
                repo_id: repo_id.to_owned(),
            })
            .copied()
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
        if request.method == methods::PREDICTION_LIST {
            let nth = lock(&shared.calls)
                .iter()
                .filter(|(m, _)| m == methods::PREDICTION_LIST)
                .count();
            if lock(&shared.script).resync_during_list == Some(nth) {
                let repo_id = params["repo_id"].as_str().unwrap_or_default().to_owned();
                let resync = Notification::new(
                    methods::NOTIFY_SCOPE_RESYNC,
                    ScopeResyncNotification {
                        scope: Scope::Repo { repo_id },
                        reason: ResyncReason::SlowConsumer,
                    },
                );
                if !write_line(writer, &resync) {
                    return;
                }
            }
        }
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
                lock(&shared.scope_subscriptions).insert(p.scope.clone(), subscription);
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
            let Ok(p) = serde_json::from_value::<SessionsListParams>(params) else {
                return invalid(&id);
            };
            let (detection, sessions) = p
                .repo_id
                .and_then(|repo| script.sessions.get(&repo).cloned())
                .unwrap_or((true, Vec::new()));
            Response::ok(
                id,
                json!({"detection_available": detection, "sessions": sessions}),
            )
        }
        methods::PREDICTION_LIST => {
            if script.refuse_list {
                return Response::err(
                    Some(id),
                    ErrorObject::new(code::METHOD_NOT_FOUND, "method not found"),
                );
            }
            let Ok(p) = serde_json::from_value::<ListPredictionParams>(params) else {
                return invalid(&id);
            };
            if prediction::validate_list(&p).is_err() {
                return invalid(&id);
            }
            let mut details = script
                .prediction
                .get(&p.repo_id)
                .cloned()
                .unwrap_or_default();
            if let Some(ids) = &p.pair_ids {
                details.retain(|d| ids.contains(&d.summary.pair_id));
            }
            Response::ok(id, prediction::page(details, p.cursor.as_deref()))
        }
        methods::GUARD_STATUS => {
            if script.guard_status_fails {
                return Response::err(
                    Some(id),
                    ErrorObject::new(code::INTERNAL, "guardrails unavailable"),
                );
            }
            match script.guard_status {
                Some(status) => Response::ok(id, status),
                None => Response::err(
                    Some(id),
                    ErrorObject::new(code::METHOD_NOT_FOUND, "method not found"),
                ),
            }
        }
        methods::GUARD_LOG => {
            if script.guard_log_fails {
                return Response::err(
                    Some(id),
                    ErrorObject::new(code::INTERNAL, "guardrails unavailable"),
                );
            }
            match script.guard_log {
                Some(log) => Response::ok(id, log),
                None => Response::err(
                    Some(id),
                    ErrorObject::new(code::METHOD_NOT_FOUND, "method not found"),
                ),
            }
        }
        methods::EVENTS_UNSUBSCRIBE => match serde_json::from_value::<UnsubscribeParams>(params) {
            Ok(_) => Response::ok(id, json!({})),
            Err(_) => invalid(&id),
        },
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

// ------------------------------------------------------------ DS-US-DSK-004 scripts

/// The raw root of the main worktree of "tienda".
pub const TIENDA_MAIN: &str = "/w/tienda";
pub const TIENDA_PAGOS: &str = "/w/wt/feat-pagos";
pub const TIENDA_LOGIN: &str = "/w/wt/feat-login";
pub const NOTAS_MAIN: &str = "/w/notas";
/// Where the fake's repos are at in their streams.
pub const REPO_RUN: &str = "run-1";
pub const REPO_SEQ: u64 = 10;

/// A worktree on `branch`, `ahead`/`behind` the base, with `files` unstaged files.
pub fn worktree_view(
    root: &str,
    main: bool,
    branch: &str,
    ahead: u64,
    behind: u64,
    files: u32,
) -> WorktreeView {
    WorktreeView {
        path: Untrusted::new(root),
        main,
        admin_name: None,
        status: WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new(branch),
            },
            counts: ChangeCounts {
                staged: 0,
                unstaged: files,
                untracked: 0,
            },
            changes: Vec::new(),
            divergence: DivergenceView::Counted {
                ahead: CommitCountView {
                    count: ahead,
                    exact: true,
                },
                behind: CommitCountView {
                    count: behind,
                    exact: true,
                },
            },
        },
        last_activity_utc_ms: Some(1_700_000_000_000),
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
    }
}

/// A snapshot of the repo `repo_id` at `/<...>/.git` with these worktrees.
pub fn repo_snapshot(
    repo_id: &str,
    git_dir: &str,
    scope_seq: u64,
    worktrees: Vec<WorktreeView>,
) -> RepoSnapshot {
    RepoSnapshot {
        run_id: REPO_RUN.to_owned(),
        scope_seq,
        repo: RepoView {
            repo_id: repo_id.to_owned(),
            state: RepoStateView::Observed,
            path: Untrusted::new(git_dir),
            base: BaseBranchView {
                name: Some(UntrustedName::new("main")),
                status: BaseStatusView::Confirmed,
            },
            worktrees,
            fetched_utc_ms: Some(1_699_990_000_000),
            tier: Some(RepoTier::Active),
            checked_utc_ms: None,
            kept_temps: None,
            prediction: Some(PredictionSummary {
                limits: LimitCode::ALL.to_vec(),
                pairs: Vec::new(),
                pairs_truncated: false,
            }),
        },
    }
}

/// A present Claude Code session of `repo_id` in the worktree `root`.
pub fn session_view(
    repo_id: &str,
    session_id: &str,
    root: &str,
    state: SessionStateView,
    state_since: i64,
) -> SessionView {
    SessionView {
        repo_id: repo_id.to_owned(),
        session_id: session_id.to_owned(),
        worktree: Untrusted::new(root),
        actor: Actor::Agent {
            kind: gitraptor_api::actor::AgentKind::ClaudeCode,
            name: None,
            origin: gitraptor_api::actor::AgentOrigin::Detected,
        },
        state,
        started_utc_ms: 1_700_000_000_000,
        state_since_utc_ms: state_since,
        utc_offset_s: 0,
        ended_utc_ms: None,
        end_cause: None,
    }
}

/// The three worktrees of "tienda": main, `feat-pagos` (up 3, down 1, 2 files) and `feat-login`
/// (up 1, down 0).
pub fn tienda_worktrees() -> Vec<WorktreeView> {
    vec![
        worktree_view(TIENDA_MAIN, true, "main", 0, 0, 0),
        worktree_view(TIENDA_PAGOS, false, "feat-pagos", 3, 1, 2),
        worktree_view(TIENDA_LOGIN, false, "feat-login", 1, 0, 0),
    ]
}

/// Scripts the repos "tienda" (`/w/tienda/.git`, three worktrees, `claude-1` active in
/// `feat-pagos`, `claude-2` active in `feat-login`, detection available) and "notas" (main only,
/// no sessions), and lists both in the global snapshot.
pub fn script_tienda_and_notas(s: &mut Script) {
    s.global.repos = ["tienda", "notas"]
        .iter()
        .map(|name| RepoSummaryView {
            repo_id: (*name).to_owned(),
            state: RepoStateView::Observed,
            path: Untrusted::new(format!("/w/{name}/.git")),
            attention: AttentionView::unpublished(),
            tier: None,
        })
        .collect();
    s.repos.insert(
        "tienda".to_owned(),
        repo_snapshot("tienda", "/w/tienda/.git", REPO_SEQ, tienda_worktrees()),
    );
    s.repos.insert(
        "notas".to_owned(),
        repo_snapshot(
            "notas",
            "/w/notas/.git",
            REPO_SEQ,
            vec![worktree_view(NOTAS_MAIN, true, "main", 0, 0, 0)],
        ),
    );
    s.sessions.insert(
        "tienda".to_owned(),
        (
            true,
            vec![
                session_view(
                    "tienda",
                    "claude-1",
                    TIENDA_PAGOS,
                    SessionStateView::Active,
                    1_700_000_100_000,
                ),
                session_view(
                    "tienda",
                    "claude-2",
                    TIENDA_LOGIN,
                    SessionStateView::Active,
                    1_700_000_200_000,
                ),
            ],
        ),
    );
    s.sessions.insert("notas".to_owned(), (true, Vec::new()));
}

// ------------------------------------------------------------ DS-US-DSK-008 scripts

/// `rev` of the pairs the "tienda" prediction scripts publish in their snapshot.
pub const PAIR_REV: u64 = REPO_SEQ;
/// `computed_at_utc_ms` of those pairs.
pub const PAIR_COMPUTED: i64 = 1_700_000_300_000;

fn side(root: &str) -> PairSideView {
    PairSideView::Worktree {
        root: Untrusted::new(root),
    }
}

/// The id of the pair `a` against `b` (the base when `None`) of `repo_id`.
pub fn pair_id_of(repo_id: &str, a: &str, b: Option<&str>) -> String {
    prediction::pair_id(
        repo_id,
        std::path::Path::new(a),
        b.map(std::path::Path::new),
    )
}

/// A summary in `current` with this `level`.
pub fn summary_of(
    repo_id: &str,
    a: &str,
    b: Option<&str>,
    rev: u64,
    level: PairLevel,
) -> PairSummary {
    let conflict = level == PairLevel::Conflict;
    PairSummary {
        pair_id: pair_id_of(repo_id, a, b),
        rev,
        a: side(a),
        b: b.map_or(PairSideView::Base {}, side),
        state: PairStateView::Current,
        reason: None,
        level: Some(level),
        computed_at_utc_ms: Some(PAIR_COMPUTED),
        overlap_files: u32::from(!conflict),
        overlap_partial: false,
        conflict_files: u32::from(conflict),
        conflict_truncated: false,
    }
}

/// A pair that has no result: `state` with this `reason`.
pub fn pending_summary(repo_id: &str, a: &str, rev: u64, reason: PairReason) -> PairSummary {
    PairSummary {
        state: PairStateView::Pending,
        reason: Some(reason),
        level: None,
        computed_at_utc_ms: None,
        overlap_files: 0,
        conflict_files: 0,
        ..summary_of(repo_id, a, None, rev, PairLevel::Clean)
    }
}

/// The detail of a conflict pair: `pagos.rs`, `ours` lines 40..59 and `theirs` lines 44..62.
pub fn conflict_detail(summary: PairSummary, path: &str) -> PairDetail {
    PairDetail {
        summary,
        overlap: Vec::new(),
        conflict: Some(ConflictView {
            merge_base: MergeBaseKind::Single,
            files: vec![ConflictFileView {
                path: Untrusted::new(path),
                kind: ConflictKindView::Content,
                ours: vec![LineRange { start: 40, len: 19 }],
                theirs: vec![LineRange { start: 44, len: 18 }],
                hunks_truncated: false,
                has_merge_rule: false,
            }],
            truncated: false,
            rewrites_escalated: false,
        }),
    }
}

/// The detail of an overlap pair: the files both sides touch.
pub fn overlap_detail(summary: PairSummary, files: &[&str]) -> PairDetail {
    PairDetail {
        summary,
        overlap: files.iter().map(|f| Untrusted::new(*f)).collect(),
        conflict: None,
    }
}

/// Sets what the snapshot and `prediction.list` of `repo_id` say: the summaries in the
/// snapshot are the first `in_snapshot` of `details` (the rest only reach `prediction.list`;
/// `pairs_truncated` is then `true`).
pub fn script_prediction(
    s: &mut Script,
    repo_id: &str,
    details: Vec<PairDetail>,
    in_snapshot: usize,
) {
    let repo = s
        .repos
        .get_mut(repo_id)
        .expect("script_prediction: unknown repo");
    repo.repo.prediction = Some(PredictionSummary {
        limits: LimitCode::ALL.to_vec(),
        pairs: details
            .iter()
            .take(in_snapshot)
            .map(|d| d.summary.clone())
            .collect(),
        pairs_truncated: in_snapshot < details.len(),
    });
    s.prediction.insert(repo_id.to_owned(), details);
}

/// The ⚡ `feat-pagos` <-> `feat-login` with `pagos.rs` (ours 40+19, theirs 44+18) at `rev`.
pub fn pagos_login_conflict(rev: u64) -> PairDetail {
    conflict_detail(
        summary_of(
            "tienda",
            TIENDA_PAGOS,
            Some(TIENDA_LOGIN),
            rev,
            PairLevel::Conflict,
        ),
        "pagos.rs",
    )
}

/// The ⚠ `feat-pagos` <-> base with `README.md` at `rev`.
pub fn pagos_base_overlap(rev: u64) -> PairDetail {
    overlap_detail(
        summary_of("tienda", TIENDA_PAGOS, None, rev, PairLevel::Overlap),
        &["README.md"],
    )
}

/// "tienda" with the ⚡ `feat-pagos` <-> `feat-login` and the ⚠ `feat-pagos` <-> base, both in
/// the snapshot and in `prediction.list`.
pub fn script_tienda_with_predictions(s: &mut Script) {
    script_tienda_and_notas(s);
    script_prediction(
        s,
        "tienda",
        vec![pagos_login_conflict(PAIR_REV), pagos_base_overlap(PAIR_REV)],
        2,
    );
}

/// "tienda-pendiente": the base is unconfirmed, the pair `feat-pagos` <-> base is `pending` with
/// `base-unconfirmed`, and the ⚡ between the two worktrees is still there.
pub fn script_tienda_pending(s: &mut Script) {
    script_tienda_and_notas(s);
    s.repos.get_mut("tienda").expect("tienda").repo.base.status = BaseStatusView::Unconfirmed;
    let pending = pending_summary(
        "tienda",
        TIENDA_PAGOS,
        PAIR_REV,
        PairReason::BaseUnconfirmed,
    );
    let repo = s.repos.get_mut("tienda").expect("tienda");
    repo.repo.prediction = Some(PredictionSummary {
        limits: LimitCode::ALL.to_vec(),
        pairs: vec![pagos_login_conflict(PAIR_REV).summary, pending.clone()],
        pairs_truncated: false,
    });
    s.prediction.insert(
        "tienda".to_owned(),
        vec![pagos_login_conflict(PAIR_REV), overlap_detail(pending, &[])],
    );
}

// ------------------------------------------------------------ DS-US-DSK-007 scripts

/// 2023-11-15 10:58:00 UTC: when the force-push of the "tienda" script was denied.
pub const GUARD_PUSH_AT: i64 = 1_700_045_880_000;

/// A log entry: a force-push denial of `claude-code` from `/w/wt/feat-pagos` (branch
/// `feat-pagos`) to `main` in `origin` (`minimum.force-push` and `policy.protected-branch`), with
/// the top-level keys of `overrides` (wire names, `guard.log` form) replacing the defaults.
pub fn log_entry(overrides: Value) -> GuardLogEntry {
    let mut base = json!({
        "atMs": GUARD_PUSH_AT,
        "utcOffsetS": 0,
        "lastMs": GUARD_PUSH_AT,
        "count": 1,
        "worktree": {"untrusted": TIENDA_PAGOS},
        "branch": {"untrusted": "feat-pagos"},
        "actor": "claude-code",
        "operation": {
            "kind": "push",
            "remote": {"untrusted": "origin"},
            "refs": [{"name": {"untrusted": "main"}, "change": "force"}]
        },
        "kind": "denial",
        "detail": "full",
        "effect": "deny",
        "appliedEffect": "deny",
        "reasons": [
            {"rule": "minimum.force-push", "level": "minimum", "cause": "not-fast-forward"},
            {"rule": "policy.protected-branch", "level": "floor"}
        ],
        "layer": "hooks",
        "origin": "daemon",
        "decisionId": "d-push"
    });
    for (key, value) in overrides.as_object().expect("overrides are an object") {
        base[key.as_str()] = value.clone();
    }
    serde_json::from_value(base).expect("a valid guard.log entry")
}

/// `guard.status` of a repo with the given protection and pending action.
pub fn guard_status(state: ProtectionState, pending: Option<PendingAction>) -> GuardStatus {
    GuardStatus {
        repo_id: "tienda".to_owned(),
        state,
        permission: Permission::Granted,
        offer: false,
        protected_bases: vec![Untrusted::new("main")],
        base_confirmed: true,
        not_preventable: Vec::new(),
        last_refusal: Vec::new(),
        misnamed_settings: Vec::new(),
        pending,
        hooks: None,
        diagnostics: Vec::new(),
        minimum_set: None,
        confirm: None,
    }
}

/// An exception announced in a terminal: `operation` and `rules`, a 10 s window.
pub fn pending_exception(operation: LoggedOperation, rules: Vec<Rule>) -> PendingAction {
    PendingAction {
        action_id: "act-1".to_owned(),
        action: PendingKind::Exception,
        applies_at_ms: GUARD_PUSH_AT + 70_000,
        window_ms: 10_000,
        exception: Some(ExceptionPlanView { operation, rules }),
    }
}

/// The push of the default [`log_entry`] as the engine normalizes it.
pub fn forced_main_push() -> LoggedOperation {
    LoggedOperation::Push {
        remote: Some(Untrusted::new("origin")),
        refs: vec![LoggedRef {
            name: Untrusted::new("main"),
            change: RefChange::Force,
        }],
    }
}

/// The "tienda" log, most recent first: a force-push denial, a commit denial in `main`, a
/// rebase denial in `feat-login`, the `exception` and the `exception-cancelled` of the same
/// push, plus a `notice` and a `protection-state` that are not blocks. Seven entries.
pub fn tienda_guard_log() -> GuardLogResult {
    let later = GUARD_PUSH_AT + 60_000;
    GuardLogResult {
        since_ms: GUARD_PUSH_AT - 86_400_000,
        summary: GuardLogSummary {
            blocked: 3,
            notices: 1,
            rate_limited: 0,
            exceptions: 1,
            exceptions_rejected: 0,
        },
        entries: vec![
            log_entry(json!({
                "atMs": later + 2_000, "lastMs": later + 2_000, "kind": "exception-cancelled",
                "actor": "other", "effect": "deny", "appliedEffect": "deny",
                "decisionId": "d-cancelled"
            })),
            log_entry(json!({
                "atMs": later, "lastMs": later, "kind": "exception",
                "effect": "deny", "appliedEffect": "allow", "decisionId": "d-exception"
            })),
            log_entry(json!({
                "atMs": GUARD_PUSH_AT - 1_000, "lastMs": GUARD_PUSH_AT - 1_000,
                "worktree": {"untrusted": TIENDA_LOGIN}, "branch": {"untrusted": "feat-login"},
                "operation": {"kind": "rebase"},
                "reasons": [{"rule": "permission.rebase", "level": "local"}],
                "decisionId": "d-rebase"
            })),
            log_entry(json!({
                "atMs": GUARD_PUSH_AT - 2_000, "lastMs": GUARD_PUSH_AT - 2_000,
                "worktree": {"untrusted": TIENDA_MAIN}, "branch": {"untrusted": "main"},
                "actor": "other",
                "operation": {"kind": "commit", "stage": "pre-commit"},
                "reasons": [{"rule": "policy.forbidden-path", "level": "floor"}],
                "decisionId": "d-commit"
            })),
            log_entry(json!({})),
            log_entry(json!({
                "atMs": GUARD_PUSH_AT - 3_000, "lastMs": GUARD_PUSH_AT - 3_000,
                "kind": "notice", "effect": "allow", "appliedEffect": "allow",
                "operation": {"kind": "commit", "stage": "commit-msg"},
                "reasons": [{"rule": "authorship.trailer-required", "level": "profile"}],
                "decisionId": "d-notice"
            })),
            log_entry(json!({
                "atMs": GUARD_PUSH_AT - 4_000, "lastMs": GUARD_PUSH_AT - 4_000,
                "kind": "protection-state", "effect": "allow", "appliedEffect": "allow",
                "actor": null,
                "operation": {"kind": "protection-state", "from": "active", "to": "active",
                              "expected": true},
                "reasons": [], "decisionId": "d-protection"
            })),
        ],
        unlogged_periods: Vec::new(),
    }
}

/// "tienda" and "notas" plus the guard of "tienda": protected `full`, no pending action and the
/// seven-entry [`tienda_guard_log`].
pub fn script_tienda_guard(s: &mut Script) {
    script_tienda_and_notas(s);
    s.guard_status = Some(guard_status(ProtectionState::Full, None));
    s.guard_log = Some(tienda_guard_log());
}

/// The alternative script: `guard.status` answers (`hooks-only`) and `guard.log` fails with
/// `INTERNAL` "guardrails unavailable".
pub fn script_tienda_guard_log_unavailable(s: &mut Script) {
    script_tienda_and_notas(s);
    s.guard_status = Some(guard_status(ProtectionState::HooksOnly, None));
    s.guard_log_fails = true;
}
