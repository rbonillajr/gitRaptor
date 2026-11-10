//! T004 of DS-US-CKP-022: the channel thread submits `graph.lanes`, keeps `ticket -> id`, takes
//! the answer from a frame or from a reply a call read, announces whether the engine offers the
//! method, and loses what was in flight when the link drops. A fake link stands in for the
//! daemon; every wait is on a message of the thread. Mounted next to `mod diff;` by the expert.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};

use gitraptor_api::graph::GraphLanes;
use gitraptor_api::methods::{GRAPH_LANES, QUERY_BUSY, QUERY_CANCEL, QUERY_CANCELLED};
use gitraptor_api::rpc::ErrorObject;
use gitraptor_api::scope::{ScopeSubscribeParams, ScopeSubscribeResult};

use super::super::prediction_tests::{global, repo};
use super::*;
use crate::model::GraphFailure;
use crate::queue::{Inbox, inbox};

const WAIT: Duration = Duration::from_secs(10);
const FIRST_ID: u64 = 700;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Senders = Arc<Mutex<Vec<Sender<Vec<u8>>>>>;

struct Fake {
    calls: Calls,
    next_id: u64,
    stream: Receiver<Vec<u8>>,
    /// Whether the engine has `graph.lanes` in its handshake.
    graph: bool,
    /// What the daemon answers once `query.cancel` is called: the answer of the cancelled
    /// request, read by that call while it waited for its own.
    on_cancel: Vec<Response>,
    read: VecDeque<Response>,
}

impl Link for Fake {
    fn requester(&self) -> Option<gitraptor_api::scope::ConnectionRequester> {
        None
    }

    fn offers(&self, method: &str) -> bool {
        self.graph && method == GRAPH_LANES
    }

    fn submit(&mut self, method: &str, params: Value) -> Result<u64, LinkError> {
        self.calls.lock().unwrap().push((method.to_owned(), params));
        self.next_id += 1;
        Ok(self.next_id)
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
                serde_json::json!({"repo_id": "r1", "worktree": {"untrusted": "/w"}})
            }
            QUERY_CANCEL => {
                self.read.extend(self.on_cancel.drain(..));
                serde_json::json!({"cancelled": true})
            }
            _ => return Err(LinkError::Refused),
        })
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError> {
        if let Some(response) = self.read.pop_front() {
            return Ok(Some(Incoming::Reply {
                recv_ns: monotonic_ns(),
                response,
            }));
        }
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

/// One link per `connect`; the test keeps the stream's sender to feed frames or drop it.
struct Script {
    calls: Calls,
    senders: Senders,
    graph: bool,
    replies: Vec<Response>,
}

impl Connector for Script {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        let (tx, rx) = channel();
        self.senders.lock().unwrap().push(tx);
        Ok(Box::new(Fake {
            calls: Arc::clone(&self.calls),
            next_id: FIRST_ID,
            stream: rx,
            graph: self.graph,
            on_cancel: std::mem::take(&mut self.replies),
            read: VecDeque::new(),
        }))
    }
}

struct Run {
    inbox: Inbox,
    thread: Option<ClientThread>,
    calls: Calls,
    senders: Senders,
    seen: Vec<Msg>,
}

fn run_with(graph: bool, replies: Vec<Response>) -> Run {
    let calls = Calls::default();
    let senders = Senders::default();
    let (inbox, _input, out) = inbox();
    let thread = spawn(
        Script {
            calls: Arc::clone(&calls),
            senders: Arc::clone(&senders),
            graph,
            replies,
        },
        Some(PathBuf::from("/w")),
        out,
    );
    Run {
        inbox,
        thread: Some(thread),
        calls,
        senders,
        seen: Vec::new(),
    }
}

fn run(replies: Vec<Response>) -> Run {
    run_with(true, replies)
}

impl Run {
    fn send(&self, cmd: LinkCmd) {
        self.thread.as_ref().unwrap().cmds.send(cmd).unwrap();
    }

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

    fn calls_of(&self, method: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// The answer of `ticket`, waiting for it.
    fn answer(&mut self, ticket: u64) -> Result<Box<GraphLanes>, GraphFailure> {
        let hit = |m: &Msg| matches!(m, Msg::Engine(Stamped { msg: EngineMsg::Graph { ticket: t, .. }, .. }) if *t == ticket);
        assert!(self.until(hit), "no answer for ticket {ticket}");
        match self.seen.pop() {
            Some(Msg::Engine(Stamped {
                msg: EngineMsg::Graph { result, .. },
                ..
            })) => result,
            other => panic!("{other:?}"),
        }
    }

    /// Waits until the thread has submitted `n` graph requests.
    fn submitted(&mut self, n: usize) {
        let deadline = std::time::Instant::now() + WAIT;
        while self.calls_of(GRAPH_LANES).len() < n {
            assert!(std::time::Instant::now() < deadline, "nothing submitted");
            self.inbox.wait(Duration::from_millis(20));
        }
    }

    fn frame(&self, response: &Response) {
        let bytes = serde_json::to_vec(response).unwrap();
        self.senders.lock().unwrap()[0].send(bytes).unwrap();
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread.shutdown();
        }
    }
}

fn ask(ticket: u64) -> LinkCmd {
    LinkCmd::Graph {
        ticket,
        repo_id: "r1".into(),
    }
}

/// The answer of an engine with a ready base and no lanes, in the wire of the contract.
fn empty_lanes() -> Value {
    serde_json::json!({
        "computed_at_utc_ms": 7,
        "base": {"ready": {"name": {"untrusted": "main"}}},
        "lanes_total": 0,
        "lanes": []
    })
}

fn failed(id: u64, code: i64) -> Response {
    Response::err(Some(Id::Num(id)), ErrorObject::new(code, "x"))
}

#[test]
fn a_graph_reply_reaches_update_by_ticket() {
    let mut run = run(Vec::new());
    run.live();
    run.send(ask(5));
    run.submitted(1);
    let params = run.calls_of(GRAPH_LANES).remove(0);
    assert_eq!(
        params,
        serde_json::json!({"repo_id": "r1"}),
        "graph.lanes takes only the repo id"
    );
    // A response of an id nobody asked is dropped; the asked one is the answer to ticket 5.
    run.frame(&Response::ok(Id::Num(1), empty_lanes()));
    run.frame(&Response::ok(Id::Num(FIRST_ID + 1), empty_lanes()));
    let lanes = run.answer(5).expect("the reply is delivered as Ok");
    assert_eq!(lanes.computed_at_utc_ms, 7);

    // A second request is answered by its own ticket, not by the previous one.
    run.send(ask(6));
    run.submitted(2);
    let mut other = empty_lanes();
    other["computed_at_utc_ms"] = serde_json::json!(8);
    run.frame(&Response::ok(Id::Num(FIRST_ID + 2), other));
    assert_eq!(run.answer(6).unwrap().computed_at_utc_ms, 8);
}

#[test]
fn busy_and_cancelled_map_to_graph_failure() {
    let mut run = run(vec![failed(FIRST_ID + 3, QUERY_CANCELLED.code)]);
    run.live();

    run.send(ask(1));
    run.submitted(1);
    run.frame(&failed(FIRST_ID + 1, QUERY_BUSY.code));
    assert_eq!(run.answer(1).unwrap_err(), GraphFailure::Busy);

    // Any other refusal keeps its own variant.
    run.send(ask(2));
    run.submitted(2);
    run.frame(&failed(FIRST_ID + 2, -32603));
    assert_eq!(run.answer(2).unwrap_err(), GraphFailure::Refused);

    // query-cancelled: the thread calls query.cancel and takes the reply that call read.
    run.send(ask(3));
    run.submitted(3);
    run.send(LinkCmd::CancelQuery { ticket: 3 });
    // A ticket that is not in flight cancels nothing.
    run.send(LinkCmd::CancelQuery { ticket: 99 });
    assert_eq!(run.answer(3).unwrap_err(), GraphFailure::Cancelled);
    assert_eq!(
        run.calls_of(QUERY_CANCEL),
        vec![serde_json::json!({"request": FIRST_ID + 3})]
    );
}

#[test]
fn a_lost_connection_answers_lost() {
    let mut run = run(Vec::new());
    run.live();
    run.send(ask(3));
    run.send(ask(4));
    run.submitted(2);
    // The first link goes down: each request in flight is answered Lost, once.
    run.senders.lock().unwrap().remove(0);
    assert_eq!(run.answer(3).unwrap_err(), GraphFailure::Lost);
    assert_eq!(run.answer(4).unwrap_err(), GraphFailure::Lost);
}

#[test]
fn an_engine_without_graph_lanes_is_announced() {
    let mut without = run_with(false, Vec::new());
    without.live();
    assert!(
        without
            .seen
            .iter()
            .any(|m| matches!(m, Msg::Conn(ConnEvent::Graph(false)))),
        "an engine that does not offer graph.lanes must be announced as ConnEvent::Graph(false)"
    );
    assert!(
        !without
            .seen
            .iter()
            .any(|m| matches!(m, Msg::Conn(ConnEvent::Graph(true)))),
        "an engine without the method is never announced as having it"
    );

    let mut with = run_with(true, Vec::new());
    with.live();
    assert!(
        with.seen
            .iter()
            .any(|m| matches!(m, Msg::Conn(ConnEvent::Graph(true)))),
        "an engine that offers graph.lanes is announced as ConnEvent::Graph(true)"
    );
}
