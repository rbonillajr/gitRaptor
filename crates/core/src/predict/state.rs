//! The pairs of one repo across plans: their state, the cache keyed by triple and the rule that
//! a result of inputs that already changed is never current.

use std::collections::BTreeMap;

use gitraptor_git::dry_merge::{DryMergeError, DryMergeOutcome};

use super::{NotComputableReason, PairKey, PairPrediction, PairState, PendingReason, Triple};

struct Entry {
    pred: PairPrediction,
    /// The triple last queued or computed: an equal one queues nothing.
    cached: Option<Triple>,
    /// The triple of the kept result and the state that result gave.
    done: Option<(Triple, PairState)>,
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
    /// whose triple equals the cached one queues nothing; one with a new triple is queued and
    /// becomes `Recalculating` if it had a result. A `Pending` or `NotComputable` pair of the plan
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
                done: None,
            });
            entry.pred.overlap = p.overlap;
            entry.pred.triple.clone_from(&p.triple);
            entry.pred.state = match (p.state, p.triple) {
                (PairState::Calculating, Some(triple)) => {
                    if let Some((_, state)) = entry.done.as_ref().filter(|(t, _)| *t == triple) {
                        entry.cached = Some(triple);
                        *state
                    } else if entry.cached.as_ref() == Some(&triple) {
                        entry.queued_state()
                    } else {
                        entry.cached = Some(triple.clone());
                        queue.push((p.key.clone(), triple));
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
        entry.pred.conflict = conflict;
        entry.pred.computed_at_utc_ms = Some(now_utc_ms);
        entry.done = Some((triple.clone(), state));
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
                entry.done = None;
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
    use crate::predict::{Overlap, PairSide};

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

    #[test]
    fn a_pair_absent_from_the_plan_is_forgotten() {
        let mut pairs = RepoPairs::new("r");
        pairs.apply(vec![pred("t", PairState::Calculating)], 0);
        pairs.apply(Vec::new(), 1);
        assert_eq!(pairs.pairs().count(), 0);
    }
}
