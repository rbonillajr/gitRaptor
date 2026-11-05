//! The TUI's model of the published conflict prediction (DS-US-CKP-006, AD2).

use std::collections::BTreeMap;

use gitraptor_api::prediction::{
    ConflictKindView, LimitCode, LineRange, PairLevel, PairReason, PairStateView,
};

use gitraptor_api::prediction::{PairDetail, PairSummary, PredictionPage, PredictionSummary};

use crate::model::{Cmd, Model, SafeText};
use crate::present::prediction as wire;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PredictionModel {
    pub limits: Vec<LimitCode>,
    pub pairs: Vec<PairRow>,
    pub truncated: bool,
    pub details: BTreeMap<String, PairDetailRow>,
    early: BTreeMap<String, PairDetailRow>,
    asked: BTreeMap<String, u64>,
    walk: Walk,
}

/// Where the walk over `prediction.list` pages stands when the summary was cut (AD4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Walk {
    /// Nothing to walk: the summary came whole, or the walk is over.
    #[default]
    Done,
    /// The first page has not been asked.
    Start,
    /// A page was asked and has not come back.
    Waiting,
    /// The next page is `cursor`.
    Next(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairRow {
    pub pair_id: String,
    pub rev: u64,
    pub a: Side,
    pub b: Side,
    pub state: PairStateView,
    pub reason: Option<PairReason>,
    pub level: Option<PairLevel>,
    pub computed_at_utc_ms: Option<i64>,
    pub overlap_files: u32,
    pub overlap_partial: bool,
    pub conflict_files: u32,
    pub conflict_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    Worktree { key: u64, folder: SafeText },
    Base,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairDetailRow {
    pub rev: u64,
    pub overlap: Vec<SafeText>,
    pub conflict: Option<ConflictDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictDetail {
    pub files: Vec<ConflictFileRow>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFileRow {
    pub path: SafeText,
    pub kind: ConflictKindView,
    pub ours: Vec<LineRange>,
    pub theirs: Vec<LineRange>,
    pub hunks_truncated: bool,
    pub has_merge_rule: bool,
}

/// Most `pair_ids` one `prediction.list` takes.
const BATCH: usize = gitraptor_api::prediction::MAX_LIST_PAIR_IDS;

/// The `prediction.list` commands the model still needs, and a note of them as asked.
pub fn wanted(model: &mut Model) -> Vec<Cmd> {
    let Some(data) = model.engine.repo.as_mut().and_then(|r| r.data.as_mut()) else {
        return Vec::new();
    };
    let Some(prediction) = data.prediction.as_mut() else {
        return Vec::new();
    };
    let repo_id = data.repo_id.clone();
    prediction.wanted(&repo_id)
}

impl PredictionModel {
    /// The model of a snapshot's summary. Nothing in flight carries over a snapshot.
    pub fn from_summary(summary: &PredictionSummary) -> Self {
        Self {
            limits: summary.limits.clone(),
            pairs: summary.pairs.iter().map(wire::pair_row).collect(),
            truncated: summary.pairs_truncated,
            walk: if summary.pairs_truncated {
                Walk::Start
            } else {
                Walk::Done
            },
            ..Self::default()
        }
    }

    /// Forgets what was asked: a snapshot, a resync or a reconnection asks again.
    pub fn forget_asked(&mut self) {
        self.asked.clear();
        if self.walk == Walk::Waiting {
            self.walk = Walk::Start;
        }
    }

    /// Applies one `prediction.changed`: replaces by `pair_id`, inserts and removes. A summary
    /// older than the kept one is ignored; a new `rev` invalidates the pair's detail.
    pub fn apply_changed(&mut self, pairs: &[PairSummary], removed: &[String]) {
        for id in removed {
            self.pairs.retain(|p| &p.pair_id != id);
            self.details.remove(id);
            self.early.remove(id);
            self.asked.remove(id);
        }
        for summary in pairs {
            let row = wire::pair_row(summary);
            match self.pairs.iter_mut().find(|p| p.pair_id == row.pair_id) {
                Some(kept) if kept.rev > row.rev => continue,
                Some(kept) => *kept = row,
                None => self.pairs.push(row),
            }
            self.settle(&summary.pair_id, summary.rev);
        }
    }

    /// What a pair's `rev` makes of its details: the stale one goes, an early one at that
    /// `rev` is adopted, an older early one is dropped.
    fn settle(&mut self, id: &str, rev: u64) {
        if self.details.get(id).is_some_and(|d| d.rev != rev) {
            self.details.remove(id);
        }
        if let Some(early) = self.early.remove(id)
            && early.rev >= rev
        {
            if early.rev == rev {
                self.details.insert(id.to_owned(), early);
            } else {
                self.early.insert(id.to_owned(), early);
            }
        }
    }

    /// Applies a page of `prediction.list`: details by their `rev` against the summary's, and,
    /// while walking, the summaries of pairs not yet known.
    pub fn apply_page(&mut self, page: &PredictionPage) {
        let walking = self.walk == Walk::Waiting;
        for detail in &page.pairs {
            self.apply_detail(detail, walking);
        }
        if walking {
            match &page.next_cursor {
                Some(cursor) => self.walk = Walk::Next(cursor.clone()),
                None => {
                    self.walk = Walk::Done;
                    self.truncated = false;
                }
            }
        }
    }

    fn apply_detail(&mut self, detail: &PairDetail, walking: bool) {
        let id = &detail.summary.pair_id;
        if !self.pairs.iter().any(|p| &p.pair_id == id) {
            if !walking {
                // The engine did not publish it: it does not exist here (P7).
                return;
            }
            self.pairs.push(wire::pair_row(&detail.summary));
        }
        let Some(rev) = self.pairs.iter().find(|p| &p.pair_id == id).map(|p| p.rev) else {
            return;
        };
        let row = wire::detail_row(detail);
        match detail.summary.rev.cmp(&rev) {
            std::cmp::Ordering::Equal => {
                self.early.remove(id);
                self.details.insert(id.clone(), row);
            }
            std::cmp::Ordering::Greater => {
                self.early.insert(id.clone(), row);
            }
            std::cmp::Ordering::Less => {}
        }
    }

    fn wanted(&mut self, repo_id: &str) -> Vec<Cmd> {
        let list = |pair_ids, cursor| Cmd::PredictionList {
            repo_id: repo_id.to_owned(),
            pair_ids,
            cursor,
        };
        // Pages and details share one reply type, so only one call is out while walking.
        match std::mem::replace(&mut self.walk, Walk::Waiting) {
            Walk::Start => return vec![list(Vec::new(), None)],
            Walk::Next(cursor) => return vec![list(Vec::new(), Some(cursor))],
            Walk::Waiting => return Vec::new(),
            Walk::Done => self.walk = Walk::Done,
        }
        let mut ids: Vec<(bool, &PairRow)> = self
            .pairs
            .iter()
            .filter(|p| {
                matches!(p.level, Some(PairLevel::Conflict | PairLevel::Overlap))
                    && matches!(
                        p.state,
                        PairStateView::Current | PairStateView::Recalculating
                    )
                    && self.details.get(&p.pair_id).is_none_or(|d| d.rev != p.rev)
                    && self.asked.get(&p.pair_id) != Some(&p.rev)
            })
            .map(|p| (p.level != Some(PairLevel::Conflict), p))
            .collect();
        // `false` (⚡) before `true` (⚠); stable, so the engine's order holds within each.
        ids.sort_by_key(|(overlap, _)| *overlap);
        let wanted: Vec<(String, u64)> = ids
            .into_iter()
            .map(|(_, p)| (p.pair_id.clone(), p.rev))
            .collect();
        wanted
            .chunks(BATCH)
            .map(|chunk| {
                self.asked.extend(chunk.iter().cloned());
                list(chunk.iter().map(|(id, _)| id.clone()).collect(), None)
            })
            .collect()
    }

    /// Some pair that includes the worktree has a known conflict: `conflict` in `current` or
    /// `recalculating`. A pair without a level claims nothing.
    pub fn known_conflict(&self, key: u64) -> bool {
        self.pairs.iter().any(|p| {
            p.level == Some(PairLevel::Conflict)
                && matches!(
                    p.state,
                    PairStateView::Current | PairStateView::Recalculating
                )
                && [&p.a, &p.b]
                    .iter()
                    .any(|s| matches!(s, Side::Worktree { key: k, .. } if *k == key))
        })
    }
}
