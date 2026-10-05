//! Event publication and per-connection output (ADR-GRP-005 § 5, SEC-08).
//!
//! The bus assigns the daemon-wide sequence, keeps a bounded replay buffer
//! and the engine view that snapshots read, all under one lock: a snapshot
//! at sequence N and a subscription from N + 1 neither miss nor repeat an
//! event (DEP-CKP-6). Publishing never blocks: each connection has a
//! bounded outbox, and one that overflows is cleared, gets an
//! `events.resync` and is disconnected once that is written.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    EngineView, EventNotification, RepoView, ResyncNotification, ResyncReason,
};
use gitraptor_api::methods::{NOTIFY_EVENT, NOTIFY_RESYNC};
use gitraptor_api::rpc::Notification;
use gitraptor_api::{Timings, clock};
use serde::Serialize;

use crate::daemon::now_ms;
use crate::observe::DivergenceInputs;

/// Lines waiting to be written to one connection.
#[derive(Debug)]
pub struct Outbox {
    state: Mutex<OutState>,
    ready: Condvar,
    capacity: usize,
}

#[derive(Debug, Default)]
struct OutState {
    queue: VecDeque<String>,
    /// Write what is queued, then close.
    closing: bool,
    /// Overflowed: dropped by the bus, closing after the resync.
    lagged: bool,
}

impl Outbox {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(OutState::default()),
            ready: Condvar::new(),
            capacity,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, OutState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queues one message. Returns `false` if the outbox overflowed (it is
    /// now lagged) or is closing.
    pub fn push(&self, message: &impl Serialize) -> bool {
        let Ok(line) = serde_json::to_string(message) else {
            return true;
        };
        let mut state = self.lock();
        if state.closing {
            return false;
        }
        if state.queue.len() >= self.capacity {
            state.queue.clear();
            let resync = Notification::new(
                NOTIFY_RESYNC,
                ResyncNotification {
                    reason: ResyncReason::SlowConsumer,
                },
            );
            if let Ok(line) = serde_json::to_string(&resync) {
                state.queue.push_back(line);
            }
            state.closing = true;
            state.lagged = true;
            self.ready.notify_all();
            return false;
        }
        state.queue.push_back(line);
        self.ready.notify_all();
        true
    }

    /// Writes what is queued, then closes.
    pub fn close(&self) {
        self.lock().closing = true;
        self.ready.notify_all();
    }

    pub fn is_lagged(&self) -> bool {
        self.lock().lagged
    }

    /// Next line to write, or `None` once closing with nothing left.
    pub fn next(&self) -> Option<String> {
        let mut state = self.lock();
        loop {
            if let Some(line) = state.queue.pop_front() {
                return Some(line);
            }
            if state.closing {
                return None;
            }
            state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// What snapshots read: the engine view and the observed repos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineShared {
    pub engine: EngineView,
    pub repos: Vec<RepoView>,
    /// What `engine.snapshot` needs to count each repo's ahead/behind
    /// again, by repo id (US-GRP-012). Updated with `repos`.
    pub divergence: BTreeMap<String, DivergenceInputs>,
}

struct Subscriber {
    outbox: Arc<Outbox>,
    id: u32,
    /// An MCP connection: only the kinds of the MCP allowlist (no audit).
    mcp: bool,
}

struct BusInner {
    seq: u64,
    replay: VecDeque<Event>,
    engine: EngineShared,
    subscribers: Vec<Subscriber>,
    next_batch: u64,
}

/// The event bus of one daemon run.
pub struct EventBus {
    run_id: String,
    replay_capacity: usize,
    inner: Mutex<BusInner>,
}

/// Outcome of a subscription request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subscribed {
    /// Events from this sequence on will be delivered.
    From(u64),
    /// The client must resync (already queued on its outbox).
    Resync,
}

impl EventBus {
    pub fn new(run_id: String, engine: EngineShared, replay_capacity: usize) -> Self {
        Self {
            run_id,
            replay_capacity,
            inner: Mutex::new(BusInner {
                seq: 0,
                replay: VecDeque::new(),
                engine,
                subscribers: Vec::new(),
                next_batch: 1,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BusInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The engine view and the sequence it reflects, read atomically.
    pub fn snapshot(&self) -> (u64, EngineShared) {
        let inner = self.lock();
        (inner.seq, inner.engine.clone())
    }

    /// A new batch id for change events of one debounce window.
    pub fn next_batch(&self) -> u64 {
        let mut inner = self.lock();
        let id = inner.next_batch;
        inner.next_batch += 1;
        id
    }

    /// Publishes an event of `kind` (non-change kinds have no timings; change
    /// kinds must carry them). `update` may change the engine view in the
    /// same critical section. Returns the sequence assigned.
    pub fn publish(
        &self,
        kind: &str,
        data: impl Serialize,
        timings: Option<Timings>,
        update: impl FnOnce(&mut EngineShared),
    ) -> u64 {
        let mut inner = self.lock();
        update(&mut inner.engine);
        inner.seq += 1;
        let version = gitraptor_api::event::kind(kind).map_or(0, |k| k.version);
        let timings = timings.map(|mut t| {
            t.t_published = clock::monotonic_ns();
            t
        });
        let event = Event {
            seq: inner.seq,
            kind: kind.to_owned(),
            version,
            wall_ms: now_ms(),
            timings,
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        };
        debug_assert!(event.is_well_formed(), "malformed event {kind}");
        inner.subscribers.retain(|sub| deliver(sub, &event));
        inner.replay.push_back(event);
        while inner.replay.len() > self.replay_capacity {
            inner.replay.pop_front();
        }
        inner.seq
    }

    /// Subscribes `outbox` as subscription `id`. With `from_seq`, replays the
    /// buffered events from there; a different `run_id` or a sequence that
    /// left the buffer queues an `events.resync` instead.
    pub fn subscribe(
        &self,
        outbox: &Arc<Outbox>,
        id: u32,
        from_seq: Option<u64>,
        run_id: Option<&str>,
        mcp: bool,
    ) -> Subscribed {
        let mut inner = self.lock();
        let resync = |reason| {
            outbox.push(&Notification::new(
                NOTIFY_RESYNC,
                ResyncNotification { reason },
            ));
            Subscribed::Resync
        };
        if run_id.is_some_and(|r| r != self.run_id) {
            return resync(ResyncReason::DaemonRestarted);
        }
        let next = inner.seq + 1;
        let from = from_seq.unwrap_or(next).min(next);
        if from < next {
            let oldest = inner.replay.front().map_or(next, |e| e.seq);
            if from < oldest {
                return resync(ResyncReason::ReplayUnavailable);
            }
        }
        let sub = Subscriber {
            outbox: Arc::clone(outbox),
            id,
            mcp,
        };
        for event in inner.replay.iter().filter(|e| e.seq >= from) {
            if !deliver(&sub, event) {
                return Subscribed::Resync;
            }
        }
        inner.subscribers.push(sub);
        Subscribed::From(from)
    }

    pub fn unsubscribe(&self, outbox: &Arc<Outbox>, id: u32) -> bool {
        let mut inner = self.lock();
        let before = inner.subscribers.len();
        inner
            .subscribers
            .retain(|s| !(Arc::ptr_eq(&s.outbox, outbox) && s.id == id));
        inner.subscribers.len() != before
    }

    pub fn drop_outbox(&self, outbox: &Arc<Outbox>) {
        self.lock()
            .subscribers
            .retain(|s| !Arc::ptr_eq(&s.outbox, outbox));
    }

    pub fn subscriber_count(&self) -> usize {
        self.lock().subscribers.len()
    }
}

fn deliver(sub: &Subscriber, event: &Event) -> bool {
    if sub.mcp && !mcp_kind(&event.kind) {
        return true;
    }
    sub.outbox.push(&Notification::new(
        NOTIFY_EVENT,
        EventNotification {
            subscription: sub.id,
            event: event.clone(),
        },
    ))
}

/// Event kinds an MCP connection receives: an allowlist, so a new kind is
/// withheld until it is added here (SEC-12). Not the reserved-command audit,
/// which `audit.list` does not offer to MCP either (SEC-14), nor anything
/// carrying paths (`repo.observation`, `worktree.state`): F-001-05 defines
/// their MCP projection.
fn mcp_kind(kind: &str) -> bool {
    use gitraptor_api::event::{DAEMON_STOPPING, ENGINE_STATE};
    kind == ENGINE_STATE || kind == DAEMON_STOPPING
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::event::{ENGINE_STATE, RESERVED_AUDIT};
    use gitraptor_api::messages::EngineStateView;
    use gitraptor_api::rpc::ServerMessage;

    fn bus(replay: usize) -> EventBus {
        EventBus::new(
            "run-1".into(),
            EngineShared {
                engine: EngineView {
                    state: EngineStateView::NoRepos,
                    git_version: None,
                },
                repos: Vec::new(),
                divergence: BTreeMap::new(),
            },
            replay,
        )
    }

    fn drain(outbox: &Outbox) -> Vec<ServerMessage> {
        outbox.close();
        std::iter::from_fn(|| outbox.next())
            .map(|l| serde_json::from_str(&l).unwrap())
            .collect()
    }

    fn seqs(messages: &[ServerMessage]) -> Vec<u64> {
        messages
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Notification(n) if n.method == NOTIFY_EVENT => {
                    Some(n.params["event"]["seq"].as_u64().unwrap())
                }
                _ => None,
            })
            .collect()
    }

    /// DEP-CKP-6: snapshot at N, subscribe from N + 1, nothing lost or
    /// repeated, in sequence order.
    #[test]
    fn snapshot_then_subscribe_is_gapless_and_ordered() {
        let bus = bus(16);
        bus.publish(RESERVED_AUDIT, 1, None, |_| {});
        let (n, _) = bus.snapshot();
        bus.publish(RESERVED_AUDIT, 2, None, |_| {});
        bus.publish(ENGINE_STATE, 3, None, |e| {
            e.engine.state = EngineStateView::Observing
        });
        let out = Outbox::new(64);
        assert_eq!(
            bus.subscribe(&out, 7, Some(n + 1), Some("run-1"), false),
            Subscribed::From(n + 1)
        );
        bus.publish(RESERVED_AUDIT, 4, None, |_| {});
        assert_eq!(seqs(&drain(&out)), [2, 3, 4]);
        assert_eq!(bus.snapshot().1.engine.state, EngineStateView::Observing);
    }

    #[test]
    fn restarted_daemon_or_lost_replay_asks_for_a_resync() {
        let bus = bus(2);
        for i in 0..5 {
            bus.publish(RESERVED_AUDIT, i, None, |_| {});
        }
        let out = Outbox::new(64);
        assert_eq!(
            bus.subscribe(&out, 1, Some(1), None, false),
            Subscribed::Resync
        );
        let out2 = Outbox::new(64);
        assert_eq!(
            bus.subscribe(&out2, 1, Some(5), Some("run-0"), false),
            Subscribed::Resync
        );
        for out in [out, out2] {
            let msgs = drain(&out);
            assert!(
                matches!(&msgs[0], ServerMessage::Notification(n) if n.method == NOTIFY_RESYNC)
            );
        }
    }

    /// SEC-08: a subscriber that does not read never blocks the producer;
    /// it gets a resync and is dropped, the others keep receiving.
    #[test]
    fn slow_subscriber_is_dropped_without_blocking() {
        let bus = bus(8);
        let slow = Outbox::new(4);
        let fast = Outbox::new(1024);
        bus.subscribe(&slow, 1, None, None, false);
        bus.subscribe(&fast, 1, None, None, false);
        for i in 0..100 {
            bus.publish(RESERVED_AUDIT, i, None, |_| {});
        }
        assert!(slow.is_lagged());
        assert_eq!(bus.subscriber_count(), 1);
        let msgs = drain(&slow);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], ServerMessage::Notification(n) if n.method == NOTIFY_RESYNC));
        assert_eq!(seqs(&drain(&fast)).len(), 100);
    }
}
