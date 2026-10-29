//! The pairs of one repo across plans: their state, the cache keyed by triple and the rule that
//! a result of inputs that already changed is never current.

use std::collections::BTreeMap;

use gitraptor_git::dry_merge::{DryMergeError, DryMergeOutcome};

use super::{
    NotComputableReason, PairKey, PairPrediction, PairSide, PairState, PendingReason, Triple,
};

/// How many computed triples a pair keeps: a base that moves and comes back finds its result.
const KEPT_RESULTS: usize = 2;

/// One computed triple of a pair and what its result gave.
struct Kept {
    triple: Triple,
    state: PairState,
    conflict: Option<DryMergeOutcome>,
    computed_at_utc_ms: i64,
}

struct Entry {
    pred: PairPrediction,
    /// The triple last queued or computed: an equal one queues nothing.
    cached: Option<Triple>,
    /// The triple that produced the result `pred` carries, if any. A base move is judged against
    /// it, not against the plan's last triple: a pair that went through `Pending` or
    /// `NotComputable` with a new base must not republish the old base's result.
    result: Option<Triple>,
    /// The results of the last [`KEPT_RESULTS`] computed triples, oldest first.
    done: Vec<Kept>,
}

impl Entry {
    fn has_result(&self) -> bool {
        self.pred.computed_at_utc_ms.is_some()
    }

    fn queued_state(&self) -> PairState {
        if self.has_result() {
            PairState::Recalculating
        } else {
            PairState::Calculating
        }
    }

    /// Takes back the kept result of `triple`, if there is one: the state it gave.
    fn reuse(&mut self, triple: &Triple) -> Option<PairState> {
        let kept = self.done.iter().find(|k| k.triple == *triple)?;
        self.pred.conflict.clone_from(&kept.conflict);
        self.pred.computed_at_utc_ms = Some(kept.computed_at_utc_ms);
        self.result = Some(triple.clone());
        Some(kept.state)
    }
}

/// The pairs of one repo with their cache.
pub struct RepoPairs {
    repo_id: String,
    entries: BTreeMap<PairKey, Entry>,
}

impl RepoPairs {
    pub fn new(repo_id: &str) -> Self {
        Self {
            repo_id: repo_id.to_owned(),
            entries: BTreeMap::new(),
        }
    }

    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// Takes a new plan of the repo and returns the pairs to queue, each with its triple. A pair
    /// whose triple is one of its last computed ones takes that result back and queues nothing;
    /// one whose triple is already queued queues nothing either; one with a new triple is queued
    /// and becomes `Recalculating` if it had a result. A pair against the base whose base tip
    /// moved becomes `Calculating` instead: the old base's result is no longer published, though
    /// it is kept should that triple come back. A `Pending` or `NotComputable` pair of the plan
    /// keeps its last result and queues nothing. The overlap is always the plan's, and a pair
    /// absent from the plan is forgotten. `_now_utc_ms` is the plan's time: a result keeps the
    /// time it was computed at.
    pub fn apply(&mut self, plan: Vec<PairPrediction>, _now_utc_ms: i64) -> Vec<(PairKey, Triple)> {
        let mut old = std::mem::take(&mut self.entries);
        let mut queue = Vec::new();
        for p in plan {
            let mut entry = old.remove(&p.key).unwrap_or_else(|| Entry {
                pred: PairPrediction {
                    computed_at_utc_ms: None,
                    conflict: None,
                    ..p.clone()
                },
                cached: None,
                result: None,
                done: Vec::new(),
            });
            let base_moved = p.key.b == PairSide::Base
                && matches!(
                    (&entry.result, &p.triple),
                    (Some(was), Some(now)) if was.tip_b != now.tip_b
                );
            entry.pred.overlap = p.overlap;
            entry.pred.triple.clone_from(&p.triple);
            entry.pred.state = match (p.state, p.triple) {
                (PairState::Calculating, Some(triple)) => {
                    if let Some(state) = entry.reuse(&triple) {
                        entry.cached = Some(triple);
                        state
                    } else {
                        if base_moved {
                            // Computed against a base that is gone: never published as current
                            // again, but kept in `done` should that base come back.
                            entry.pred.conflict = None;
                            entry.pred.computed_at_utc_ms = None;
                            entry.result = None;
                        }
                        if entry.cached.as_ref() != Some(&triple) {
                            entry.cached = Some(triple.clone());
                            queue.push((p.key.clone(), triple));
                        }
                        entry.queued_state()
                    }
                }
                (PairState::Calculating, None) => {
                    PairState::NotComputable(NotComputableReason::Error)
                }
                (state, _) => state,
            };
            self.entries.insert(p.key, entry);
        }
        queue
    }

    /// Takes the result of the job of `key` for `triple`. Returns `false`, and keeps nothing,
    /// when the pair is gone or its triple is no longer `triple`: a result of stale inputs is
    /// never current. A pair that is `Pending` keeps its state and the result.
    pub fn complete(
        &mut self,
        key: &PairKey,
        triple: &Triple,
        result: Result<DryMergeOutcome, DryMergeError>,
        now_utc_ms: i64,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        if entry.pred.triple.as_ref() != Some(triple) || entry.cached.as_ref() != Some(triple) {
            return false;
        }
        let (state, conflict) = match result {
            Ok(outcome) => (PairState::Current, Some(outcome)),
            Err(err) => (PairState::NotComputable(reason(&err)), None),
        };
        entry.done.retain(|k| k.triple != *triple);
        if entry.done.len() == KEPT_RESULTS {
            entry.done.remove(0);
        }
        entry.done.push(Kept {
            triple: triple.clone(),
            state,
            conflict: conflict.clone(),
            computed_at_utc_ms: now_utc_ms,
        });
        entry.pred.conflict = conflict;
        entry.pred.computed_at_utc_ms = Some(now_utc_ms);
        entry.result = Some(triple.clone());
        if matches!(
            entry.pred.state,
            PairState::Calculating | PairState::Recalculating
        ) {
            entry.pred.state = state;
        }
        true
    }

    /// Every pair waits for `reason`, with its last result and time. `Dormant` also drops the
    /// cache (a dormant repo keeps no resident state), so the next plan queues every pair as the
    /// initial calculation does; other reasons keep it, so only changed triples are queued.
    pub fn suspend(&mut self, reason: PendingReason) {
        for entry in self.entries.values_mut() {
            entry.pred.state = PairState::Pending(reason);
            if reason == PendingReason::Dormant {
                entry.cached = None;
                entry.done.clear();
            }
        }
    }

    pub fn get(&self, key: &PairKey) -> Option<&PairPrediction> {
        self.entries.get(key).map(|e| &e.pred)
    }

    pub fn pairs(&self) -> impl Iterator<Item = &PairPrediction> {
        self.entries.values().map(|e| &e.pred)
    }
}

/// The reason a failed dry merge leaves its pair not computable.
fn reason(err: &DryMergeError) -> NotComputableReason {
    match err {
        DryMergeError::MissingObject => NotComputableReason::MissingObject,
        DryMergeError::Shallow => NotComputableReason::Shallow,
        // The merge absorbs an oversized object as binary; should one arrive, it is a limit.
        DryMergeError::LimitExceeded | DryMergeError::TooLarge => {
            NotComputableReason::LimitExceeded
        }
        DryMergeError::Unavailable(_) => NotComputableReason::Error,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gitraptor_git::dry_merge::MergeBaseUsed;

    use super::*;
    use crate::predict::Overlap;

    fn pred(tip: &str, state: PairState) -> PairPrediction {
        PairPrediction {
            key: PairKey {
                repo_id: "r".into(),
                a: PathBuf::from("/w"),
                b: PairSide::Base,
            },
            triple: Some(triple(tip)),
            state,
            computed_at_utc_ms: None,
            overlap: Overlap::default(),
            conflict: None,
        }
    }

    fn triple(tip: &str) -> Triple {
        Triple {
            tip_a: tip.into(),
            tip_b: "base".into(),
            merge_base: None,
        }
    }

    fn outcome() -> DryMergeOutcome {
        DryMergeOutcome {
            merge_base: MergeBaseUsed::None,
            files: Vec::new(),
            truncated: false,
            rewrites_escalated: false,
        }
    }

    #[test]
    fn an_error_result_maps_to_its_reason() {
        let mut pairs = RepoPairs::new("r");
        let queued = pairs.apply(vec![pred("t", PairState::Calculating)], 0);
        let (key, t) = &queued[0];
        assert!(pairs.complete(key, t, Err(DryMergeError::Shallow), 5));
        let p = pairs.get(key).unwrap();
        assert_eq!(
            p.state,
            PairState::NotComputable(NotComputableReason::Shallow)
        );
        assert_eq!(p.computed_at_utc_ms, Some(5));
    }

    #[test]
    fn a_pending_pair_comes_back_to_its_result_without_queueing() {
        let mut pairs = RepoPairs::new("r");
        let queued = pairs.apply(vec![pred("t", PairState::Calculating)], 0);
        let (key, t) = queued[0].clone();
        assert!(pairs.complete(&key, &t, Ok(outcome()), 1));
        let pending = PairState::Pending(PendingReason::OperationInProgress);
        assert!(pairs.apply(vec![pred("t", pending)], 2).is_empty());
        assert_eq!(pairs.get(&key).unwrap().state, pending);
        assert!(
            pairs
                .apply(vec![pred("t", PairState::Calculating)], 3)
                .is_empty()
        );
        assert_eq!(pairs.get(&key).unwrap().state, PairState::Current);
    }

    fn on_base(tip: &str, base: &str) -> PairPrediction {
        let mut p = pred(tip, PairState::Calculating);
        if let Some(t) = p.triple.as_mut() {
            t.tip_b = base.into();
        }
        p
    }

    #[test]
    fn a_moved_base_calculates_without_the_old_result_and_its_return_reuses_it() {
        let mut pairs = RepoPairs::new("r");
        let queued = pairs.apply(vec![on_base("t", "b1")], 0);
        let (key, t1) = queued[0].clone();
        assert!(pairs.complete(&key, &t1, Ok(outcome()), 1));

        let queued = pairs.apply(vec![on_base("t", "b2")], 2);
        let p = pairs.get(&key).unwrap();
        assert_eq!(p.state, PairState::Calculating);
        assert_eq!((p.computed_at_utc_ms, p.conflict.is_none()), (None, true));
        let (_, t2) = queued[0].clone();
        assert!(pairs.complete(&key, &t2, Ok(outcome()), 3));

        assert!(pairs.apply(vec![on_base("t", "b1")], 4).is_empty());
        let p = pairs.get(&key).unwrap();
        assert_eq!(
            (p.state, p.computed_at_utc_ms),
            (PairState::Current, Some(1))
        );

        // A tip move of the worktree keeps the last result while it recalculates.
        pairs.apply(vec![on_base("t2", "b1")], 5);
        let p = pairs.get(&key).unwrap();
        assert_eq!(
            (p.state, p.computed_at_utc_ms),
            (PairState::Recalculating, Some(1))
        );
    }

    #[test]
    fn a_base_move_while_pending_does_not_republish_the_old_result() {
        let mut pairs = RepoPairs::new("r");
        let queued = pairs.apply(vec![on_base("t", "b1")], 0);
        let (key, t1) = queued[0].clone();
        assert!(pairs.complete(&key, &t1, Ok(outcome()), 1));

        let mut pending = on_base("t", "b2");
        pending.state = PairState::Pending(PendingReason::OperationInProgress);
        assert!(pairs.apply(vec![pending], 2).is_empty());

        let queued = pairs.apply(vec![on_base("t", "b2")], 3);
        assert_eq!(queued.len(), 1);
        let p = pairs.get(&key).unwrap();
        assert_eq!(p.state, PairState::Calculating);
        assert_eq!((p.computed_at_utc_ms, p.conflict.is_none()), (None, true));
        assert!(
            !pairs
                .pairs()
                .any(|p| p.state == PairState::Current || p.state == PairState::Recalculating)
        );
    }

    #[test]
    fn a_pair_absent_from_the_plan_is_forgotten() {
        let mut pairs = RepoPairs::new("r");
        pairs.apply(vec![pred("t", PairState::Calculating)], 0);
        pairs.apply(Vec::new(), 1);
        assert_eq!(pairs.pairs().count(), 0);
    }
}
