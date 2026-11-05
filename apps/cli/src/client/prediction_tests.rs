//! T003 of DS-US-CKP-006: the channel thread tells the capability, runs `prediction.list` and
//! hands back the page. A fake link stands in for the daemon; every wait is on a message of the
//! thread, with a deadline.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};

use gitraptor_api::Untrusted;
use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, DaemonView, EngineStateView, EngineView, RepoStateView,
    RepoView,
};
use gitraptor_api::methods::{self, CAP_PREDICTION_PAIRS, PREDICTION_LIST};
use gitraptor_api::prediction::{ListPredictionParams, PredictionPage};
use gitraptor_api::scope::{
    AutostartView, GlobalSnapshot, RepoSnapshot, ScopeSnapshot, ScopeSubscribeParams,
    ScopeSubscribeResult,
};

use super::*;
use crate::queue::{Inbox, inbox};

const REPO: &str = "r1";
const WAIT: Duration = Duration::from_secs(10);

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake {
    capability: bool,
    calls: Calls,
    stream: Receiver<Vec<u8>>,
}

impl Link for Fake {
    fn requester(&self) -> Option<gitraptor_api::scope::ConnectionRequester> {
        None
    }

    fn has(&self, capability: &str) -> bool {
        self.capability && capability == CAP_PREDICTION_PAIRS.name
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.calls
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        Ok(match method {
            methods::SCOPE_SNAPSHOT => {
                let params: ScopeSnapshotParams = serde_json::from_value(params).unwrap();
                serde_json::to_value(match params.scope {
                    Scope::Global => global(),
                    Scope::Repo { .. } => repo(),
                })
                .unwrap()
            }
            methods::SCOPE_SUBSCRIBE => {
                let params: ScopeSubscribeParams = serde_json::from_value(params).unwrap();
                serde_json::to_value(ScopeSubscribeResult {
                    subscription: 1,
                    scope: params.scope,
                    from_seq: params.from_seq.unwrap_or(1),
                })
                .unwrap()
            }
            methods::REPO_LOCATE => {
                serde_json::json!({"repo_id": REPO, "worktree": {"untrusted": "/w"}})
            }
            PREDICTION_LIST => {
                let params: ListPredictionParams = serde_json::from_value(params).unwrap();
                match params.pair_ids.as_deref() {
                    Some([id]) if id == "refused" => return Err(LinkError::Refused),
                    Some([id]) if id == "lost" => return Err(LinkError::Lost),
                    Some([id]) => serde_json::to_value(PredictionPage {
                        limits: Vec::new(),
                        pairs: Vec::new(),
                        next_cursor: Some(id.clone()),
                    })
                    .unwrap(),
                    _ => return Err(LinkError::Refused),
                }
            }
            _ => return Err(LinkError::Refused),
        })
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError> {
        match self.stream.recv_timeout(timeout) {
            Ok(bytes) => Ok(Some(Incoming::Frame {
                recv_ns: monotonic_ns(),
                bytes,
            })),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(LinkError::Lost),
        }
    }
}

fn global() -> ScopeSnapshot {
    ScopeSnapshot::Global(GlobalSnapshot {
        run_id: "run".into(),
        scope_seq: 0,
        engine: EngineView {
            state: EngineStateView::Observing,
            git_version: None,
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

fn repo() -> ScopeSnapshot {
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
            tier: None,
            checked_utc_ms: None,
            kept_temps: None,
            prediction: None,
        },
    })
}

/// Hands out the scripted outcome of each connection: `Err` is a failed connection.
struct Script {
    connections: VecDeque<Result<bool, LinkError>>,
    calls: Calls,
    /// Keeps the streams open for as long as the test runs.
    streams: Vec<Sender<Vec<u8>>>,
    opened: Arc<Mutex<u32>>,
}

impl Connector for Script {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        let capability = self.connections.pop_front().ok_or(LinkError::Lost)??;
        *self.opened.lock().unwrap() += 1;
        let (tx, rx) = channel();
        self.streams.push(tx);
        Ok(Box::new(Fake {
            capability,
            calls: Arc::clone(&self.calls),
            stream: rx,
        }))
    }
}

struct Run {
    inbox: Inbox,
    thread: Option<ClientThread>,
    calls: Calls,
    opened: Arc<Mutex<u32>>,
    seen: Vec<Msg>,
}

fn run(connections: Vec<Result<bool, LinkError>>) -> Run {
    let calls = Calls::default();
    let opened = Arc::new(Mutex::new(0));
    let (inbox, _input, out) = inbox();
    let thread = spawn(
        Script {
            connections: connections.into(),
            calls: Arc::clone(&calls),
            streams: Vec::new(),
            opened: Arc::clone(&opened),
        },
        Some(PathBuf::from("/w")),
        out,
    );
    Run {
        inbox,
        thread: Some(thread),
        calls,
        opened,
        seen: Vec::new(),
    }
}

impl Run {
    fn send(&self, cmd: LinkCmd) {
        self.thread.as_ref().unwrap().cmds.send(cmd).unwrap();
    }

    /// Reads the thread's messages until one satisfies `done`; `false` at the deadline.
    fn until(&mut self, done: impl Fn(&Msg) -> bool) -> bool {
        let deadline = std::time::Instant::now() + WAIT;
        while std::time::Instant::now() < deadline {
            while let Some(msg) = self.inbox.next_engine() {
                let hit = done(&msg);
                self.seen.push(msg);
                if hit {
                    return true;
                }
            }
            self.inbox.wait(Duration::from_millis(50));
        }
        false
    }

    fn live(&mut self) {
        assert!(
            self.until(|m| matches!(m, Msg::Conn(ConnEvent::State(ConnState::Live)))),
            "the session never went live"
        );
    }

    fn pages(&self) -> Vec<Option<String>> {
        self.seen
            .iter()
            .filter_map(|m| match m {
                Msg::Engine(Stamped {
                    msg: EngineMsg::PredictionPage { page, .. },
                    ..
                }) => Some(page.next_cursor.clone()),
                _ => None,
            })
            .collect()
    }

    fn list_calls(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m == PREDICTION_LIST)
            .count()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread.shutdown();
        }
    }
}

fn list(id: &str) -> LinkCmd {
    LinkCmd::PredictionList {
        repo_id: REPO.into(),
        pair_ids: vec![id.into()],
        cursor: None,
    }
}

#[test]
fn the_channel_tells_whether_the_connection_has_prediction_pairs() {
    for capability in [true, false] {
        let mut run = run(vec![Ok(capability)]);
        run.live();
        assert!(
            run.seen
                .iter()
                .any(|m| matches!(m, Msg::Conn(ConnEvent::Prediction(has)) if *has == capability)),
            "ConnEvent::Prediction({capability}) is sent before the session is live: {:?}",
            run.seen
        );
    }
}

#[test]
fn prediction_list_returns_the_page_and_calls_the_engine_with_its_params() {
    let mut run = run(vec![Ok(true)]);
    run.live();
    run.send(LinkCmd::PredictionList {
        repo_id: REPO.into(),
        pair_ids: vec!["p1".into()],
        cursor: None,
    });
    assert!(
        run.until(|m| matches!(
            m,
            Msg::Engine(Stamped { msg: EngineMsg::PredictionPage { repo_id, page }, .. })
                if repo_id == REPO && page.next_cursor.as_deref() == Some("p1")
        )),
        "Cmd::PredictionList answers with EngineMsg::PredictionPage"
    );
    let calls = run.calls.lock().unwrap();
    let (_, params) = calls
        .iter()
        .find(|(m, _)| m == PREDICTION_LIST)
        .expect("prediction.list was called");
    let params: ListPredictionParams = serde_json::from_value(params.clone()).unwrap();
    assert_eq!(params.repo_id, REPO);
    assert_eq!(params.pair_ids, Some(vec!["p1".to_owned()]));
}

#[test]
fn a_refused_list_is_dropped_without_notice() {
    let mut run = run(vec![Ok(true), Ok(true)]);
    run.live();
    run.send(list("refused"));
    run.send(list("good"));
    assert!(
        run.until(|m| matches!(
            m,
            Msg::Engine(Stamped {
                msg: EngineMsg::PredictionPage { .. },
                ..
            })
        )),
        "the next list still answers"
    );
    assert_eq!(
        run.pages(),
        [Some("good".to_owned())],
        "nothing for the refused one"
    );
    assert_eq!(run.list_calls(), 2);
    assert!(
        !run.seen.iter().any(|m| matches!(
            m,
            Msg::Conn(ConnEvent::State(ConnState::Reconnecting { .. }))
        )),
        "a refusal does not reconnect"
    );
}

#[test]
fn a_transport_error_reconnects() {
    let mut run = run(vec![Ok(true), Ok(true)]);
    run.live();
    run.send(list("lost"));
    assert!(
        run.until(|m| matches!(
            m,
            Msg::Conn(ConnEvent::State(ConnState::Reconnecting { .. }))
        )),
        "a transport error on prediction.list ends the session and reconnects"
    );
    assert!(
        run.until(|m| matches!(m, Msg::Conn(ConnEvent::State(ConnState::Live)))),
        "and the new session goes live"
    );
    assert_eq!(*run.opened.lock().unwrap(), 2);
}

#[test]
fn outside_a_session_the_order_is_ignored() {
    let mut run = run(vec![Err(LinkError::EngineUnavailable), Ok(true)]);
    // The first connection fails: the thread is between sessions.
    run.send(list("early"));
    run.live();
    run.send(list("late"));
    assert!(
        run.until(|m| matches!(
            m,
            Msg::Engine(Stamped {
                msg: EngineMsg::PredictionPage { .. },
                ..
            })
        )),
        "the order sent inside the session answers"
    );
    assert_eq!(
        run.pages(),
        [Some("late".to_owned())],
        "the order sent between sessions was ignored"
    );
    assert_eq!(run.list_calls(), 1, "and never reached the engine");
}
