//! Event publication and per-connection output (ADR-GRP-005 § 5, SEC-08).
//!
//! The bus assigns the daemon-wide sequence, keeps a bounded replay buffer
//! and the engine view that snapshots read, all under one lock: a snapshot
//! at sequence N and a subscription from N + 1 neither miss nor repeat an
//! event (DEP-CKP-6). Publishing never blocks: each connection has a
//! bounded outbox, and one that overflows is cleared, gets an
//! `events.resync` and is disconnected once that is written.
//!
//! Protocol 5 adds scopes (ADR-CKP-003 § 4 N1, N2): every event also gets a
//! contiguous sequence within its scope (global, or one repo), assigned
//! under the same lock, and a scoped subscription receives only its
//! scope's events with that sequence.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    EngineView, EventNotification, RepoView, ResyncNotification, ResyncReason,
};
use gitraptor_api::methods::{
    NOTIFY_EVENT, NOTIFY_RESYNC, NOTIFY_SCOPE_EVENT, NOTIFY_SCOPE_RESYNC,
};
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeResyncNotification};
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
    /// Negotiated protocol older than 8 (see [`Outbox::set_before_reset`]).
    before_reset: std::sync::atomic::AtomicBool,
    /// Without `scope.activity` (see [`Outbox::set_without_activity`]).
    without_activity: std::sync::atomic::AtomicBool,
    /// Without `events.authorship` (see [`Outbox::set_without_authorship`]).
    without_authorship: std::sync::atomic::AtomicBool,
    /// Without `observation.tiers` (see [`Outbox::set_without_tiers`]).
    without_tiers: std::sync::atomic::AtomicBool,
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
            before_reset: std::sync::atomic::AtomicBool::new(false),
            without_activity: std::sync::atomic::AtomicBool::new(true),
            without_authorship: std::sync::atomic::AtomicBool::new(true),
            without_tiers: std::sync::atomic::AtomicBool::new(true),
        })
    }

    /// The connection negotiated a protocol older than 8: Git events of
    /// kind `reset` (US-TMC-004) are not delivered to it, since it cannot
    /// read them.
    pub fn set_before_reset(&self, before: bool) {
        self.before_reset
            .store(before, std::sync::atomic::Ordering::Relaxed);
    }

    /// The connection lacks `scope.activity` (DEP-CKP-4): `worktree.state`
    /// reaches it without the last activity and fetch, which it cannot read.
    pub fn set_without_activity(&self, without: bool) {
        self.without_activity
            .store(without, std::sync::atomic::Ordering::Relaxed);
    }

    /// The connection lacks `observation.tiers` (TS-GRP-006): `repo.tier`
    /// never reaches it.
    pub fn set_without_tiers(&self, without: bool) {
        self.without_tiers
            .store(without, std::sync::atomic::Ordering::Relaxed);
    }

    /// The connection lacks `events.authorship` (US-GRD-019): `git.event`
    /// reaches it without the declared authorship nor the trailer check of
    /// the hint, so no names or emails (`raptor-mcp`).
    pub fn set_without_authorship(&self, without: bool) {
        self.without_authorship
            .store(without, std::sync::atomic::Ordering::Relaxed);
    }

    /// The event in the shapes this connection reads.
    fn shape<'e>(&self, event: &'e Event) -> std::borrow::Cow<'e, Event> {
        if event.kind == gitraptor_api::event::GIT_EVENT {
            return self.shape_git_event(event);
        }
        if !self
            .without_activity
            .load(std::sync::atomic::Ordering::Relaxed)
            || event.kind != gitraptor_api::event::WORKTREE_STATE
        {
            return std::borrow::Cow::Borrowed(event);
        }
        let Ok(mut data) = serde_json::from_value::<gitraptor_api::messages::WorktreeStateData>(
            event.data.clone(),
        ) else {
            return std::borrow::Cow::Borrowed(event);
        };
        data.without_activity();
        let mut event = event.clone();
        event.data = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
        std::borrow::Cow::Owned(event)
    }

    fn shape_git_event<'e>(&self, event: &'e Event) -> std::borrow::Cow<'e, Event> {
        if !self
            .without_authorship
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return std::borrow::Cow::Borrowed(event);
        }
        let Ok(view) =
            serde_json::from_value::<gitraptor_api::messages::GitEventView>(event.data.clone())
        else {
            return std::borrow::Cow::Borrowed(event);
        };
        if view.authorship.is_none() && view.inferred.as_ref().is_none_or(|h| h.trailer.is_none()) {
            return std::borrow::Cow::Borrowed(event);
        }
        let mut event = event.clone();
        event.data = serde_json::to_value(view.without_authorship()).unwrap_or_default();
        std::borrow::Cow::Owned(event)
    }

    fn skips(&self, event: &Event) -> bool {
        if event.kind == gitraptor_api::event::REPO_TIER {
            return self
                .without_tiers
                .load(std::sync::atomic::Ordering::Relaxed);
        }
        self.before_reset.load(std::sync::atomic::Ordering::Relaxed)
            && event.kind == gitraptor_api::event::GIT_EVENT
            && event.data.get("kind").and_then(|k| k.as_str()) == Some("reset")
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
    /// A scoped subscription (protocol 6): only this scope's events, as
    /// `scope.event`. `None`: every event, as `events.event`.
    scope: Option<Scope>,
}

/// One buffered event with its place in its scope.
struct Stamped {
    event: Event,
    scope: Option<Scope>,
    scope_seq: u64,
}

/// The sequence of one scope.
#[derive(Debug, Default, Clone, Copy)]
struct ScopeSeq {
    /// Last sequence assigned in the scope. Never reset within a run, so
    /// an old `from_seq` never replays other events (a repo retired and
    /// added again continues its sequence).
    seq: u64,
    /// Last sequence of the scope that left the replay buffer: a scoped
    /// subscription can replay only from above it.
    floor: u64,
}

struct BusInner {
    seq: u64,
    replay: VecDeque<Stamped>,
    scopes: BTreeMap<Scope, ScopeSeq>,
    engine: EngineShared,
    subscribers: Vec<Subscriber>,
    next_batch: u64,
}

impl BusInner {
    fn scope_seq(&self, scope: &Scope) -> u64 {
        self.scopes.get(scope).map_or(0, |s| s.seq)
    }

    fn knows(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Global => true,
            Scope::Repo { repo_id } => self.engine.repos.iter().any(|r| &r.repo_id == repo_id),
        }
    }
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
    /// The scope names a repo that is not observed.
    UnknownScope,
}

impl EventBus {
    pub fn new(run_id: String, engine: EngineShared, replay_capacity: usize) -> Self {
        Self {
            run_id,
            replay_capacity,
            inner: Mutex::new(BusInner {
                seq: 0,
                replay: VecDeque::new(),
                scopes: BTreeMap::new(),
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

    /// The engine view and the sequence of `scope` it reflects, read
    /// atomically (N1). `None` if the scope names a repo not observed.
    pub fn scope_snapshot(&self, scope: &Scope) -> Option<(u64, EngineShared)> {
        let inner = self.lock();
        inner
            .knows(scope)
            .then(|| (inner.scope_seq(scope), inner.engine.clone()))
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
        self.publish_with(kind, timings, |engine| {
            update(engine);
            data
        })
    }

    /// [`EventBus::publish`] with the data built from the engine view in the
    /// same critical section that changes it, when the event depends on what
    /// the view held before.
    pub fn publish_with<D: Serialize>(
        &self,
        kind: &str,
        timings: Option<Timings>,
        update: impl FnOnce(&mut EngineShared) -> D,
    ) -> u64 {
        let mut inner = self.lock();
        let data = update(&mut inner.engine);
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
        // A repo kind whose data has no `repo_id` has no scope: only the
        // unscoped subscriptions of protocol 5 receive it.
        let scope = event.scope();
        let scope_seq = match &scope {
            Some(scope) => {
                let entry = inner.scopes.entry(scope.clone()).or_default();
                entry.seq += 1;
                entry.seq
            }
            None => 0,
        };
        let stamped = Stamped {
            event,
            scope,
            scope_seq,
        };
        inner.subscribers.retain(|sub| deliver(sub, &stamped));
        if let Some(repo_id) = retired_repo(&stamped.event) {
            close_scope(&mut inner.subscribers, &repo_id);
        }
        inner.replay.push_back(stamped);
        while inner.replay.len() > self.replay_capacity {
            if let Some(Stamped {
                scope: Some(scope),
                scope_seq,
                ..
            }) = inner.replay.pop_front()
                && let Some(entry) = inner.scopes.get_mut(&scope)
            {
                entry.floor = scope_seq;
            }
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
            let oldest = inner.replay.front().map_or(next, |e| e.event.seq);
            if from < oldest {
                return resync(ResyncReason::ReplayUnavailable);
            }
        }
        let sub = Subscriber {
            outbox: Arc::clone(outbox),
            id,
            mcp,
            scope: None,
        };
        for stamped in inner.replay.iter().filter(|e| e.event.seq >= from) {
            if !deliver(&sub, stamped) {
                return Subscribed::Resync;
            }
        }
        inner.subscribers.push(sub);
        Subscribed::From(from)
    }

    /// Subscribes `outbox` to `scope` as subscription `id` (protocol 6).
    /// With `from_seq` (a sequence of the scope), replays the buffered
    /// events of the scope from there; a different `run_id` or a sequence
    /// that left the buffer queues a `scope.resync` instead (N2).
    pub fn subscribe_scope(
        &self,
        outbox: &Arc<Outbox>,
        id: u32,
        scope: &Scope,
        from_seq: Option<u64>,
        run_id: Option<&str>,
    ) -> Subscribed {
        let mut inner = self.lock();
        let resync = |reason| {
            outbox.push(&Notification::new(
                NOTIFY_SCOPE_RESYNC,
                ScopeResyncNotification {
                    scope: scope.clone(),
                    reason,
                },
            ));
            Subscribed::Resync
        };
        if run_id.is_some_and(|r| r != self.run_id) {
            return resync(ResyncReason::DaemonRestarted);
        }
        if !inner.knows(scope) {
            return Subscribed::UnknownScope;
        }
        let state = inner.scopes.get(scope).copied().unwrap_or_default();
        let next = state.seq + 1;
        let from = from_seq.unwrap_or(next).clamp(1, next);
        if from <= state.floor {
            return resync(ResyncReason::ReplayUnavailable);
        }
        let sub = Subscriber {
            outbox: Arc::clone(outbox),
            id,
            mcp: false,
            scope: Some(scope.clone()),
        };
        for stamped in inner
            .replay
            .iter()
            .filter(|e| e.scope.as_ref() == Some(scope) && e.scope_seq >= from)
        {
            if !deliver(&sub, stamped) {
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

fn deliver(sub: &Subscriber, stamped: &Stamped) -> bool {
    let event = &stamped.event;
    if sub.outbox.skips(event) {
        return true;
    }
    match &sub.scope {
        None => {
            if sub.mcp && !mcp_kind(&event.kind) {
                return true;
            }
            sub.outbox.push(&Notification::new(
                NOTIFY_EVENT,
                EventNotification {
                    subscription: sub.id,
                    event: sub.outbox.shape(event).into_owned(),
                },
            ))
        }
        Some(scope) if stamped.scope.as_ref() == Some(scope) => {
            sub.outbox.push(&Notification::new(
                NOTIFY_SCOPE_EVENT,
                ScopeEventNotification {
                    subscription: sub.id,
                    scope: scope.clone(),
                    scope_seq: stamped.scope_seq,
                    event: sub.outbox.shape(event).into_owned(),
                },
            ))
        }
        Some(_) => true,
    }
}

/// The repo a `repo.observation` event stops observing.
fn retired_repo(event: &Event) -> Option<String> {
    use gitraptor_api::event::REPO_OBSERVATION;
    if event.kind != REPO_OBSERVATION || event.data.get("observed")?.as_bool()? {
        return None;
    }
    event.data.get("repo_id")?.as_str().map(str::to_owned)
}

/// A repo stopped being observed: its scoped subscriptions get a
/// `scope.resync { scope-closed }` and end. Its sequence is kept.
fn close_scope(subscribers: &mut Vec<Subscriber>, repo_id: &str) {
    subscribers.retain(|sub| match &sub.scope {
        Some(scope @ Scope::Repo { repo_id: id }) if id == repo_id => {
            sub.outbox.push(&Notification::new(
                NOTIFY_SCOPE_RESYNC,
                ScopeResyncNotification {
                    scope: scope.clone(),
                    reason: ResyncReason::ScopeClosed,
                },
            ));
            false
        }
        _ => true,
    });
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

    fn scoped(messages: &[ServerMessage]) -> Vec<(u64, u64)> {
        messages
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Notification(n) if n.method == NOTIFY_SCOPE_EVENT => Some((
                    n.params["scope_seq"].as_u64().unwrap(),
                    n.params["event"]["seq"].as_u64().unwrap(),
                )),
                _ => None,
            })
            .collect()
    }

    /// N2: a scope's sequence is contiguous whatever is published in other
    /// scopes, and its floor follows the replay buffer.
    #[test]
    fn scope_sequences_are_contiguous_and_replay_has_a_floor() {
        use gitraptor_api::event::OPERATION_QUEUED;
        let bus = bus(3);
        let repo = |id: &str| serde_json::json!({ "repo_id": id });
        bus.publish(RESERVED_AUDIT, 1, None, |_| {}); // global 1
        bus.publish(OPERATION_QUEUED, repo("b"), None, |_| {}); // b 1
        bus.publish(RESERVED_AUDIT, 2, None, |_| {}); // global 2
        bus.publish(OPERATION_QUEUED, repo("b"), None, |_| {}); // b 2
        let out = Outbox::new(64);
        // Global 1 left the buffer (capacity 3): from 1 is a resync, from
        // 2 replays.
        assert_eq!(
            bus.subscribe_scope(&out, 1, &Scope::Global, Some(1), None),
            Subscribed::Resync
        );
        let out = Outbox::new(64);
        assert_eq!(
            bus.subscribe_scope(&out, 1, &Scope::Global, Some(2), None),
            Subscribed::From(2)
        );
        bus.publish(OPERATION_QUEUED, repo("b"), None, |_| {});
        bus.publish(RESERVED_AUDIT, 3, None, |_| {});
        assert_eq!(scoped(&drain(&out)), [(2, 3), (3, 6)]);
        // An unknown repo has no scope to subscribe to.
        let out = Outbox::new(64);
        assert_eq!(
            bus.subscribe_scope(
                &out,
                1,
                &Scope::Repo {
                    repo_id: "b".into()
                },
                None,
                None
            ),
            Subscribed::UnknownScope
        );
    }

    /// SEC-08 with scopes: a scoped subscriber that does not read is
    /// dropped with the connection-level resync, never blocking.
    #[test]
    fn scoped_subscriber_that_does_not_read_gets_a_connection_resync() {
        let bus = bus(8);
        let slow = Outbox::new(4);
        assert_eq!(
            bus.subscribe_scope(&slow, 1, &Scope::Global, None, None),
            Subscribed::From(1)
        );
        for i in 0..100 {
            bus.publish(RESERVED_AUDIT, i, None, |_| {});
        }
        assert!(slow.is_lagged());
        assert_eq!(bus.subscriber_count(), 0);
        let msgs = drain(&slow);
        assert!(matches!(&msgs[..], [ServerMessage::Notification(n)] if n.method == NOTIFY_RESYNC));
    }

    /// US-GRD-019: without `events.authorship` (the default, and always for
    /// `raptor-mcp`) a `git.event` loses the declared authorship and the
    /// trailer check of its hint; with it, both stay.
    #[test]
    fn git_events_carry_authorship_only_with_the_capability() {
        use gitraptor_api::event::GIT_EVENT;
        let data = serde_json::json!({
            "repo_id": "r", "seq": 1, "worktree": {"untrusted": "/wt"}, "kind": "commit",
            "actor": {"actor": "unattributed"}, "observed_utc_ms": 1, "utc_offset_s": 0,
            "details": {},
            "inferred": {"kind": "claude-code", "session_id": "1:1", "trailer": "confirmed"},
            "authorship": {
                "author": {"name": {"untrusted": "Ana"}, "email": {"untrusted": "ana@x"}},
                "committer": {"name": {"untrusted": "Ana"}, "email": {"untrusted": "ana@x"}}
            }
        });
        let event = Event {
            seq: 1,
            kind: GIT_EVENT.into(),
            version: 1,
            wall_ms: 1,
            timings: None,
            data: data.clone(),
        };
        let outbox = Outbox::new(4);
        let shaped = outbox.shape(&event).into_owned();
        assert!(shaped.data.get("authorship").is_none(), "{}", shaped.data);
        assert!(
            shaped.data["inferred"].get("trailer").is_none(),
            "{}",
            shaped.data
        );
        assert_eq!(shaped.data["inferred"]["session_id"], "1:1");
        assert!(!shaped.data.to_string().contains("Ana"));
        outbox.set_without_authorship(false);
        assert_eq!(outbox.shape(&event).data, data);
    }
}
