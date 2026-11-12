//! D2, D3, D17 (DS-US-DSK-005 T008): the supervisor asks the daemon for the lanes of the open repo
//! by itself (`graph.lanes`, no IPC command), without blocking the stream, one query at a time,
//! and the webview gets `graph` or `graphUnavailable`.
//!
//! Same arrange as `prediction_supervisor.rs`: the fake daemon of `support/fake_daemon.rs` (a real
//! Unix socket, `graph.lanes` answered out of order by a thread, one query in flight per
//! connection like the daemon), `Sink::Collect`, a 5 s deadline, never a fixed sleep. Every test
//! ends with the read-only assertion. Temporary folders only (NFR-01).
//!
//! Expected values come from the story: the request is `{"repo_id": ...}` and nothing else, the
//! spacing is `max(1 s, 2 x last duration)`, `query-busy` retries after the spacing, a cancelled
//! query frees the connection's slot only when it answers.
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fake_daemon::{
    FakeDaemon, GRAPH_COMPUTED, REPO_SEQ, TIENDA_LOGIN, TIENDA_PAGOS, assert_read_only,
    script_tienda_graph, tienda_graph, tienda_worktrees, worktree_view,
};
use gitraptor_api::graph::GraphLanes;
use gitraptor_api::messages::WorktreeStateData;
use gitraptor_api::methods;
use gitraptor_api::rpc::{Id, code};
use gitraptor_api::scope::ConnectionRequester;
use gitraptor_desktop::engine::ingest::key;
use gitraptor_desktop::engine::link::Connector;
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink, SupervisorHandle};
use gitraptor_desktop::engine::wire::{ConnState, EngineMessage, GraphDto, GraphUnavailableReason};
use serde_json::json;

const DEADLINE: Duration = Duration::from_secs(5);
const GRAPH: &str = "graph.lanes";
const CANCEL: &str = "query.cancel";

struct Harness {
    rx: Receiver<EngineMessage>,
    seen: Vec<EngineMessage>,
    daemon: Arc<FakeDaemon>,
    handle: Option<SupervisorHandle>,
}

impl Harness {
    fn spawn(daemon: &Arc<FakeDaemon>, connector: Box<dyn Connector>) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = supervisor::spawn(connector, None, Sink::Collect(tx));
        let mut h = Self {
            rx,
            seen: Vec::new(),
            daemon: Arc::clone(daemon),
            handle: Some(handle),
        };
        h.wait_for("`conn live`", |m| {
            matches!(
                m,
                EngineMessage::Conn {
                    state: ConnState::Live,
                    ..
                }
            )
            .then_some(())
        });
        h
    }

    fn live(daemon: &Arc<FakeDaemon>) -> Self {
        Self::spawn(daemon, Box::new(daemon.connector()))
    }

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
                    let tail: Vec<&EngineMessage> = self.seen.iter().rev().take(4).collect();
                    panic!(
                        "waiting for {what}: the supervisor sent nothing more ({err}); \
                         last messages (newest first): {tail:?}"
                    );
                }
            }
        }
    }

    fn open(&self, repo_id: &str) {
        let handle = self.handle.as_ref().expect("supervisor handle");
        assert!(
            handle.send(HubCmd::Open {
                repo_id: repo_id.to_owned()
            }),
            "the supervisor thread ended before Open"
        );
    }

    /// The next `graph` message of `repo_id`: its graph and the backend time.
    fn wait_graph(&mut self, repo_id: &str) -> (GraphDto, u64) {
        let id = repo_id.to_owned();
        self.wait_for(
            &format!("a `graph` message of {repo_id}"),
            move |m| match m {
                EngineMessage::Graph {
                    repo_id,
                    graph,
                    perf,
                } if *repo_id == id => Some((graph.clone(), perf.backend_ns)),
                _ => None,
            },
        )
    }

    fn wait_unavailable(&mut self, repo_id: &str) -> GraphUnavailableReason {
        let id = repo_id.to_owned();
        self.wait_for(
            &format!("a `graphUnavailable` message of {repo_id}"),
            move |m| match m {
                EngineMessage::GraphUnavailable { repo_id, reason } if *repo_id == id => {
                    Some(*reason)
                }
                _ => None,
            },
        )
    }

    /// Consumes messages up to the next `worktrees` of `repo_id`: a barrier after an event pushed
    /// before it, since the stream is read in order.
    fn wait_worktrees(&mut self, repo_id: &str) {
        let id = repo_id.to_owned();
        self.wait_for(&format!("a `worktrees` message of {repo_id}"), move |m| {
            matches!(m, EngineMessage::Worktrees { repo_id, .. } if *repo_id == id).then_some(())
        });
    }

    fn graph_messages(&self) -> Vec<(&str, &GraphDto)> {
        self.seen
            .iter()
            .filter_map(|m| match m {
                EngineMessage::Graph { repo_id, graph, .. } => Some((repo_id.as_str(), graph)),
                _ => None,
            })
            .collect()
    }

    fn unavailable_count(&self) -> usize {
        self.seen
            .iter()
            .filter(|m| matches!(m, EngineMessage::GraphUnavailable { .. }))
            .count()
    }
}

impl Drop for Harness {
    /// The common assertion: the app never writes, whatever the test did.
    fn drop(&mut self) {
        drop(self.handle.take());
        if std::thread::panicking() {
            return;
        }
        assert_read_only(&self.daemon.calls());
    }
}

fn daemon_with(script: impl FnOnce(&mut fake_daemon::Script)) -> Arc<FakeDaemon> {
    let daemon = FakeDaemon::start();
    daemon.script(script);
    daemon
}

fn tienda_daemon() -> Arc<FakeDaemon> {
    daemon_with(script_tienda_graph)
}

fn graph_calls(daemon: &FakeDaemon) -> usize {
    daemon.calls_of(GRAPH).len()
}

fn git_event(daemon: &FakeDaemon, seq: u64) {
    assert!(
        daemon.push_repo_event(
            "tienda",
            seq,
            "git.event",
            json!({"repo_id": "tienda", "seq": seq}),
        ),
        "the fake could not push git.event"
    );
}

fn worktree_state(daemon: &FakeDaemon, seq: u64, pagos_ahead: u64, unstaged_files: u32) {
    let mut worktrees = tienda_worktrees();
    worktrees[1] = worktree_view(
        TIENDA_PAGOS,
        false,
        "feat-pagos",
        pagos_ahead,
        1,
        unstaged_files,
    );
    assert!(
        daemon.push_repo_event(
            "tienda",
            seq,
            "worktree.state",
            WorktreeStateData {
                repo_id: "tienda".to_owned(),
                worktrees,
                fetched_utc_ms: Some(1_699_990_500_000),
            },
        ),
        "the fake could not push worktree.state"
    );
}

// ---------------------------------------------------------------- D2-LANES

#[test]
fn opening_a_repo_requests_graph_lanes_and_delivers_lanes_with_worktree_keys() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let (graph, _) = h.wait_graph("tienda");

    assert_eq!(
        daemon.calls_of(GRAPH),
        [json!({"repo_id": "tienda"})],
        "D2-LANES: one graph.lanes with the repo id and nothing else, made by the supervisor itself"
    );
    assert_eq!(graph.computed_utc_ms, GRAPH_COMPUTED);
    let keys: Vec<&str> = graph
        .lanes
        .iter()
        .map(|l| l.worktree_key.as_str())
        .collect();
    assert_eq!(
        keys,
        [key(TIENDA_PAGOS), key(TIENDA_LOGIN)],
        "D2-LANES: each lane carries the key of its raw worktree root, the fleet's key"
    );
    let wire = serde_json::to_value(&graph).expect("the graph serialises");
    assert_eq!(wire["base"], json!({"state": "ready", "name": "main"}));
    assert!(
        !wire.to_string().contains(TIENDA_PAGOS),
        "SEC-12: the raw worktree path never reaches the webview"
    );
}

#[test]
fn a_base_missing_graph_reaches_the_webview_as_not_computable() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda-develop");
    let (graph, _) = h.wait_graph("tienda-develop");
    let wire = serde_json::to_value(&graph).expect("the graph serialises");
    assert_eq!(
        wire["base"],
        json!({"state": "not-computable", "reason": "base-missing", "name": "develop"}),
        "D2-LANES: a missing base reaches the webview as not-computable with the base named"
    );
    assert_eq!(
        wire["lanes"],
        json!([]),
        "D2-LANES: no lane against a missing base"
    );
}

#[test]
fn graph_messages_carry_backend_perf() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let (_, backend_ns) = h.wait_graph("tienda");
    assert!(
        backend_ns > 0 && backend_ns < 5_000_000_000,
        "NFR-13.4: perf.backendNs is the time since the reply was read (never zero): {backend_ns}"
    );
}

// ---------------------------------------------------------------- D3-LIVE

#[test]
fn a_git_event_marks_the_graph_stale_and_asks_again_once() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_graph("tienda");

    // Three events inside the spacing are one new request, not three.
    git_event(&daemon, REPO_SEQ + 1);
    git_event(&daemon, REPO_SEQ + 2);
    git_event(&daemon, REPO_SEQ + 3);
    let _ = h.wait_graph("tienda");
    assert_eq!(
        graph_calls(&daemon),
        2,
        "D3-LIVE: the first request plus one for the burst of git.event"
    );
    // A barrier: everything pushed before it was read.
    worktree_state(&daemon, REPO_SEQ + 4, 3, 2);
    h.wait_worktrees("tienda");
    assert_eq!(
        graph_calls(&daemon),
        2,
        "D3-LIVE: a worktree.state with the same heads does not ask again"
    );
}

#[test]
fn a_worktree_state_with_a_new_head_asks_again() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_graph("tienda");

    // feat-pagos goes from 3 to 4 commits ahead.
    worktree_state(&daemon, REPO_SEQ + 1, 4, 2);
    let _ = h.wait_graph("tienda");
    assert_eq!(
        graph_calls(&daemon),
        2,
        "D3-LIVE: a (head, divergence) that changed marks the graph stale"
    );
}

#[test]
fn a_worktree_state_that_only_changes_files_does_not_ask_again() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_graph("tienda");

    // The same heads and divergence, one more unstaged file: nothing about the lanes changed.
    worktree_state(&daemon, REPO_SEQ + 1, 3, 3);
    h.wait_worktrees("tienda");
    // The spacing is 1 s: a request marked stale now would be sent within this wait.
    assert!(
        !daemon.wait_for_calls(GRAPH, 2, Duration::from_millis(1_500)),
        "D3-LIVE: the lanes did not change, so the graph is not asked again"
    );
}

#[test]
fn queries_are_spaced_by_twice_the_last_duration() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_delay_ms = 700);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(
        daemon.wait_for_calls(GRAPH, 1, DEADLINE),
        "the first graph.lanes was never sent"
    );
    let first_sent = Instant::now();
    let _ = h.wait_graph("tienda");

    git_event(&daemon, REPO_SEQ + 1);
    assert!(
        daemon.wait_for_calls(GRAPH, 2, DEADLINE),
        "D3-LIVE: the stale graph is asked again"
    );
    // The last query took 700 ms, so the next one waits max(1 s, 1.4 s) from the last send.
    // `first_sent` is read after the first send, so the real gap is at least as long.
    let gap = first_sent.elapsed();
    assert!(
        gap >= Duration::from_millis(1_300),
        "D3-LIVE: the second query went out {gap:?} after the first; the spacing is 2 x 700 ms"
    );
}

#[test]
fn the_spacing_is_at_least_one_second() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    let first_sent = Instant::now();
    let _ = h.wait_graph("tienda");
    git_event(&daemon, REPO_SEQ + 1);
    assert!(daemon.wait_for_calls(GRAPH, 2, DEADLINE));
    let gap = first_sent.elapsed();
    assert!(
        gap >= Duration::from_millis(900),
        "D3-LIVE: a 30 ms query still waits 1 s before the next: {gap:?}"
    );
}

// ---------------------------------------------------------------- D17-FAIL

#[test]
fn an_engine_without_graph_lanes_reports_unsupported_once() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_hidden = true);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert_eq!(
        h.wait_unavailable("tienda"),
        GraphUnavailableReason::Unsupported,
        "D17-FAIL: hello.methods without graph.lanes says unsupported"
    );
    git_event(&daemon, REPO_SEQ + 1);
    worktree_state(&daemon, REPO_SEQ + 2, 4, 2);
    h.wait_worktrees("tienda");
    assert_eq!(
        h.unavailable_count(),
        1,
        "D17-FAIL: unsupported is said once per opened repo, not on every event"
    );
    assert_eq!(
        graph_calls(&daemon),
        0,
        "D17-FAIL: a method the daemon does not list is never called"
    );
}

#[test]
fn query_busy_retries_after_the_spacing() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_busy_first = 1);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    let first_sent = Instant::now();
    let _ = h.wait_graph("tienda");
    assert_eq!(
        graph_calls(&daemon),
        2,
        "D17-FAIL: query-busy marks the graph stale and the supervisor asks again"
    );
    assert!(
        first_sent.elapsed() >= Duration::from_millis(900),
        "D17-FAIL: the retry waits for the spacing, it does not hammer the daemon"
    );
    assert_eq!(
        h.unavailable_count(),
        0,
        "D17-FAIL: query-busy never reaches the webview"
    );
}

#[test]
fn a_refused_query_reports_failed_and_keeps_the_last_graph() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_graph("tienda");

    daemon.script(|s| s.graph_refuse = Some(code::INVALID_PARAMS));
    git_event(&daemon, REPO_SEQ + 1);
    assert_eq!(
        h.wait_unavailable("tienda"),
        GraphUnavailableReason::Failed,
        "D17-FAIL: a refusal (invalid params, not found, ...) says failed"
    );
    assert_eq!(
        h.graph_messages().len(),
        1,
        "D17-FAIL: the failure sends no empty graph that would replace the last one"
    );
}

#[test]
fn an_unknown_repo_is_a_failed_graph_not_a_crash() {
    let daemon = tienda_daemon();
    daemon.script(|s| {
        s.graph.remove("tienda");
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert_eq!(
        h.wait_unavailable("tienda"),
        GraphUnavailableReason::Failed,
        "D17-FAIL: NOT_FOUND from the daemon says failed, never the daemon's text"
    );
}

// ---------------------------------------------------------------- D3-ASYNC

#[test]
fn an_async_reply_read_while_waiting_for_events_reaches_the_webview() {
    let daemon = tienda_daemon();
    // The answer comes while the supervisor waits for notifications, not inside a `call`.
    daemon.script(|s| s.graph_delay_ms = 300);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    // Notifications flowing meanwhile: the stream is not blocked by the query.
    worktree_state(&daemon, REPO_SEQ + 1, 3, 5);
    h.wait_worktrees("tienda");
    let (graph, _) = h.wait_graph("tienda");
    assert_eq!(
        graph.computed_utc_ms, GRAPH_COMPUTED,
        "the reply of the request in flight reaches the webview (never stuck on loading)"
    );
}

#[test]
fn an_unknown_reply_id_is_dropped() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_delay_ms = 300);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    // A daemon answering a request nobody made, with a graph that would be easy to spot.
    let spoof = GraphLanes {
        computed_at_utc_ms: 42,
        ..tienda_graph()
    };
    assert!(
        daemon.push_response(9_999_999, spoof),
        "the fake could not push"
    );
    let (graph, _) = h.wait_graph("tienda");
    assert_eq!(
        graph.computed_utc_ms, GRAPH_COMPUTED,
        "L-01: the reply of the request in flight is the one delivered"
    );
    assert!(
        h.graph_messages()
            .iter()
            .all(|(_, g)| g.computed_utc_ms == GRAPH_COMPUTED),
        "L-01: a reply whose id nobody asked for is dropped, never shown"
    );
}

// ---------------------------------------------------------------- D3-REPO

#[test]
fn a_reply_for_a_repo_no_longer_open_is_dropped_and_the_query_is_cancelled() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_delay_ms = 400);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    h.open("notas");

    assert!(
        daemon.wait_for_calls(CANCEL, 1, DEADLINE),
        "D3-REPO: changing the repo cancels the query in flight"
    );
    let asked = daemon.graph_request_ids();
    let cancels = daemon.calls_of(CANCEL);
    let cancelled_id = match &asked[0] {
        Id::Num(n) => json!(n),
        Id::Str(s) => json!(s),
    };
    assert_eq!(
        cancels,
        [json!({"request": cancelled_id})],
        "D3-REPO: query.cancel names the request of the first repo's graph"
    );
    let (_, _) = h.wait_graph("notas");
    assert!(
        h.graph_messages().iter().all(|(repo, _)| *repo != "tienda"),
        "D3-REPO: the reply of a repo that is no longer open never reaches the webview"
    );
}

#[test]
fn after_a_repo_change_the_next_graph_query_waits_for_query_cancelled() {
    let daemon = tienda_daemon();
    daemon.script(|s| s.graph_delay_ms = 400);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(daemon.wait_for_calls(GRAPH, 1, DEADLINE));
    h.open("notas");

    let (graph, _) = h.wait_graph("notas");
    assert_eq!(
        graph.computed_utc_ms,
        GRAPH_COMPUTED + 1,
        "the graph of notas"
    );
    assert_eq!(
        daemon.busy_hits(),
        0,
        "M-01: the daemon frees the slot only when the cancelled query answers, so the query of \
         the new repo must wait for that answer and never meet query-busy"
    );
    let asked = daemon.calls_of(GRAPH);
    assert_eq!(
        asked,
        [json!({"repo_id": "tienda"}), json!({"repo_id": "notas"})],
        "M-01: one query per repo, the second only after the first answered"
    );
}

// ---------------------------------------------------------------- D3-SAFE

#[test]
fn a_restricted_session_never_asks_graph_lanes() {
    // Control: the same script on a full session does ask.
    let control = tienda_daemon();
    let mut hc = Harness::live(&control);
    hc.open("tienda");
    let _ = hc.wait_graph("tienda");
    drop(hc);

    let daemon = tienda_daemon();
    daemon.script(|s| s.requester = Some(ConnectionRequester::Unverified));
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    // A `watch` after `open` is attended after it: by then the `open` was handled.
    let (tx, replay) = mpsc::channel();
    assert!(
        h.handle
            .as_ref()
            .unwrap()
            .send(HubCmd::Watch(Sink::Collect(tx))),
        "supervisor gone"
    );
    replay
        .recv_timeout(DEADLINE)
        .expect("the watch after Open is attended");
    while let Ok(m) = h.rx.try_recv() {
        h.seen.push(m);
    }
    assert_eq!(
        graph_calls(&daemon),
        0,
        "D3-SAFE: a restricted session never asks graph.lanes"
    );
    assert!(
        !h.seen.iter().any(|m| matches!(
            m,
            EngineMessage::Graph { .. } | EngineMessage::GraphUnavailable { .. }
        )),
        "D3-SAFE: no graph data reaches the webview of a restricted session"
    );
}

#[test]
fn a_graph_run_never_sends_writes_or_reserved_methods() {
    let daemon = tienda_daemon();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_graph("tienda");
    git_event(&daemon, REPO_SEQ + 1);
    let _ = h.wait_graph("tienda");
    h.open("notas");
    let _ = h.wait_graph("notas");

    let calls = daemon.calls();
    assert_read_only(&calls);
    let spec = methods::spec(GRAPH).expect("the contract knows graph.lanes");
    assert!(
        !spec.reserved && spec.writes == methods::RepoWrite::None,
        "D3-READONLY: graph.lanes is a read"
    );
    for (method, params) in calls.iter().filter(|(m, _)| m == GRAPH) {
        assert_eq!(
            params
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["repo_id".to_owned()]),
            "D3-READONLY: {method} carries only repo_id: {params}"
        );
    }
}
