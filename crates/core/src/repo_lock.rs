//! The write lock of a repo in the daemon, shared by every writer: the Time Machine applier
//! (undo, redo, restore; ADR-TMC-002 § 3, step 3) and the user-operation executor
//! (ADR-CKP-002 § 5). It lives in a neutral module so the executor does not import the Time
//! Machine write layer, and so a merge and an undo never interleave.
//!
//! Two ways to take it:
//! - [`try_lock`] never waits: a second applier on the same repo is told the repo is busy.
//! - [`lock_queued`] waits in arrival order, behind at most `max_waiting` other requests, and
//!   gives up when `give_up` says so (the daemon stops, the request is cancelled). A request that arrives with the queue full is told
//!   the repo is busy. A waiter is never overtaken by a later [`try_lock`].
//!
//! The key must be the same for every writer of one repo: the path of the repo's snapshot store,
//! unique per repo and per profile.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// How often a waiter looks at its cancel flag.
const POLL: Duration = Duration::from_millis(25);

#[derive(Default)]
struct Slot {
    held: bool,
    queue: VecDeque<u64>,
}

#[derive(Default)]
struct Registry {
    next_ticket: u64,
    slots: HashMap<String, Slot>,
}

struct Locks {
    registry: Mutex<Registry>,
    freed: Condvar,
}

fn locks() -> &'static Locks {
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    LOCKS.get_or_init(|| Locks {
        registry: Mutex::new(Registry::default()),
        freed: Condvar::new(),
    })
}

fn registry() -> MutexGuard<'static, Registry> {
    locks().registry.lock().unwrap_or_else(|e| e.into_inner())
}

/// The repo, taken. Dropping it (also on panic) frees the repo and wakes the next waiter.
#[derive(Debug)]
pub struct RepoGuard {
    repo_id: String,
}

impl RepoGuard {
    /// The key of the repo this guard holds.
    pub fn key(&self) -> &str {
        &self.repo_id
    }
}

/// Takes the repo keyed `repo_id` if no one holds it and no one waits for it.
pub fn try_lock(repo_id: &str) -> Option<RepoGuard> {
    let mut reg = registry();
    let slot = reg.slots.entry(repo_id.to_owned()).or_default();
    if slot.held || !slot.queue.is_empty() {
        return None;
    }
    slot.held = true;
    Some(RepoGuard {
        repo_id: repo_id.to_owned(),
    })
}

/// Why [`lock_queued`] gave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// `max_waiting` requests already wait for this repo.
    Full,
    /// `give_up` returned true while waiting.
    Cancelled,
}

/// Takes the repo keyed `repo_id`, waiting in arrival order. `queued` is called once, with the
/// number of requests ahead (the holder included), when the request has to wait.
pub fn lock_queued(
    repo_id: &str,
    max_waiting: usize,
    give_up: &dyn Fn() -> bool,
    queued: impl FnOnce(u32),
) -> Result<RepoGuard, QueueError> {
    let mut reg = registry();
    let ticket = reg.next_ticket;
    reg.next_ticket += 1;
    let slot = reg.slots.entry(repo_id.to_owned()).or_default();
    if !slot.held && slot.queue.is_empty() {
        slot.held = true;
        return Ok(RepoGuard {
            repo_id: repo_id.to_owned(),
        });
    }
    if slot.queue.len() >= max_waiting {
        return Err(QueueError::Full);
    }
    slot.queue.push_back(ticket);
    let ahead = u32::try_from(slot.queue.len()).unwrap_or(u32::MAX);
    drop(reg);
    queued(ahead);

    let mut reg = registry();
    loop {
        let slot = reg.slots.entry(repo_id.to_owned()).or_default();
        if !slot.held && slot.queue.front() == Some(&ticket) {
            slot.queue.pop_front();
            slot.held = true;
            return Ok(RepoGuard {
                repo_id: repo_id.to_owned(),
            });
        }
        if give_up() {
            slot.queue.retain(|t| *t != ticket);
            // The next in line may be free to go now.
            locks().freed.notify_all();
            return Err(QueueError::Cancelled);
        }
        reg = locks()
            .freed
            .wait_timeout(reg, POLL)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// Requests waiting for `repo_id` (not counting the holder).
pub fn waiting(repo_id: &str) -> usize {
    registry().slots.get(repo_id).map_or(0, |s| s.queue.len())
}

impl Drop for RepoGuard {
    fn drop(&mut self) {
        let mut reg = registry();
        if let Some(slot) = reg.slots.get_mut(&self.repo_id) {
            slot.held = false;
            if slot.queue.is_empty() {
                reg.slots.remove(&self.repo_id);
            }
        }
        drop(reg);
        locks().freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    #[test]
    fn one_holder_per_repo() {
        let a = try_lock("repo-lock-test-a").unwrap();
        assert!(try_lock("repo-lock-test-a").is_none());
        let b = try_lock("repo-lock-test-b").unwrap();
        drop(a);
        assert!(try_lock("repo-lock-test-a").is_some());
        drop(b);
    }

    /// Q-CKP-19: requests wait in arrival order, the queue is bounded, and a later `try_lock`
    /// never overtakes a waiter.
    #[test]
    fn waiters_go_in_arrival_order() {
        const KEY: &str = "repo-lock-test-queue";
        let holder = try_lock(KEY).unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let never = Arc::new(AtomicBool::new(false));
        let mut joins = Vec::new();
        for i in 0..3u32 {
            let (tx, rx) = mpsc::channel();
            let order = Arc::clone(&order);
            let never = Arc::clone(&never);
            joins.push(std::thread::spawn(move || {
                let guard = lock_queued(KEY, 3, &|| never.load(Ordering::SeqCst), |ahead| {
                    tx.send(ahead).unwrap()
                })
                .unwrap();
                order.lock().unwrap().push(i);
                std::thread::sleep(Duration::from_millis(10));
                drop(guard);
            }));
            assert_eq!(rx.recv().unwrap(), i + 1);
        }
        assert_eq!(waiting(KEY), 3);
        let full = lock_queued(KEY, 3, &|| false, |_| {});
        assert_eq!(full.unwrap_err(), QueueError::Full);
        assert!(try_lock(KEY).is_none());
        drop(holder);
        for j in joins {
            j.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), [0, 1, 2]);
        assert!(try_lock(KEY).is_some());
    }

    #[test]
    fn a_cancelled_waiter_leaves_the_queue() {
        const KEY: &str = "repo-lock-test-cancel";
        let holder = try_lock(KEY).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel();
        let join = std::thread::spawn(move || {
            lock_queued(KEY, 8, &|| flag.load(Ordering::SeqCst), |_| {
                tx.send(()).unwrap()
            })
        });
        rx.recv().unwrap();
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(join.join().unwrap().unwrap_err(), QueueError::Cancelled);
        assert_eq!(waiting(KEY), 0);
        drop(holder);
        assert!(try_lock(KEY).is_some());
    }
}
