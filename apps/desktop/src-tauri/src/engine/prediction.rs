//! Per-repo bookkeeping of the predicted pairs: which ones still lack their detail.

use std::collections::HashMap;

use gitraptor_api::prediction::{
    PairDetail, PairLevel, PairStateView, PairSummary, PredictionChanged, PredictionSummary,
};

/// Most ids one `prediction.list` call may carry.
const MAX_IDS: usize = 50;

/// What the tracker keeps of one pair.
#[derive(Debug)]
struct Entry {
    rev: u64,
    level: Option<PairLevel>,
    state: PairStateView,
    /// The `rev` of the detail already received.
    detail_rev: Option<u64>,
    /// The `rev` the detail was last asked for: a refused or empty answer is not retried until the
    /// `rev` moves.
    asked_rev: Option<u64>,
}

impl Entry {
    fn new(summary: &PairSummary) -> Self {
        Self {
            rev: summary.rev,
            level: summary.level,
            state: summary.state,
            detail_rev: None,
            asked_rev: None,
        }
    }

    fn follow(&mut self, summary: &PairSummary) {
        self.rev = summary.rev;
        self.level = summary.level;
        self.state = summary.state;
    }

    /// A conflict or overlap with a result the user can look at.
    fn showable(&self) -> bool {
        matches!(self.level, Some(PairLevel::Conflict | PairLevel::Overlap))
            && matches!(
                self.state,
                PairStateView::Current | PairStateView::Recalculating
            )
    }

    fn lacks_detail(&self) -> bool {
        self.showable() && self.detail_rev != Some(self.rev) && self.asked_rev != Some(self.rev)
    }
}

/// The `rev`, level and state of each pair, and the detail fetched for it.
#[derive(Debug, Default)]
pub struct PredictionTracker {
    pairs: HashMap<String, Entry>,
    /// The snapshot was truncated and its cursor walk has not been attempted yet.
    walk_pending: bool,
}

impl PredictionTracker {
    /// Starts again from a snapshot; `None` when the repo carries no prediction.
    pub fn reset(&mut self, summary: Option<&PredictionSummary>) {
        self.pairs.clear();
        self.walk_pending = summary.is_some_and(|s| s.pairs_truncated);
        for pair in summary.map(|s| s.pairs.as_slice()).unwrap_or_default() {
            self.pairs.insert(pair.pair_id.clone(), Entry::new(pair));
        }
    }

    /// Follows a `prediction.changed`: a pair is replaced only by one of the same or a newer `rev`.
    pub fn apply(&mut self, changed: &PredictionChanged) {
        for pair in &changed.pairs {
            self.upsert(pair);
        }
        for id in &changed.removed {
            self.pairs.remove(id);
        }
    }

    /// Records the details received; a detail newer than what is known brings its own summary.
    pub fn adopt(&mut self, details: &[PairDetail]) {
        for detail in details {
            let entry = self.upsert(&detail.summary);
            if entry.rev == detail.summary.rev {
                entry.detail_rev = Some(detail.summary.rev);
            }
        }
    }

    fn upsert(&mut self, summary: &PairSummary) -> &mut Entry {
        let entry = self
            .pairs
            .entry(summary.pair_id.clone())
            .or_insert_with(|| Entry::new(summary));
        if summary.rev >= entry.rev {
            entry.follow(summary);
        }
        entry
    }

    /// Ids of the conflict and overlap pairs whose detail is missing, conflicts first, at most 50.
    pub fn wanted(&self) -> Vec<String> {
        let mut ids: Vec<(bool, &String)> = self
            .pairs
            .iter()
            .filter(|(_, entry)| entry.lacks_detail())
            .map(|(id, entry)| (entry.level != Some(PairLevel::Conflict), id))
            .collect();
        ids.sort();
        ids.into_iter()
            .take(MAX_IDS)
            .map(|(_, id)| id.clone())
            .collect()
    }

    /// Marks `ids` as asked at their current `rev`.
    pub fn asked(&mut self, ids: &[String]) {
        for id in ids {
            if let Some(entry) = self.pairs.get_mut(id) {
                entry.asked_rev = Some(entry.rev);
            }
        }
    }

    /// The snapshot was truncated and the cursor walk has not finished.
    pub fn needs_fill(&self) -> bool {
        self.walk_pending
    }

    /// The walk was attempted (it finished, was cut or was refused): it is not started again for
    /// this snapshot.
    pub fn walk_done(&mut self) {
        self.walk_pending = false;
    }

    /// There is something to ask the engine.
    pub fn has_work(&self) -> bool {
        self.walk_pending || self.pairs.values().any(Entry::lacks_detail)
    }
}
