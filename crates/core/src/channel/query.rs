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

/// One thread (`raptor-query`) and a bounded queue shared by every connection. The thread ends
/// when the pool is dropped, which is when the channel is.
pub(crate) struct QueryPool {
    queue: SyncSender<Job>,
}

impl QueryPool {
    /// A pool that holds at most `queue` waiting jobs. If the thread cannot start, every
    /// submission is refused as busy.
    pub fn new(queue: usize) -> Self {
        let (tx, rx) = sync_channel::<Job>(queue);
        // A panicking job must not take the pool with it.
        let _ = spawn_unpooled(WorkClass::UserInitiated, "raptor-query", move || {
            for job in rx {
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
        if table.len() >= QUERIES_PER_CONNECTION as usize {
            return Err(Busy);
        }
        let token = QueryToken(Arc::new(AtomicBool::new(false)));
        table.push_back((id.clone(), token.clone()));
        Ok(token)
    }

    /// Cancels request `id`; `false` when it is not in flight here.
    pub fn cancel(&self, id: &Id) -> bool {
        let table = self.lock();
        let Some((_, token)) = table.iter().find(|(i, _)| i == id) else {
            return false;
        };
        token.0.store(true, Ordering::SeqCst);
        true
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
    fn cancel_finds_only_what_is_in_flight() {
        let table = QueryTable::default();
        let a = Id::Num(1);
        let token = table.register(&a).unwrap();
        assert!(!table.cancel(&Id::Num(9)));
        assert!(table.cancel(&a));
        assert!(token.is_cancelled());
        table.finish(&a);
        assert!(!table.cancel(&a));
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
