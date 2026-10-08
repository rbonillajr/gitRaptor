//! Processes started by a protected operation (DEP-MCP-3).
//!
//! A hook or `git` that the operation's step runs acts with the authority of
//! whoever asked for the operation: a confused deputy if it could use a
//! reserved command or pass for someone else. Every child the step spawns is
//! marked by `(pid, start)` and by its process group, from the moment it
//! starts until the operation closes (not until the child ends, so an orphan
//! grandchild of the same group is still covered). The channel treats a
//! marked process, and any descendant of one, as a descendant of the
//! executor: attributed to the operation's requester and refused for
//! reserved commands.
//!
//! Start barrier (I-02, ADR-CKP-002 § 4), in safe Rust: before a step launches a child it opens
//! a *pending spawn*, closed once the child is marked. A caller that descends from the daemon
//! without a mark while a spawn is pending waits for the registration, up to
//! [`REGISTRATION_WAIT`]; past it, its identity counts as unverified and it is refused. So a hook
//! that connects the instant `git` starts is attributed to the plan's requester, never resolved
//! as "unattributed" in the window before the mark. (A suspended spawn needs `unsafe` or
//! `posix_spawn`, forbidden by the workspace lints: Pendiente in ADR-CKP-002.)
//!
//! Accepted residual risk (ADR-GRP-005 § 6): a grandchild that changes its
//! process group and detaches from the tree escapes the mark.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::peer::ProcInfo;
use super::requester::Who;

#[derive(Debug, Clone)]
struct Mark {
    operation_id: String,
    who: Who,
    /// When the operation opened, on the clock of the start times
    /// ([`super::peer::proc_clock_us`]), microseconds since the epoch.
    /// A process group only matches processes started after it, so a
    /// reused group id of an older process never matches.
    opened_us: u64,
    children: Vec<(u32, u64)>,
    groups: Vec<u32>,
}

/// The operation a marked process belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedBy {
    pub operation_id: String,
    pub who: Who,
}

/// The marks of the operations running now. Shared by the protected
/// operation (writes) and the channel (reads).
#[derive(Debug, Default)]
pub struct ExecutorMarks {
    marks: Mutex<Vec<Mark>>,
    /// Children being launched and not marked yet.
    pending: AtomicUsize,
}

/// Longest wait of a caller for a pending registration.
pub const REGISTRATION_WAIT: Duration = Duration::from_secs(2);

/// A spawn in flight: closed when dropped, after the child is marked or the spawn failed.
#[derive(Debug)]
pub struct PendingSpawn<'a> {
    marks: &'a ExecutorMarks,
}

impl Drop for PendingSpawn<'_> {
    fn drop(&mut self) {
        self.marks.pending.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ExecutorMarks {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Mark>> {
        // A panic while holding the lock leaves a consistent list.
        self.marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Opens the marks of an operation; they last until the guard drops.
    pub fn open(self: &Arc<Self>, operation_id: &str, who: &Who, opened_us: u64) -> MarkGuard {
        self.lock().push(Mark {
            operation_id: operation_id.to_owned(),
            who: who.clone(),
            opened_us,
            children: Vec::new(),
            groups: Vec::new(),
        });
        MarkGuard {
            marks: Arc::clone(self),
            operation_id: operation_id.to_owned(),
        }
    }

    /// Marks a child the operation started.
    pub fn add_child(&self, operation_id: &str, pid: u32, start_us: u64, pgid: u32) {
        if let Some(m) = self
            .lock()
            .iter_mut()
            .find(|m| m.operation_id == operation_id)
        {
            m.children.push((pid, start_us));
            if pgid > 1 && !m.groups.contains(&pgid) {
                m.groups.push(pgid);
            }
        }
    }

    /// Whether `info` is a process started by a running operation.
    pub fn lookup(&self, info: &ProcInfo) -> Option<MarkedBy> {
        self.lock()
            .iter()
            .find(|m| {
                m.children.contains(&(info.pid, info.start_us))
                    || (m.groups.contains(&info.pgid) && info.start_us >= m.opened_us)
            })
            .map(|m| MarkedBy {
                operation_id: m.operation_id.clone(),
                who: m.who.clone(),
            })
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Opens a pending spawn (start barrier, I-02).
    pub fn begin_spawn(&self) -> PendingSpawn<'_> {
        self.pending.fetch_add(1, Ordering::SeqCst);
        PendingSpawn { marks: self }
    }

    /// Whether a child is being launched and not marked yet.
    pub fn spawn_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }

    /// Waits until no spawn is pending, up to `timeout`. `false` if one still is.
    pub fn wait_registered(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.spawn_pending() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }
}

/// Removes the marks of an operation when it closes.
#[derive(Debug)]
pub struct MarkGuard {
    marks: Arc<ExecutorMarks>,
    operation_id: String,
}

impl Drop for MarkGuard {
    fn drop(&mut self) {
        self.marks
            .lock()
            .retain(|m| m.operation_id != self.operation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_spawn_holds_the_barrier_until_dropped() {
        let marks = ExecutorMarks::default();
        assert!(marks.wait_registered(Duration::ZERO));
        let pending = marks.begin_spawn();
        assert!(!marks.wait_registered(Duration::from_millis(20)));
        drop(pending);
        assert!(marks.wait_registered(Duration::ZERO));
    }
    use gitraptor_api::Actor;

    fn proc(pid: u32, start: u64, pgid: u32) -> ProcInfo {
        ProcInfo {
            pid,
            ppid: 1,
            uid: 501,
            start_us: start,
            exe: None,
            controlling_terminal: false,
            desktop_session: None,
            session: pid,
            pgid,
        }
    }

    #[test]
    fn children_and_their_group_are_marked_until_the_operation_closes() {
        let marks = Arc::new(ExecutorMarks::default());
        let who = Who::unattributed();
        let guard = marks.open("op-1", &who, 1_000);
        marks.add_child("op-1", 50, 1_100, 50);
        assert_eq!(
            marks.lookup(&proc(50, 1_100, 50)).unwrap().operation_id,
            "op-1"
        );
        // A grandchild of the same group, even reparented after the child
        // ended.
        assert!(marks.lookup(&proc(51, 1_200, 50)).is_some());
        // The same pid with another start, or an older process of a reused
        // group, is not marked.
        assert!(marks.lookup(&proc(50, 900, 7)).is_none());
        assert!(marks.lookup(&proc(52, 900, 50)).is_none());
        drop(guard);
        assert!(marks.lookup(&proc(50, 1_100, 50)).is_none());
        assert!(marks.is_empty());
        assert_eq!(who.actor, Actor::Unattributed);
    }
}
