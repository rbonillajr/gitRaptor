//! The coalescing queue of dry merges: at most one job per pair, by priority, then by arrival.

use std::collections::{BTreeSet, HashMap};

use super::{PairKey, Triple};

/// Which job goes first. Lower sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// A pair against the base of the worktree that changed.
    ChangedVsBase,
    /// A pair with a worktree that has a session present.
    PresentSession,
    Rest,
}

/// Pending dry merges. A push for a pair already queued replaces its job, with the new
/// priority and a new place in the arrival order.
#[derive(Debug, Default)]
pub struct PairQueue {
    jobs: HashMap<PairKey, (Priority, u64, Triple)>,
    order: BTreeSet<(Priority, u64, PairKey)>,
    next: u64,
}

impl PairQueue {
    pub fn push(&mut self, key: PairKey, triple: Triple, p: Priority) {
        let seq = self.next;
        self.next += 1;
        if let Some((old_p, old_seq, _)) = self.jobs.remove(&key) {
            self.order.remove(&(old_p, old_seq, key.clone()));
        }
        self.order.insert((p, seq, key.clone()));
        self.jobs.insert(key, (p, seq, triple));
    }

    /// The job of highest priority, the oldest of its priority.
    pub fn pop(&mut self) -> Option<(PairKey, Triple)> {
        let (_, _, key) = self.order.pop_first()?;
        let (_, _, triple) = self.jobs.remove(&key)?;
        Some((key, triple))
    }

    /// Drops every job of `repo_id` (a dormant or retired repo).
    pub fn forget_repo(&mut self, repo_id: &str) {
        self.jobs.retain(|key, _| key.repo_id != repo_id);
        self.order.retain(|(_, _, key)| key.repo_id != repo_id);
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::predict::PairSide;

    fn key(n: &str) -> PairKey {
        PairKey {
            repo_id: "r".into(),
            a: PathBuf::from(n),
            b: PairSide::Base,
        }
    }

    fn triple(n: &str) -> Triple {
        Triple {
            tip_a: n.into(),
            tip_b: "b".into(),
            merge_base: None,
        }
    }

    #[test]
    fn a_replaced_job_leaves_no_stale_order_entry() {
        let mut q = PairQueue::default();
        q.push(key("a"), triple("1"), Priority::Rest);
        q.push(key("a"), triple("2"), Priority::ChangedVsBase);
        q.push(key("a"), triple("3"), Priority::Rest);
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop(), Some((key("a"), triple("3"))));
        assert!(q.is_empty());
        assert_eq!(q.pop(), None);
    }
}
