//! On-demand queries (N8): a pool with its own thread, a token per request and the helper that
//! answers out of order.
//!
//! A query is any request whose answer is computed away from the connection's reader: the reader
//! registers it ([`QueryTable::register`]), queues the job ([`QueryPool::submit`]) and goes back
//! to reading. The job runs in [`QueryTable::run`], which looks at the token, always calls
//! `finish` and pushes the one answer (`query-cancelled` or the result) to the connection's
//! outbox. Nothing here knows which query it serves.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

use gitraptor_api::diff::{QUERIES_PER_CONNECTION, QUERY_QUEUE};
use gitraptor_api::methods::{QUERY_BUSY, QUERY_CANCELLED};
use gitraptor_api::query::QueryBusyData;
use gitraptor_api::rpc::{ErrorObject, Id, Response, code};
use serde::Serialize;

use super::bus::Outbox;
use crate::work_class::{WorkClass, spawn_unpooled};

/// A job that could not be accepted: the connection has one in flight, or the queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Busy;

impl Busy {
    /// The `query-busy` error of the contract.
    pub fn error(self) -> ErrorObject {
        ErrorObject::new(QUERY_BUSY.code, QUERY_BUSY.name).with_data(QueryBusyData {
            limit: QUERIES_PER_CONNECTION,
        })
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// Tests only: holds the pool thread before each job until a test opens it, so a test that needs
/// "the pool is busy" holds it instead of racing the speed of a diff. It does not exist in a
/// release build and is absent in production.
#[cfg(debug_assertions)]
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct QueryGate(Arc<(Mutex<bool>, std::sync::Condvar)>);

#[cfg(debug_assertions)]
impl QueryGate {
    /// A gate that holds every job until [`Self::open`].
    pub fn closed() -> Self {
        Self::default()
    }

    /// Lets the held job and every later one run.
    pub fn open(&self) {
        let (open, cv) = &*self.0;
        *open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
    }

    fn wait(&self) {
        let (open, cv) = &*self.0;
        let mut open = open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = cv.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(debug_assertions)]
impl std::fmt::Debug for QueryGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QueryGate")
    }
}

#[cfg(debug_assertions)]
impl PartialEq for QueryGate {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(debug_assertions)]
impl Eq for QueryGate {}

/// What the pool thread runs before each job; `None` in production.
type Hold = Option<Box<dyn Fn() + Send>>;

/// One thread (`raptor-query`) and a bounded queue shared by every connection. The thread ends
/// when the pool is dropped, which is when the channel is.
pub(crate) struct QueryPool {
    queue: SyncSender<Job>,
}

impl QueryPool {
    /// A pool that holds at most `queue` waiting jobs. If the thread cannot start, every
    /// submission is refused as busy.
    pub fn new(queue: usize) -> Self {
        Self::start(queue, None)
    }

    /// A pool whose thread waits at `gate` before each job (tests only).
    #[cfg(debug_assertions)]
    pub fn gated(queue: usize, gate: Option<QueryGate>) -> Self {
        Self::start(queue, gate.map(|g| Box::new(move || g.wait()) as _))
    }

    fn start(queue: usize, hold: Hold) -> Self {
        let (tx, rx) = sync_channel::<Job>(queue);
        // A panicking job must not take the pool with it.
        let _ = spawn_unpooled(WorkClass::UserInitiated, "raptor-query", move || {
            for job in rx {
                if let Some(hold) = &hold {
                    hold();
                }
                let _ = catch_unwind(AssertUnwindSafe(job));
            }
        });
        Self { queue: tx }
    }

    /// Queues `job`.
    ///
    /// # Errors
    /// [`Busy`] when the queue is full or the pool is gone.
    pub fn submit(&self, job: Box<dyn FnOnce() + Send>) -> Result<(), Busy> {
        match self.queue.try_send(job) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => Err(Busy),
        }
    }
}

impl Default for QueryPool {
    fn default() -> Self {
        Self::new(QUERY_QUEUE)
    }
}

/// The cancellation flag of one request.
#[derive(Clone)]
pub(crate) struct QueryToken(Arc<AtomicBool>);

impl QueryToken {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The requests of one connection that are in flight. Cloning shares the table, so a job can
/// call [`Self::finish`] from the pool's thread.
#[derive(Clone, Default)]
pub(crate) struct QueryTable {
    inflight: Arc<Mutex<VecDeque<(Id, QueryToken)>>>,
}

impl QueryTable {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(Id, QueryToken)>> {
        self.inflight.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers request `id`.
    ///
    /// # Errors
    /// [`Busy`] when the connection already has [`QUERIES_PER_CONNECTION`] in flight.
    pub fn register(&self, id: &Id) -> Result<QueryToken, Busy> {
        let mut table = self.lock();
        // A request that was cancelled while it waited in the queue no longer counts: its job
        // will only answer `query-cancelled`, so the connection may ask again at once.
        let live = table.iter().filter(|(_, t)| !t.is_cancelled()).count();
        if live >= QUERIES_PER_CONNECTION as usize {
            return Err(Busy);
        }
        let token = QueryToken(Arc::new(AtomicBool::new(false)));
        table.push_back((id.clone(), token.clone()));
        Ok(token)
    }

    /// Cancels request `id`. `announce` receives whether it was in flight here and pushes the
    /// answer of `query.cancel`; it runs BEFORE the token is set, so the job, which only sees the
    /// cancellation after that, can never answer `query-cancelled` ahead of the cancel's own
    /// answer (the contract: `query.cancel` answers now, the query ends after).
    pub fn cancel(&self, id: &Id, announce: impl FnOnce(bool)) {
        let table = self.lock();
        let token = table.iter().find(|(i, _)| i == id).map(|(_, t)| t);
        announce(token.is_some());
        if let Some(token) = token {
            token.0.store(true, Ordering::SeqCst);
        }
    }

    /// Removes request `id`: the connection may ask again.
    pub fn finish(&self, id: &Id) {
        self.lock().retain(|(i, _)| i != id);
    }

    /// Cancels every request: the connection is closing.
    pub fn cancel_all(&self) {
        for (_, token) in self.lock().iter() {
            token.0.store(true, Ordering::SeqCst);
        }
    }

    /// The body of every job: runs `work` unless the request is already cancelled, then removes
    /// it from the table and pushes the one answer to `outbox`: `query-cancelled` if the token
    /// says so, else the result or its error. A closed outbox drops the answer.
    pub fn run<T: Serialize>(
        &self,
        outbox: &Outbox,
        id: Id,
        token: &QueryToken,
        work: impl FnOnce(&QueryToken) -> Result<T, ErrorObject>,
    ) {
        let result = if token.is_cancelled() {
            Ok(None)
        } else {
            catch_unwind(AssertUnwindSafe(|| work(token)))
                .unwrap_or_else(|_| Err(ErrorObject::new(code::INTERNAL, "query failed")))
                .map(Some)
        };
        self.finish(&id);
        let response = match result {
            _ if token.is_cancelled() => Response::err(
                Some(id),
                ErrorObject::new(QUERY_CANCELLED.code, QUERY_CANCELLED.name),
            ),
            Ok(Some(value)) => Response::ok(id, value),
            Ok(None) => return,
            Err(error) => Response::err(Some(id), error),
        };
        outbox.push(&response);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_connection_has_one_query_in_flight_until_it_finishes() {
        let table = QueryTable::default();
        let (a, b) = (Id::Num(1), Id::Num(2));
        assert!(table.register(&a).is_ok());
        assert_eq!(table.register(&b).err(), Some(Busy));
        table.finish(&a);
        assert!(table.register(&b).is_ok());
    }

    #[test]
    fn a_cancelled_request_does_not_hold_the_slot() {
        let table = QueryTable::default();
        let a = Id::Num(1);
        table.register(&a).unwrap();
        table.cancel(&a, |_| {});
        assert!(table.register(&Id::Num(2)).is_ok());
    }

    #[test]
    fn cancel_finds_only_what_is_in_flight() {
        let table = QueryTable::default();
        let a = Id::Num(1);
        let token = table.register(&a).unwrap();
        let found = |id: &Id| {
            let mut found = None;
            table.cancel(id, |f| found = Some(f));
            found.unwrap()
        };
        assert!(!found(&Id::Num(9)));
        assert!(found(&a));
        assert!(token.is_cancelled());
        table.finish(&a);
        assert!(!found(&a));
    }

    #[test]
    fn cancel_announces_before_the_token_is_set() {
        let table = QueryTable::default();
        let a = Id::Num(1);
        let token = table.register(&a).unwrap();
        let mut seen = None;
        table.cancel(&a, |found| seen = Some((found, token.is_cancelled())));
        assert_eq!(seen, Some((true, false)), "the answer goes out first");
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_all_marks_every_token() {
        let table = QueryTable::default();
        let token = table.register(&Id::Num(1)).unwrap();
        table.cancel_all();
        assert!(token.is_cancelled());
    }

    #[test]
    fn the_pool_refuses_past_its_queue_and_keeps_serving_after_a_panic() {
        let pool = QueryPool::new(1);
        let (gate_tx, gate_rx) = channel::<()>();
        let (started_tx, started_rx) = channel::<()>();
        // One job holds the thread; the next waits in the queue; a third is busy.
        pool.submit(Box::new(move || {
            started_tx.send(()).unwrap();
            let _ = gate_rx.recv();
            panic!("a job that panics");
        }))
        .unwrap();
        started_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let (done_tx, done_rx) = channel::<()>();
        pool.submit(Box::new(move || done_tx.send(()).unwrap()))
            .unwrap();
        assert_eq!(pool.submit(Box::new(|| {})), Err(Busy));
        gate_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    }
}
