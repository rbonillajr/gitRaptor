//! What the predictor published, as the channel reads it: per repo, every pair's detail with
//! the `rev` of the `prediction.changed` that last changed it, and the `attention.conflicts` of
//! the last `repo.attention`.
//!
//! Only the predictor's thread writes it, and only inside the event bus's publication of the
//! event that carries the change, so a snapshot read under the same lock never disagrees with
//! the stream. Paths travel as untrusted text and no file content is ever kept.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use gitraptor_api::prediction::{
    ConflictFileView, ConflictKindView, ConflictView, LineRange, MergeBaseKind, PairDetail,
    PairLevel, PairReason, PairSideView, PairStateView, PairSummary, pair_id,
};
use gitraptor_api::scope::{AttentionCount, AttentionView, UnavailableCause};
use gitraptor_api::untrusted::Untrusted;
use gitraptor_git::dry_merge::{ConflictKind, DryMergeLimits, DryMergeOutcome, MergeBaseUsed};

use super::{NotComputableReason, PairKey, PairPrediction, PairSide, PairState, PendingReason};

/// The published state of one repo.
#[derive(Default)]
struct Published {
    /// In page order: the order of [`PairKey`].
    pairs: BTreeMap<PairKey, PairDetail>,
    /// The `attention.conflicts` of the last `repo.attention`; `None` before the first.
    conflicts: Option<AttentionCount>,
}

/// The predictor's publication, shared by its thread and the channel's connections.
pub struct PredictionStore {
    /// Whether the predictor runs: without it nothing is ever published.
    active: bool,
    repos: Mutex<HashMap<String, Published>>,
}

impl PredictionStore {
    pub fn new(active: bool) -> Self {
        Self {
            active,
            repos: Mutex::new(HashMap::new()),
        }
    }

    /// Whether the predictor runs and publishes.
    pub fn active(&self) -> bool {
        self.active
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Published>> {
        self.repos.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The summaries of `repo_id`'s pairs, in page order.
    pub fn summaries(&self, repo_id: &str) -> Vec<PairSummary> {
        self.lock().get(repo_id).map_or_else(Vec::new, |p| {
            p.pairs.values().map(|d| d.summary.clone()).collect()
        })
    }

    /// The details of `repo_id`'s pairs by key, as published.
    pub(crate) fn details(&self, repo_id: &str) -> BTreeMap<PairKey, PairDetail> {
        self.lock()
            .get(repo_id)
            .map(|p| p.pairs.clone())
            .unwrap_or_default()
    }

    /// The attention of `repo_id` as the last `repo.attention` left it: only the conflicts are
    /// the predictor's, and they are "not published" until its first.
    pub fn attention(&self, repo_id: &str) -> AttentionView {
        let mut view = AttentionView::unpublished();
        if let Some(conflicts) = self.lock().get(repo_id).and_then(|p| p.conflicts) {
            view.conflicts = conflicts;
        }
        view
    }

    /// Records a batch: `changed` replaces the details of its pairs and `removed` drops pairs.
    pub(crate) fn record(
        &self,
        repo_id: &str,
        changed: Vec<(PairKey, PairDetail)>,
        removed: &[PairKey],
    ) {
        let mut repos = self.lock();
        let published = repos.entry(repo_id.to_owned()).or_default();
        for key in removed {
            published.pairs.remove(key);
        }
        published.pairs.extend(changed);
    }

    /// Records the `attention.conflicts` a `repo.attention` published.
    pub(crate) fn record_conflicts(&self, repo_id: &str, conflicts: AttentionCount) {
        self.lock().entry(repo_id.to_owned()).or_default().conflicts = Some(conflicts);
    }

    /// Forgets a retired repo.
    pub(crate) fn forget(&self, repo_id: &str) {
        self.lock().remove(repo_id);
    }
}

/// The published pairs of `repo_id` in page order, with their `rev`; with `worktree`, only the
/// pairs that include that root. Empty for a repo the predictor has not published.
pub(crate) fn predict_read(
    store: &PredictionStore,
    repo_id: &str,
    worktree: Option<&Path>,
) -> Vec<PairDetail> {
    let includes = |d: &PairDetail| {
        worktree.is_none_or(|w| {
            [&d.summary.a, &d.summary.b].into_iter().any(|side| {
                matches!(side, PairSideView::Worktree { root } if Path::new(root.raw()) == w)
            })
        })
    };
    store
        .lock()
        .get(repo_id)
        .map(|p| p.pairs.values().filter(|d| includes(d)).cloned().collect())
        .unwrap_or_default()
}

/// The hybrid `attention.conflicts` of `pairs`: the known conflicts when there is one, a counted
/// 0 only when no pair is still calculating, "not published" otherwise.
pub fn conflicts<'a>(pairs: impl IntoIterator<Item = &'a PairSummary>) -> AttentionCount {
    let mut count = 0_u32;
    let mut calculating = false;
    for p in pairs {
        if p.level == Some(PairLevel::Conflict)
            && matches!(
                p.state,
                PairStateView::Current | PairStateView::Recalculating
            )
        {
            count = count.saturating_add(1);
        }
        calculating |= p.state == PairStateView::Calculating;
    }
    if count == 0 && calculating {
        AttentionCount::Unavailable {
            reason: UnavailableCause::NotPublished,
        }
    } else {
        AttentionCount::Counted { count }
    }
}

/// The detail of `pred` as the wire carries it, with `rev`.
pub(crate) fn detail(pred: &PairPrediction, rev: u64) -> PairDetail {
    let max_files = DryMergeLimits::default().max_files;
    let (state, reason) = state_view(pred.state);
    let conflict = pred.conflict.as_ref();
    let overlap = &pred.overlap.files;
    let level = conflict
        .filter(|_| matches!(state, PairStateView::Current | PairStateView::Recalculating))
        .map(|c| {
            if !c.files.is_empty() || c.truncated {
                PairLevel::Conflict
            } else if !overlap.is_empty() || pred.overlap.partial {
                PairLevel::Overlap
            } else {
                PairLevel::Clean
            }
        });
    let side = |p: &Path| PairSideView::Worktree {
        root: Untrusted::from_os(p.as_os_str()),
    };
    let b = match &pred.key.b {
        PairSide::Base => None,
        PairSide::Worktree(b) => Some(b.as_path()),
    };
    PairDetail {
        summary: PairSummary {
            pair_id: pair_id(&pred.key.repo_id, &pred.key.a, b),
            rev,
            a: side(&pred.key.a),
            b: b.map_or(PairSideView::Base {}, side),
            state,
            reason,
            level,
            computed_at_utc_ms: pred.computed_at_utc_ms,
            overlap_files: u32::try_from(overlap.len()).unwrap_or(u32::MAX),
            overlap_partial: pred.overlap.partial || overlap.len() > max_files,
            conflict_files: conflict
                .map_or(0, |c| u32::try_from(c.files.len()).unwrap_or(u32::MAX)),
            conflict_truncated: conflict.is_some_and(|c| c.truncated),
        },
        overlap: overlap
            .iter()
            .take(max_files)
            .map(|f| Untrusted::new(f.as_str()))
            .collect(),
        conflict: conflict.map(conflict_view),
    }
}

fn state_view(state: PairState) -> (PairStateView, Option<PairReason>) {
    match state {
        PairState::Calculating => (PairStateView::Calculating, None),
        PairState::Current => (PairStateView::Current, None),
        PairState::Recalculating => (PairStateView::Recalculating, None),
        PairState::Pending(r) => (
            PairStateView::Pending,
            Some(match r {
                PendingReason::BaseUnconfirmed => PairReason::BaseUnconfirmed,
                PendingReason::OperationInProgress => PairReason::OperationInProgress,
                PendingReason::PowerSaving => PairReason::PowerSaving,
                PendingReason::Dormant => PairReason::Dormant,
            }),
        ),
        PairState::NotComputable(r) => (
            PairStateView::NotComputable,
            Some(match r {
                NotComputableReason::BaseMissing => PairReason::BaseMissing,
                NotComputableReason::MissingObject => PairReason::MissingObject,
                NotComputableReason::Shallow => PairReason::Shallow,
                NotComputableReason::LimitExceeded => PairReason::LimitExceeded,
                NotComputableReason::Error => PairReason::Error,
            }),
        ),
    }
}

/// The files of a result: paths and line ranges, never content.
fn conflict_view(outcome: &DryMergeOutcome) -> ConflictView {
    let ranges = |r: &[gitraptor_git::dry_merge::LineRange]| {
        r.iter()
            .map(|r| LineRange {
                start: r.start,
                len: r.len,
            })
            .collect()
    };
    ConflictView {
        merge_base: match outcome.merge_base {
            MergeBaseUsed::Single(_) => MergeBaseKind::Single,
            MergeBaseUsed::Virtual { .. } => MergeBaseKind::Virtual,
            MergeBaseUsed::None => MergeBaseKind::Absent,
        },
        files: outcome
            .files
            .iter()
            .map(|f| ConflictFileView {
                path: Untrusted::new(f.path.as_str()),
                kind: match f.kind {
                    ConflictKind::Content => ConflictKindView::Content,
                    ConflictKind::AddAdd => ConflictKindView::AddAdd,
                    ConflictKind::ModifyDelete => ConflictKindView::ModifyDelete,
                    ConflictKind::Rename => ConflictKindView::Rename,
                    ConflictKind::DirectoryFile => ConflictKindView::DirectoryFile,
                    ConflictKind::Binary => ConflictKindView::Binary,
                    ConflictKind::Submodule => ConflictKindView::Submodule,
                },
                ours: ranges(&f.ours),
                theirs: ranges(&f.theirs),
                hunks_truncated: f.hunks_truncated,
                has_merge_rule: f.has_merge_rule,
            })
            .collect(),
        truncated: outcome.truncated,
        rewrites_escalated: outcome.rewrites_escalated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(state: PairStateView, level: Option<PairLevel>) -> PairSummary {
        PairSummary {
            pair_id: "0000000000000000".into(),
            rev: 0,
            a: PairSideView::Base {},
            b: PairSideView::Base {},
            state,
            reason: None,
            level,
            computed_at_utc_ms: None,
            overlap_files: 0,
            overlap_partial: false,
            conflict_files: 0,
            conflict_truncated: false,
        }
    }

    #[test]
    fn conflicts_count_known_ones_and_never_a_zero_while_calculating() {
        use PairStateView::{Calculating, Current, Recalculating};
        let conflict = Some(PairLevel::Conflict);
        let unknown = AttentionCount::Unavailable {
            reason: UnavailableCause::NotPublished,
        };
        let c = |p: &[PairSummary]| conflicts(p);
        assert_eq!(
            c(&[summary(Recalculating, conflict), summary(Calculating, None)]),
            AttentionCount::Counted { count: 1 }
        );
        assert_eq!(c(&[summary(Calculating, None)]), unknown);
        assert_eq!(
            c(&[summary(Current, Some(PairLevel::Clean))]),
            AttentionCount::Counted { count: 0 }
        );
    }
}
