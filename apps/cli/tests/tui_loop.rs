//! The TUI loop without a screen (INF-CKP-001, ADR-CKP-003 § 3, § 4, § 6):
//! the real `App`, channel thread and queues on `TestBackend`, over a fake
//! channel that serves raw frames. No daemon, no profile, no repo.
//!
//! - Synthetic microbench (Validation V3): a burst of 1,000 `worktree.state`
//!   events of 10 worktrees goes through the channel thread (decode
//!   included). It fails when the p95 of `t_client_recv` → `t_render` goes
//!   over 100 ms, naming the slowest stage. Apply ingests the fleet rows
//!   (US-CKP-001). The gate with the real daemon, end to end, is the
//!   `tui-modify` scenario of the engine bench (INF-GRP-002).
//! - Coalescing and input first: deterministic, no clock thresholds.
//! - Resync and reconnection redo the snapshot.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::event::{Event, WORKTREE_STATE};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, DaemonView, EngineStateView, EngineView, RepoStateView,
    RepoView, ResyncReason, UnavailableReason, WorktreeStateData, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::{
    AutostartView, GlobalSnapshot, RepoSnapshot, Scope, ScopeEventNotification,
    ScopeResyncNotification, ScopeSnapshot, ScopeSnapshotParams, ScopeSubscribeParams,
    ScopeSubscribeResult,
};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_cli::client::{self, Connector, Incoming, Link, LinkError};
use gitraptor_cli::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue::{self, Outlet};
use gitraptor_cli::tui::app::App;
use gitraptor_cli::tui::metrics::{COCKPIT_P95_NS, KEY_FEEDBACK_P95_NS};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

const REPO: &str = "r1";
const BURST: u64 = 1_000;
const WORKTREES: usize = 10;

/// What the fake daemon has seen and serves.
#[derive(Default)]
struct Daemon {
    connects: AtomicU32,
    global_snapshots: AtomicU32,
    repo_snapshots: AtomicU32,
    subscribes: AtomicU32,
}

struct FakeConnector {
    daemon: Arc<Daemon>,
    /// Each connection takes a new stream receiver from here.
    streams: Arc<Mutex<VecDeque<Receiver<Vec<u8>>>>>,
}

impl Connector for FakeConnector {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        let Some(rx) = self.streams.lock().unwrap().pop_front() else {
            return Err(LinkError::EngineUnavailable);
        };
        self.daemon.connects.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeLink {
            daemon: Arc::clone(&self.daemon),
            rx,
        }))
    }
}

struct FakeLink {
    daemon: Arc<Daemon>,
    rx: Receiver<Vec<u8>>,
}

impl Link for FakeLink {
    fn requester(&self) -> Option<gitraptor_api::scope::ConnectionRequester> {
        None
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        Ok(match method {
            methods::SCOPE_SNAPSHOT => {
                let params: ScopeSnapshotParams = serde_json::from_value(params).unwrap();
                serde_json::to_value(match params.scope {
                    Scope::Global => {
                        self.daemon.global_snapshots.fetch_add(1, Ordering::SeqCst);
                        global_snapshot()
                    }
                    Scope::Repo { .. } => {
                        self.daemon.repo_snapshots.fetch_add(1, Ordering::SeqCst);
                        repo_snapshot()
                    }
                })
                .unwrap()
            }
            methods::SCOPE_SUBSCRIBE => {
                self.daemon.subscribes.fetch_add(1, Ordering::SeqCst);
                let params: ScopeSubscribeParams = serde_json::from_value(params).unwrap();
                serde_json::to_value(ScopeSubscribeResult {
                    subscription: 1,
                    scope: params.scope,
                    from_seq: params.from_seq.unwrap_or(1),
                })
                .unwrap()
            }
            methods::REPO_LOCATE => json!({"repo_id": REPO, "worktree": {"untrusted": "/w"}}),
            _ => return Err(LinkError::Refused),
        })
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError> {
        match self.rx.recv_timeout(timeout) {
            Ok(bytes) => Ok(Some(Incoming::Frame {
                recv_ns: monotonic_ns(),
                bytes,
            })),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(LinkError::Lost),
        }
    }
}

fn global_snapshot() -> ScopeSnapshot {
    ScopeSnapshot::Global(GlobalSnapshot {
        run_id: "run".into(),
        scope_seq: 0,
        engine: EngineView {
            state: EngineStateView::Observing,
            git_version: Some("2.50.0".into()),
        },
        daemon: DaemonView {
            pid: 1,
            protocol: gitraptor_api::PROTOCOL_VERSION,
            binary_version: "0.0.0".into(),
            started_wall_ms: 0,
        },
        autostart: AutostartView::Unknown,
        repos: Vec::new(),
    })
}

fn repo_snapshot() -> ScopeSnapshot {
    ScopeSnapshot::Repo(RepoSnapshot {
        run_id: "run".into(),
        scope_seq: 0,
        repo: RepoView {
            fetched_utc_ms: None,
            repo_id: REPO.into(),
            state: RepoStateView::Observed,
            path: Untrusted::new("/w/.git"),
            base: BaseBranchView {
                name: None,
                status: BaseStatusView::Invalid,
            },
            worktrees: Vec::new(),
        },
    })
}

fn worktree_event(scope_seq: u64) -> Event {
    let worktrees = (0..WORKTREES)
        .map(|i| WorktreeView {
            last_activity_utc_ms: None,
            path: Untrusted::new(format!("/w/agent-{i}")),
            main: i == 0,
            admin_name: Some(UntrustedName::new(format!("agent-{i}"))),
            status: WorktreeStatus::Unavailable {
                reason: UnavailableReason::Missing,
            },
        })
        .collect();
    Event {
        seq: scope_seq,
        kind: WORKTREE_STATE.into(),
        version: 1,
        wall_ms: 0,
        timings: None,
        data: serde_json::to_value(WorktreeStateData {
            fetched_utc_ms: None,
            repo_id: REPO.into(),
            worktrees,
        })
        .unwrap(),
    }
}

fn frame(method: &str, params: impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(&Notification::new(method, params)).unwrap()
}

fn event_frame(scope_seq: u64) -> Vec<u8> {
    frame(
        methods::NOTIFY_SCOPE_EVENT,
        ScopeEventNotification {
            subscription: 1,
            scope: Scope::Repo {
                repo_id: REPO.into(),
            },
            scope_seq,
            event: worktree_event(scope_seq),
        },
    )
}

struct Harness {
    app: App<TestBackend>,
    input: Outlet,
    daemon: Arc<Daemon>,
    /// One sender per connection, in order.
    streams: Vec<Sender<Vec<u8>>>,
    channel: Option<client::ClientThread>,
}

impl Harness {
    /// A TUI over the fake daemon; `connections` streams are ready.
    fn new(connections: usize) -> Self {
        let daemon = Arc::new(Daemon::default());
        let mut senders = Vec::new();
        let mut receivers = VecDeque::new();
        for _ in 0..connections {
            let (tx, rx) = channel();
            senders.push(tx);
            receivers.push_back(rx);
        }
        let (inbox, input, engine) = queue::inbox();
        let terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let model = Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        );
        let mut app = App::new(terminal, model, inbox);
        let connector = FakeConnector {
            daemon: Arc::clone(&daemon),
            streams: Arc::new(Mutex::new(receivers)),
        };
        let channel = client::spawn(connector, Some("/w".into()), engine);
        app.attach(channel.cmds.clone());
        Self {
            app,
            input,
            daemon,
            streams: senders,
            channel: Some(channel),
        }
    }

    fn drive(&mut self, what: &str, done: impl Fn(&Model) -> bool) {
        let start = Instant::now();
        while !done(&self.app.model) {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}: {:?}",
                self.app.model.conn
            );
            self.app.step(Duration::from_millis(10)).unwrap();
        }
    }

    fn live(&mut self) {
        self.drive("live", |m| {
            m.conn == ConnState::Live && m.engine.repo.is_some() && m.engine.all_synced()
        });
    }

    fn applied(model: &Model) -> u64 {
        model.engine.repo.as_ref().map_or(0, |r| r.applied)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.streams.clear();
        if let Some(channel) = self.channel.take() {
            channel.shutdown();
        }
    }
}

/// The microbench: the Cockpit p95 gate (ADR-GRP-011 E2) and the key
/// feedback, under a burst of 1,000 events.
#[test]
fn a_burst_of_1000_events_is_painted_within_the_cockpit_budget() {
    let mut h = Harness::new(1);
    h.live();
    let frames: Vec<Vec<u8>> = (1..=BURST).map(event_frame).collect();
    let stream = h.streams[0].clone();
    let producer = std::thread::spawn(move || {
        for (i, f) in frames.into_iter().enumerate() {
            stream.send(f).unwrap();
            if i == BURST as usize / 2 {
                std::thread::yield_now();
            }
        }
    });
    // A key in the middle of the burst.
    std::thread::sleep(Duration::from_millis(2));
    h.input
        .send(Msg::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .unwrap();
    h.drive("the burst", |m| Harness::applied(m) == BURST);
    producer.join().unwrap();

    let metrics = &h.app.metrics;
    eprintln!("tui microbench: {metrics}");
    assert_eq!(
        h.app
            .model
            .engine
            .repo
            .as_ref()
            .and_then(|r| r.data.as_ref())
            .map(|d| d.worktrees.len()),
        Some(WORKTREES)
    );
    let p95 = metrics.total.p95().unwrap();
    assert!(
        p95 <= COCKPIT_P95_NS,
        "Cockpit p95 {:.1} ms > 100 ms; slowest stage: {:?}; {metrics}",
        p95 as f64 / 1e6,
        metrics.slowest_stage()
    );
    assert!(
        metrics.frames < BURST / 2,
        "the burst was not coalesced: {} frames",
        metrics.frames
    );
    let key = metrics.key.p95().unwrap();
    if key >= KEY_FEEDBACK_P95_NS {
        eprintln!("warning: key feedback p95 {:.1} ms", key as f64 / 1e6);
    }
}

/// Engine messages already queued are coalesced into few frames, and a key
/// queued behind them is painted in the first frame (input first).
#[test]
fn queued_messages_are_coalesced_and_input_goes_first() {
    let (inbox, input, engine) = queue::inbox();
    let terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let model = Model::new(
        Lang::En,
        Size {
            width: 80,
            height: 24,
        },
    );
    let mut app = App::new(terminal, model, inbox);
    let stamped = |msg| {
        Msg::Engine(Stamped {
            recv_ns: monotonic_ns(),
            decoded_ns: monotonic_ns(),
            msg,
        })
    };
    engine
        .send(stamped(EngineMsg::Snapshot(Box::new(repo_snapshot()))))
        .unwrap();
    for seq in 1..=BURST {
        engine
            .send(stamped(EngineMsg::Event {
                scope: Scope::Repo {
                    repo_id: REPO.into(),
                },
                scope_seq: seq,
                event: Box::new(worktree_event(seq)),
            }))
            .unwrap();
    }
    input
        .send(Msg::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .unwrap();
    app.step(Duration::ZERO).unwrap();
    // The key was applied and painted in the very first iteration.
    assert_eq!(app.metrics.frames, 1);
    assert_eq!(app.metrics.key.len(), 1);
    assert!(app.model.ui.notice.is_some());
    while Harness::applied(&app.model) < BURST {
        app.step(Duration::ZERO).unwrap();
    }
    // One frame per iteration, and the apply budget bounds each iteration.
    assert!(
        app.metrics.frames <= BURST / 10,
        "{} frames for {BURST} queued messages",
        app.metrics.frames
    );
    assert_eq!(app.metrics.total.len() as u64, BURST + 1);
}

#[test]
fn a_gap_takes_a_new_snapshot_without_resubscribing() {
    let mut h = Harness::new(1);
    h.live();
    let snapshots = h.daemon.repo_snapshots.load(Ordering::SeqCst);
    let subscribes = h.daemon.subscribes.load(Ordering::SeqCst);
    h.streams[0].send(event_frame(1)).unwrap();
    h.streams[0].send(event_frame(3)).unwrap();
    h.drive("the new snapshot", |m| {
        m.conn == ConnState::Live && m.engine.all_synced()
    });
    let start = Instant::now();
    while h.daemon.repo_snapshots.load(Ordering::SeqCst) == snapshots {
        assert!(start.elapsed() < Duration::from_secs(10));
        h.app.step(Duration::from_millis(10)).unwrap();
    }
    h.drive("live again", |m| {
        m.conn == ConnState::Live && m.engine.all_synced()
    });
    assert_eq!(h.daemon.subscribes.load(Ordering::SeqCst), subscribes);
    // The fake snapshot is at 0 again: the next contiguous event applies.
    h.streams[0].send(event_frame(1)).unwrap();
    h.drive("the event after the snapshot", |m| Harness::applied(m) == 1);
}

#[test]
fn a_daemon_resync_redoes_snapshot_and_subscription() {
    let mut h = Harness::new(1);
    h.live();
    let subscribes = h.daemon.subscribes.load(Ordering::SeqCst);
    h.streams[0]
        .send(frame(
            methods::NOTIFY_SCOPE_RESYNC,
            ScopeResyncNotification {
                scope: Scope::Global,
                reason: ResyncReason::ReplayUnavailable,
            },
        ))
        .unwrap();
    let start = Instant::now();
    while h.daemon.subscribes.load(Ordering::SeqCst) == subscribes {
        assert!(start.elapsed() < Duration::from_secs(10));
        h.app.step(Duration::from_millis(10)).unwrap();
    }
    assert_eq!(h.daemon.global_snapshots.load(Ordering::SeqCst), 2);
    h.drive("live again", |m| {
        m.conn == ConnState::Live && m.engine.all_synced()
    });
}

#[test]
fn a_lost_channel_reconnects_and_redoes_the_snapshots() {
    let mut h = Harness::new(2);
    h.live();
    h.streams[0].send(event_frame(1)).unwrap();
    h.drive("one event", |m| Harness::applied(m) == 1);
    // The daemon goes away: the first stream closes.
    let (dead, _) = channel();
    h.streams[0] = dead;
    // Stale marking in between is covered by the unit tests of `update`:
    // here the whole reconnection may land in one iteration.
    let daemon = Arc::clone(&h.daemon);
    h.drive("the reconnection", move |m| {
        daemon.connects.load(Ordering::SeqCst) == 2
            && m.conn == ConnState::Live
            && m.engine.all_synced()
    });
    assert_eq!(h.daemon.connects.load(Ordering::SeqCst), 2);
    assert_eq!(h.daemon.global_snapshots.load(Ordering::SeqCst), 2);
    assert!(!h.app.model.engine.repo.as_ref().unwrap().stale);
}
