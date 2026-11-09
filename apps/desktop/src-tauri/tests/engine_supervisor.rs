//! D3 (DS-US-DSK-003 § 9.4): the engine supervisor end to end against the fake daemon
//! (`support/fake_daemon.rs`), through a real Unix socket and the seam of `engine::link`.
//!
//! Every test collects the `EngineMessage`s of `Sink::Collect` and waits for them with a 5 s
//! deadline, never with a fixed sleep. Each test ends with the common assertion of D3-READONLY:
//! the app never called `daemon.replace`, `daemon.stop`, `repo.add` or a method with writes.
//! Temporary folders only (NFR-01).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fake_daemon::{ALL_CAPABILITIES, FakeDaemon, ScriptedConnector, assert_read_only};
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::Untrusted;
use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::catalog::Layer;
use gitraptor_api::discovery::CandidateView;
use gitraptor_api::messages::{
    ConnectionProfile, EngineStateView, EngineView, IncompatibleData, RepoStateView,
    ResyncReason as ApiResync,
};
use gitraptor_api::scope::{
    AttentionView, ConnectionRequester, RepoSummaryView, Scope, ScopeSubscribeParams,
};
use gitraptor_desktop::engine::link::{
    ConnectFailure, Connector, Incompatibility, IncompatibilityKind,
};
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink, SupervisorHandle};
use gitraptor_desktop::engine::wire::{
    CandidateDto, ConnState, EngineDto, EngineMessage, EngineStateDto, IncompatibleAction,
    IncompatibleWhich, ResyncReason, StoppedReason,
};

const DEADLINE: Duration = Duration::from_secs(5);

struct Harness {
    rx: Receiver<EngineMessage>,
    seen: Vec<EngineMessage>,
    daemon: Option<Arc<FakeDaemon>>,
    handle: Option<SupervisorHandle>,
}

impl Harness {
    fn spawn(
        connector: Box<dyn Connector>,
        own_raptor: Option<PathBuf>,
        daemon: Option<&Arc<FakeDaemon>>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = supervisor::spawn(connector, own_raptor, Sink::Collect(tx));
        Self {
            rx,
            seen: Vec::new(),
            daemon: daemon.map(Arc::clone),
            handle: Some(handle),
        }
    }

    /// Consumes messages until `pick` returns a value, for at most 5 s.
    fn wait_for<T>(&mut self, what: &str, mut pick: impl FnMut(&EngineMessage) -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(message) => {
                    let found = pick(&message);
                    self.seen.push(message);
                    if let Some(found) = found {
                        return found;
                    }
                }
                Err(err) => {
                    let tail: Vec<&EngineMessage> = self.seen.iter().rev().take(6).collect();
                    panic!(
                        "waiting for {what}: the supervisor sent nothing more ({err}); \
                         last messages (newest first): {tail:?}"
                    );
                }
            }
        }
    }

    fn wait_live(&mut self) {
        self.wait_for("`conn live`", |m| is_live(m).then_some(()));
    }

    fn retry(&self, start: bool) {
        let handle = self.handle.as_ref().expect("supervisor handle");
        assert!(
            handle.send(HubCmd::Retry { start }),
            "the supervisor thread ended before Retry"
        );
    }

    fn count(&self, pred: impl Fn(&EngineMessage) -> bool) -> usize {
        self.seen.iter().filter(|m| pred(m)).count()
    }
}

impl Drop for Harness {
    /// The common assertion of D3-READONLY, at the end of every test.
    fn drop(&mut self) {
        drop(self.handle.take());
        if std::thread::panicking() {
            return;
        }
        if let Some(daemon) = &self.daemon {
            assert_read_only(&daemon.calls());
        }
    }
}

fn is_live(m: &EngineMessage) -> bool {
    matches!(
        m,
        EngineMessage::Conn {
            state: ConnState::Live,
            ..
        }
    )
}

fn conn_state(m: &EngineMessage) -> Option<&ConnState> {
    match m {
        EngineMessage::Conn { state, .. } => Some(state),
        _ => None,
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn own_exe() -> PathBuf {
    std::env::current_exe().unwrap().canonicalize().unwrap()
}

fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn engine_dto(state: EngineStateDto, git: Option<&str>) -> EngineDto {
    EngineDto {
        state,
        git_version: git.map(str::to_owned),
    }
}

fn engine_state_messages(h: &Harness) -> Vec<EngineDto> {
    h.seen
        .iter()
        .filter_map(|m| match m {
            EngineMessage::EngineState { engine } => Some(engine.clone()),
            _ => None,
        })
        .collect()
}

fn global_subscribe(from_seq: u64, run_id: &str) -> ScopeSubscribeParams {
    ScopeSubscribeParams {
        scope: Scope::Global,
        from_seq: Some(from_seq),
        run_id: Some(run_id.to_owned()),
    }
}

fn subscribe_calls(daemon: &FakeDaemon) -> Vec<ScopeSubscribeParams> {
    daemon
        .calls_of("scope.subscribe")
        .into_iter()
        .map(|p| serde_json::from_value(p).expect("scope.subscribe params"))
        .collect()
}

fn engine_view(state: EngineStateView, git: Option<&str>) -> EngineView {
    EngineView {
        state,
        git_version: git.map(str::to_owned),
    }
}

// ---------------------------------------------------------------- D3-LINK

#[test]
fn connects_to_the_fake_daemon() {
    let daemon = FakeDaemon::start();
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    let session = h.wait_for("a `session` message after the handshake", |m| {
        matches!(m, EngineMessage::Session { .. }).then(|| m.clone())
    });
    assert_eq!(
        session,
        EngineMessage::Session {
            daemon_version: "0.1.0".to_owned(),
            protocol: PROTOCOL_VERSION,
            restricted: false,
            activity: true,
            discovery: true,
        },
        "D3-LINK: `session` must carry the hello of the fake daemon"
    );
    h.wait_live();
    let calls = daemon.calls();
    let methods: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).take(2).collect();
    assert_eq!(
        methods,
        ["hello", "connection.accept"],
        "D3-LINK: the app must greet and then accept the capabilities"
    );
    let accepted = calls[1].1["capabilities"].to_string();
    for cap in ["scope.activity", "observation.tiers", "prediction.pairs"] {
        assert!(
            accepted.contains(cap),
            "D3-LINK: connection.accept must ask for {cap}, asked {accepted}"
        );
    }
}

// -------------------------------------------------------------- D3-STOPPED

#[test]
fn a_stopped_engine_is_reported_and_retry_with_start_launches_it() {
    let daemon = FakeDaemon::stopped();
    let connector = daemon.launching_on_start_call(2);
    let starts = connector.starts();
    let mut h = Harness::spawn(Box::new(connector), None, Some(&daemon));
    h.wait_for("`conn stopped not-running` without an engine", |m| {
        matches!(
            conn_state(m),
            Some(ConnState::Stopped {
                reason: StoppedReason::NotRunning
            })
        )
        .then_some(())
    });
    assert!(!daemon.is_running(), "the fake must still be down");
    h.retry(true);
    h.wait_for("`conn starting` after Retry { start: true }", |m| {
        matches!(conn_state(m), Some(ConnState::Starting)).then_some(())
    });
    h.wait_live();
    assert!(
        daemon.is_running(),
        "D3-STOPPED: Retry {{ start: true }} must make the Connector launch the engine"
    );
    let starts = starts.lock().unwrap().clone();
    assert_eq!(
        starts.first(),
        Some(&true),
        "D3-STOPPED: the first attempt starts the engine (D5)"
    );
    assert_eq!(
        starts.iter().filter(|s| **s).count(),
        2,
        "D3-STOPPED: only the first attempt and Retry {{ start: true }} call connect(true), got {starts:?}"
    );
}

// ---------------------------------------------------------------- D3-LOST

#[test]
fn a_lost_connection_marks_data_stale_and_catches_up_alone() {
    let daemon = FakeDaemon::start();
    let connector = daemon.connector();
    let starts = connector.starts();
    let before = now_ms();
    let mut h = Harness::spawn(Box::new(connector), None, Some(&daemon));
    h.wait_live();
    assert_eq!(
        subscribe_calls(&daemon),
        [global_subscribe(6, "run-1")],
        "D3-LOST: subscribe from the snapshot's seq + 1"
    );
    daemon.stop();
    let lost = now_ms();

    let mut waits = Vec::new();
    h.wait_for("`conn reconnecting` with attempt 4", |m| {
        if let EngineMessage::Conn {
            state: ConnState::Reconnecting { attempt, retry_in_ms },
            stale_since_ms,
            ..
        } = m
        {
            let stale = stale_since_ms.expect("D3-LOST: reconnecting carries staleSinceMs");
            assert!(
                (before..=lost).contains(&stale),
                "D3-LOST: staleSinceMs {stale} must be the wall time of the last live ({before}..={lost})"
            );
            waits.push((*attempt, *retry_in_ms));
            return (*attempt == 4).then_some(());
        }
        None
    });
    assert_eq!(
        waits,
        [(1, 250), (2, 500), (3, 1000), (4, 2000)],
        "D3-LOST: the wait is 250 ms x 2^(n-1)"
    );

    daemon.set_global_position("run-2", 40);
    daemon.restart();
    let stale = h.wait_for("`conn live` again, without being asked", |m| match m {
        EngineMessage::Conn {
            state: ConnState::Live,
            stale_since_ms,
            ..
        } => Some(*stale_since_ms),
        _ => None,
    });
    assert_eq!(stale, None, "D3-LOST: back to live clears staleSinceMs");
    assert_eq!(
        daemon.calls_of("scope.snapshot").len(),
        2,
        "D3-LOST: a new global snapshot"
    );
    assert_eq!(
        subscribe_calls(&daemon).last(),
        Some(&global_subscribe(41, "run-2")),
        "D3-LOST: subscribe from N + 1 of the new run"
    );
    assert_eq!(h.count(|m| matches!(m, EngineMessage::Global(_))), 2);
    let starts = starts.lock().unwrap().clone();
    assert!(
        starts[0] && starts[1..].iter().all(|s| !s),
        "D3-LOST: a reconnection only connects (connect(false)), got {starts:?}"
    );
}

// ------------------------------------------------------------- D3-INCOMPAT

fn incompatible_of(h: &mut Harness) -> ConnState {
    h.wait_for("`conn incompatible`", |m| {
        matches!(conn_state(m), Some(ConnState::Incompatible { .. }))
            .then(|| conn_state(m).cloned())
            .flatten()
    })
}

#[test]
fn an_older_daemon_is_incompatible_with_its_version() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.incompatible = Some(IncompatibleData {
            daemon_protocol: PROTOCOL_VERSION - 1,
            binary_version: "0.0.8".to_owned(),
        });
    });
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    assert_eq!(
        incompatible_of(&mut h),
        ConnState::Incompatible {
            which: IncompatibleWhich::DaemonOlder,
            daemon_version: Some("0.0.8".to_owned()),
            daemon_path: None,
            stop_command: None,
            action: IncompatibleAction::None,
        },
        "D3-INCOMPAT: an older daemon, without a known path, has nothing to offer"
    );

    // With the path of another installation: the command to stop it, copiable.
    let exe = own_exe().to_string_lossy().into_owned();
    let connector = ScriptedConnector::failing(ConnectFailure::Incompatible(Incompatibility {
        which: IncompatibilityKind::DaemonOlder,
        daemon_version: Some("0.0.8".to_owned()),
        daemon_path: Some(exe.clone()),
        missing: Vec::new(),
    }));
    let mut h = Harness::spawn(
        Box::new(connector),
        Some(PathBuf::from("/nonexistent/own/raptor")),
        None,
    );
    assert_eq!(
        incompatible_of(&mut h),
        ConnState::Incompatible {
            which: IncompatibleWhich::DaemonOlder,
            daemon_version: Some("0.0.8".to_owned()),
            daemon_path: Some(exe.clone()),
            stop_command: Some(format!("{} 'daemon' 'stop'", quote(&exe))),
            action: IncompatibleAction::CopyStop,
        },
        "D3-INCOMPAT: another installation's daemon offers copy-stop with a quoted command"
    );
}

#[test]
fn a_newer_daemon_asks_to_update_the_app() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.incompatible = Some(IncompatibleData {
            daemon_protocol: PROTOCOL_VERSION + 1,
            binary_version: "9.9.0".to_owned(),
        });
    });
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    assert_eq!(
        incompatible_of(&mut h),
        ConnState::Incompatible {
            which: IncompatibleWhich::AppOlder,
            daemon_version: Some("9.9.0".to_owned()),
            daemon_path: None,
            stop_command: None,
            action: IncompatibleAction::UpdateApp,
        },
        "D3-INCOMPAT: a newer daemon asks to update the app"
    );
}

#[test]
fn a_missing_capability_is_incompatible() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.capabilities = Some(
            ALL_CAPABILITIES
                .iter()
                .filter(|c| **c != "connection.requester")
                .map(|c| (*c).to_owned())
                .collect(),
        );
    });
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    assert_eq!(
        incompatible_of(&mut h),
        ConnState::Incompatible {
            which: IncompatibleWhich::MissingCapabilities,
            daemon_version: Some("0.1.0".to_owned()),
            daemon_path: None,
            stop_command: None,
            action: IncompatibleAction::None,
        },
        "D3-INCOMPAT: a daemon without connection.requester is incompatible"
    );
    assert!(
        daemon.calls_of("scope.snapshot").is_empty()
            && daemon.calls_of("scope.subscribe").is_empty(),
        "D3-INCOMPAT: nothing is read from an incompatible daemon"
    );
    assert_eq!(h.count(is_live), 0, "D3-INCOMPAT: never live");
}

#[test]
fn the_own_engine_missing_capabilities_offers_start() {
    let exe = own_exe();
    let connector = ScriptedConnector::failing(ConnectFailure::Incompatible(Incompatibility {
        which: IncompatibilityKind::MissingCapabilities,
        daemon_version: Some("0.1.0".to_owned()),
        daemon_path: Some(exe.to_string_lossy().into_owned()),
        missing: vec!["connection.requester".to_owned()],
    }));
    let mut h = Harness::spawn(Box::new(connector), Some(exe.clone()), None);
    assert_eq!(
        incompatible_of(&mut h),
        ConnState::Incompatible {
            which: IncompatibleWhich::MissingCapabilities,
            daemon_version: Some("0.1.0".to_owned()),
            daemon_path: Some(exe.to_string_lossy().into_owned()),
            stop_command: None,
            action: IncompatibleAction::Start,
        },
        "D3-INCOMPAT: the app's own engine offers start, with no stopCommand"
    );
}

// ------------------------------------------------------------- D3-NOREPOS

#[test]
fn no_repos_delivers_the_engine_state_and_the_candidates() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.global.engine = engine_view(EngineStateView::NoRepos, Some("2.45.1"));
        s.candidates = vec![CandidateView {
            path: "/Users/me/code/alpha".to_owned(),
            name: "alpha".to_owned(),
            root: "/Users/me/code".to_owned(),
            found_utc_ms: 1,
        }];
    });
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    let global = h.wait_for("a `global` snapshot", |m| match m {
        EngineMessage::Global(g) => Some(g.clone()),
        _ => None,
    });
    assert_eq!(
        global.engine,
        engine_dto(EngineStateDto::NoRepos, Some("2.45.1"))
    );
    assert!(global.repos.is_empty(), "D3-NOREPOS: no repos observed");
    let (candidates, replace) = h.wait_for("`discovered` candidates", |m| match m {
        EngineMessage::Discovered {
            candidates,
            replace,
        } => Some((candidates.clone(), *replace)),
        _ => None,
    });
    assert_eq!(
        candidates,
        [CandidateDto {
            name: "alpha".to_owned(),
            path: "/Users/me/code/alpha".to_owned()
        }]
    );
    assert!(
        replace,
        "D3-NOREPOS: discovery.candidates replaces the list"
    );
    h.wait_live();
}

// ---------------------------------------------------------------- D3-SEQ

#[test]
fn a_gap_takes_a_new_snapshot() {
    let daemon = FakeDaemon::start();
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    h.wait_live();
    let waiting = engine_view(EngineStateView::WaitingForGit, None);
    assert!(
        daemon.push_global_event(6, "engine.state", &waiting),
        "no connection to push on"
    );
    h.wait_for("the `engineState` of seq 6", |m| {
        matches!(m, EngineMessage::EngineState { .. }).then_some(())
    });

    daemon.set_global_position("run-1", 20);
    let skipped = engine_view(EngineStateView::NoRepos, None);
    assert!(
        daemon.push_global_event(8, "engine.state", &skipped),
        "no connection to push on"
    );
    h.wait_for("`conn syncing` after the gap", |m| {
        matches!(conn_state(m), Some(ConnState::Syncing)).then_some(())
    });
    h.wait_live();
    assert_eq!(
        daemon.calls_of("scope.snapshot").len(),
        2,
        "D3-SEQ: a gap takes a new snapshot"
    );
    assert_eq!(
        subscribe_calls(&daemon).last(),
        Some(&global_subscribe(21, "run-1")),
        "D3-SEQ: and subscribes from its seq + 1"
    );
    assert_eq!(
        engine_state_messages(&h),
        [engine_dto(EngineStateDto::WaitingForGit, None)],
        "D3-SEQ: the event after the gap is not applied"
    );
}

#[test]
fn a_duplicate_is_dropped() {
    let daemon = FakeDaemon::start();
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    h.wait_live();
    let waiting = engine_view(EngineStateView::WaitingForGit, None);
    let stale = engine_view(EngineStateView::NoRepos, None);
    let observing = engine_view(EngineStateView::Observing, Some("2.46.0"));
    for (seq, view) in [(6, &waiting), (5, &stale), (6, &stale), (7, &observing)] {
        assert!(
            daemon.push_global_event(seq, "engine.state", view),
            "no connection to push on"
        );
    }
    h.wait_for("the `engineState` of seq 7", |m| {
        matches!(m, EngineMessage::EngineState { engine } if engine.state == EngineStateDto::Observing)
            .then_some(())
    });
    assert_eq!(
        engine_state_messages(&h),
        [
            engine_dto(EngineStateDto::WaitingForGit, None),
            engine_dto(EngineStateDto::Observing, Some("2.46.0")),
        ],
        "D3-SEQ: an event at or below the last applied one is dropped"
    );
    assert_eq!(
        daemon.calls_of("scope.snapshot").len(),
        1,
        "D3-SEQ: a duplicate takes no snapshot"
    );
}

// -------------------------------------------------------------- D3-RESYNC

#[test]
fn a_scope_resync_carries_its_reason_until_live() {
    let daemon = FakeDaemon::start();
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    h.wait_live();
    daemon.set_global_position("run-1", 30);
    assert!(
        daemon.push_scope_resync(ApiResync::ReplayUnavailable),
        "no connection to push on"
    );
    let mut carried = 0;
    let resync = h.wait_for("`conn live` after the resync", |m| match m {
        EngineMessage::Conn {
            state: ConnState::Live,
            resync,
            ..
        } => Some(*resync),
        EngineMessage::Conn { resync, .. } => {
            assert_eq!(
                *resync,
                Some(ResyncReason::ReplayUnavailable),
                "D3-RESYNC: every conn until live carries the reason"
            );
            carried += 1;
            None
        }
        _ => None,
    });
    assert!(
        carried >= 1,
        "D3-RESYNC: at least one conn must carry the reason before live"
    );
    assert_eq!(resync, None, "D3-RESYNC: live clears the reason");
    assert_eq!(daemon.calls_of("scope.snapshot").len(), 2);
    assert_eq!(
        subscribe_calls(&daemon).last(),
        Some(&global_subscribe(31, "run-1"))
    );
}

#[test]
fn a_slow_consumer_reconnects_with_growing_wait() {
    let daemon = FakeDaemon::start();
    let connector = daemon.connector();
    let starts = connector.starts();
    let mut h = Harness::spawn(Box::new(connector), None, Some(&daemon));
    let mut waits = Vec::new();
    for round in 0..2 {
        h.wait_live();
        assert!(
            daemon.push_events_resync(ApiResync::SlowConsumer),
            "no connection to push on"
        );
        let (retry_in_ms, resync) =
            h.wait_for("`conn reconnecting` after events.resync", |m| match m {
                EngineMessage::Conn {
                    state: ConnState::Reconnecting { retry_in_ms, .. },
                    resync,
                    ..
                } => Some((*retry_in_ms, *resync)),
                _ => None,
            });
        assert_eq!(
            resync,
            Some(ResyncReason::SlowConsumer),
            "D3-RESYNC: round {round}: the reconnection names the reason"
        );
        waits.push(retry_in_ms);
    }
    assert!(
        waits[1] > waits[0]
            && waits
                .iter()
                .all(|w| [250, 500, 1000, 2000, 4000, 5000].contains(w)),
        "D3-RESYNC: the wait keeps growing across slow-consumer sessions, got {waits:?}"
    );
    let starts = starts.lock().unwrap().clone();
    assert!(
        starts[0] && starts[1..].iter().all(|s| !s),
        "D3-RESYNC: a reconnection only connects, got {starts:?}"
    );
}

// ----------------------------------------------------------- D3-RESTRICTED

/// A window that must not show repos: one `engine.snapshot`, `engineOnly`, no subscription.
fn assert_restricted(daemon: &Arc<FakeDaemon>, expected_engine: &EngineDto, why: &str) {
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(daemon));
    let session = h.wait_for("a restricted `session`", |m| match m {
        EngineMessage::Session { restricted, .. } => Some(*restricted),
        _ => None,
    });
    assert!(
        session,
        "D3-RESTRICTED ({why}): session.restricted must be true"
    );
    let engine = h.wait_for("`engineOnly`", |m| match m {
        EngineMessage::EngineOnly { engine } => Some(engine.clone()),
        _ => None,
    });
    assert_eq!(
        &engine, expected_engine,
        "D3-RESTRICTED ({why}): engineOnly carries the engine state"
    );
    h.wait_live();
    assert_eq!(
        daemon.calls_of("engine.snapshot").len(),
        1,
        "D3-RESTRICTED ({why}): one engine.snapshot"
    );
    for method in ["scope.subscribe", "scope.snapshot", "discovery.candidates"] {
        assert!(
            daemon.calls_of(method).is_empty(),
            "D3-RESTRICTED ({why}): the restricted view must not call {method}"
        );
    }
    assert_eq!(
        h.count(|m| matches!(
            m,
            EngineMessage::Global(_)
                | EngineMessage::Discovered { .. }
                | EngineMessage::RepoObserved { .. }
        )),
        0,
        "D3-RESTRICTED ({why}): no repo data reaches the webview"
    );
}

#[test]
fn an_agent_requester_is_restricted_and_never_subscribes() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.requester = Some(ConnectionRequester::Resolved {
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            layer: Layer::Mcp,
            confirmable: false,
        });
        s.global.engine = engine_view(EngineStateView::Observing, Some("2.45.1"));
    });
    assert_restricted(
        &daemon,
        &engine_dto(EngineStateDto::Observing, Some("2.45.1")),
        "agent requester",
    );
}

#[test]
fn an_mcp_profile_is_restricted() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.profile = ConnectionProfile::Mcp;
        s.global.engine = engine_view(EngineStateView::NoRepos, Some("2.45.1"));
    });
    // The MCP snapshot carries the state only, never the Git version (SEC-12).
    assert_restricted(
        &daemon,
        &engine_dto(EngineStateDto::NoRepos, None),
        "mcp profile",
    );
}

#[test]
fn an_unverified_requester_is_restricted() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.requester = Some(ConnectionRequester::Unverified);
        s.global.engine = engine_view(EngineStateView::Observing, Some("2.45.1"));
    });
    assert_restricted(
        &daemon,
        &engine_dto(EngineStateDto::Observing, Some("2.45.1")),
        "unverified requester",
    );
}

#[test]
fn a_missing_requester_is_restricted() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.requester = None;
        s.global.engine = engine_view(EngineStateView::Observing, Some("2.45.1"));
    });
    assert_restricted(
        &daemon,
        &engine_dto(EngineStateDto::Observing, Some("2.45.1")),
        "missing requester",
    );
}

// ----------------------------------------------------------- D3-UNTRUSTED

/// No character that acts on a terminal or flips text, and no JSON escape of one.
fn assert_no_raw_escape(json: &str, what: &str) {
    assert!(
        !json.contains('\u{202e}') && !json.contains("\\u001b") && !json.contains("\\u0007"),
        "D3-UNTRUSTED: {what} carries a raw escape: {json}"
    );
}

#[test]
fn untrusted_text_reaches_the_webview_escaped() {
    let daemon = FakeDaemon::start();
    daemon.script(|s| {
        s.global.daemon.binary_version = "0.1.0\u{202e}".to_owned();
        s.global.engine = engine_view(EngineStateView::Observing, Some("2.45\u{1b}]52;c;x\u{7}"));
        s.global.repos = vec![RepoSummaryView {
            repo_id: "r1".to_owned(),
            state: RepoStateView::Observed,
            path: Untrusted::new("/w/pro\u{202e}j/.git"),
            attention: AttentionView::unpublished(),
            tier: None,
        }];
        s.candidates = vec![CandidateView {
            path: "/c/al\u{1b}]52;c;QUJD\u{7}pha".to_owned(),
            name: "al\u{1b}]52;c;QUJD\u{7}pha".to_owned(),
            root: "/c".to_owned(),
            found_utc_ms: 1,
        }];
    });
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    let global = h.wait_for("a `global` snapshot", |m| match m {
        EngineMessage::Global(g) => Some(g.clone()),
        _ => None,
    });
    let candidates = h.wait_for("`discovered` candidates", |m| match m {
        EngineMessage::Discovered { candidates, .. } => Some(candidates.clone()),
        _ => None,
    });
    h.wait_live();
    for m in &h.seen {
        assert_no_raw_escape(&serde_json::to_string(m).unwrap(), "a message");
    }
    let repo = &global.repos[0];
    assert!(
        repo.path.contains("\\u{202e}"),
        "D3-UNTRUSTED: repo path {:?}",
        repo.path
    );
    assert!(
        repo.name.contains("\\u{202e}"),
        "D3-UNTRUSTED: repo name {:?}",
        repo.name
    );
    assert!(
        global.daemon.version.contains("\\u{202e}"),
        "D3-UNTRUSTED: daemon version"
    );
    let git = global.engine.git_version.expect("git version");
    assert!(
        git.contains("\\x1b]52;"),
        "D3-UNTRUSTED: git version {git:?}"
    );
    assert!(
        candidates[0].path.contains("\\x1b]52;"),
        "D3-UNTRUSTED: candidate path {:?}",
        candidates[0].path
    );
    assert!(
        candidates[0].name.contains("\\x1b]52;"),
        "D3-UNTRUSTED: candidate name {:?}",
        candidates[0].name
    );
}

#[test]
fn an_incompatibility_from_the_daemon_is_sanitized() {
    let connector = ScriptedConnector::failing(ConnectFailure::Incompatible(Incompatibility {
        which: IncompatibilityKind::DaemonOlder,
        daemon_version: Some("0.0.8\u{202e}".to_owned()),
        daemon_path: Some("/opt/\u{1b}]52;c;AAAA\u{7}/raptor".to_owned()),
        missing: Vec::new(),
    }));
    let mut h = Harness::spawn(Box::new(connector), None, None);
    let ConnState::Incompatible {
        daemon_version,
        daemon_path,
        stop_command,
        action,
        ..
    } = incompatible_of(&mut h)
    else {
        unreachable!("incompatible_of returns an Incompatible")
    };
    let version = daemon_version.expect("daemon version");
    let path = daemon_path.expect("daemon path");
    assert!(
        version.contains("\\u{202e}"),
        "D3-UNTRUSTED: version {version:?}"
    );
    assert!(path.contains("\\x1b]52;"), "D3-UNTRUSTED: path {path:?}");
    assert_no_raw_escape(&format!("{version}{path}"), "the incompatibility");
    assert_eq!(
        stop_command, None,
        "D3-UNTRUSTED: no stop command for a path with SEC-12 characters (M-1)"
    );
    assert_eq!(
        action,
        IncompatibleAction::None,
        "D3-UNTRUSTED: without a command there is nothing to copy"
    );
}

// ------------------------------------------------------------ D3-READONLY

#[test]
fn never_sends_replace_stop_or_writes() {
    let daemon = FakeDaemon::start();
    let mut h = Harness::spawn(Box::new(daemon.connector()), None, Some(&daemon));
    h.wait_live();
    // A gap and a drop: the paths that talk to the daemon again.
    daemon.set_global_position("run-1", 20);
    assert!(daemon.push_global_event(
        9,
        "engine.state",
        engine_view(EngineStateView::NoRepos, None)
    ));
    h.wait_for("`conn syncing`", |m| {
        matches!(conn_state(m), Some(ConnState::Syncing)).then_some(())
    });
    h.wait_live();
    let calls = daemon.calls();
    let called: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
    for expected in [
        "hello",
        "connection.accept",
        "scope.snapshot",
        "scope.subscribe",
    ] {
        assert!(
            called.contains(&expected),
            "D3-READONLY: the supervisor never talked to the daemon (no {expected}); calls: {called:?}"
        );
    }
    assert_read_only(&calls);
}
