//! The pairs of a repo with their triple and their overlap.
//!
//! A worktree's own work is what changed between its merge-base with the base and its tip, plus
//! its uncommitted paths; without own work it forms no pairs. Each worktree with own work pairs
//! with the base (unless it has the base checked out) and with every other worktree with own
//! work.

use std::time::{Duration, Instant};

use gitraptor_api::messages::BaseStatusView;
use gitraptor_git::{ChangedPaths, ReadError, RepoReader};

use super::overlap::{OwnWork, intersect};
use super::{
    BaseInput, NotComputableReason, Overlap, PairKey, PairPrediction, PairSide, PairState,
    PendingReason, Triple, WorktreeInput,
};
use crate::observe::MAX_UNCOMMITTED_FOR_OVERLAP;

/// Wall time every tree comparison of one plan shares. A plan that runs out fails: never a
/// made-up empty set of paths.
pub const PLAN_READ_BUDGET: Duration = Duration::from_secs(5);

/// One worktree with what the plan read for it.
struct Side<'a> {
    input: &'a WorktreeInput,
    own: OwnWork,
    /// Its merge-base with the base and what the base changed since: `None` without a base.
    base: Option<(Option<String>, OwnWork)>,
}

/// The pairs of `repo_id`, each in its initial state: `Calculating` when its triple can be
/// merged, `Pending` or `NotComputable` otherwise. The overlap is computed for every pair.
///
/// A base `Invalid` or without a tip leaves every pair `NotComputable(BaseMissing)`: own work is
/// counted from the base, so without one the committed part is unknown and each side is partial.
///
/// # Errors
/// The [`ReadError`] of a merge-base or a tree comparison that could not be read, or that did
/// not end within [`PLAN_READ_BUDGET`].
pub fn plan(
    reader: &RepoReader,
    repo_id: &str,
    base: &BaseInput,
    wts: &[WorktreeInput],
) -> Result<Vec<PairPrediction>, ReadError> {
    let deadline = Instant::now() + PLAN_READ_BUDGET;
    let base_tip = match (base.status, &base.tip) {
        (BaseStatusView::Invalid, _) | (_, None) => None,
        (_, Some(tip)) => Some(tip.as_str()),
    };
    let mut sides = Vec::with_capacity(wts.len());
    for input in wts {
        sides.push(side(reader, input, base_tip, deadline)?);
    }
    sides.retain(|s| !s.own.is_empty());
    sides.sort_by(|x, y| {
        let key = |s: &Side<'_>| s.input.root.as_os_str().as_encoded_bytes().to_vec();
        key(x).cmp(&key(y))
    });

    let mut out = Vec::new();
    for s in &sides {
        if base.checked_out_at.as_deref() == Some(s.input.root.as_path()) {
            continue;
        }
        let triple = match (&s.input.tip, base_tip, &s.base) {
            (Some(tip), Some(base_tip), Some((merge_base, _))) => Some(Triple {
                tip_a: tip.clone(),
                tip_b: base_tip.to_owned(),
                merge_base: merge_base.clone(),
            }),
            _ => None,
        };
        let state = if base_tip.is_none() {
            PairState::NotComputable(NotComputableReason::BaseMissing)
        } else if s.input.in_progress {
            PairState::Pending(PendingReason::OperationInProgress)
        } else if base.status == BaseStatusView::Unconfirmed {
            PairState::Pending(PendingReason::BaseUnconfirmed)
        } else if triple.is_none() {
            PairState::NotComputable(NotComputableReason::Error)
        } else {
            PairState::Calculating
        };
        let overlap = match &s.base {
            Some((_, changed)) => intersect(&s.own, changed),
            None => Overlap {
                files: Vec::new(),
                partial: true,
            },
        };
        out.push(prediction(
            repo_id,
            s,
            PairSide::Base,
            triple,
            state,
            overlap,
        ));
    }
    for (i, a) in sides.iter().enumerate() {
        for b in &sides[i + 1..] {
            let triple = match (&a.input.tip, &b.input.tip) {
                (Some(ta), Some(tb)) if base_tip.is_some() => Some(Triple {
                    tip_a: ta.clone(),
                    tip_b: tb.clone(),
                    merge_base: reader.merge_bases(ta, tb)?.into_iter().next(),
                }),
                _ => None,
            };
            let state = if base_tip.is_none() {
                PairState::NotComputable(NotComputableReason::BaseMissing)
            } else if a.input.in_progress || b.input.in_progress {
                PairState::Pending(PendingReason::OperationInProgress)
            } else if triple.is_none() {
                PairState::NotComputable(NotComputableReason::Error)
            } else {
                PairState::Calculating
            };
            let overlap = intersect(&a.own, &b.own);
            let other = PairSide::Worktree(b.input.root.clone());
            out.push(prediction(repo_id, a, other, triple, state, overlap));
        }
    }
    Ok(out)
}

fn prediction(
    repo_id: &str,
    a: &Side<'_>,
    b: PairSide,
    triple: Option<Triple>,
    state: PairState,
    overlap: Overlap,
) -> PairPrediction {
    PairPrediction {
        key: PairKey {
            repo_id: repo_id.to_owned(),
            a: a.input.root.clone(),
            b,
        },
        triple,
        state,
        computed_at_utc_ms: None,
        overlap,
        conflict: None,
    }
}

fn side<'a>(
    reader: &RepoReader,
    input: &'a WorktreeInput,
    base_tip: Option<&str>,
    deadline: Instant,
) -> Result<Side<'a>, ReadError> {
    let mut own = OwnWork {
        paths: (*input.uncommitted.paths).clone(),
        partial: input.uncommitted.partial,
    };
    let Some(base_tip) = base_tip else {
        own.partial = true;
        return Ok(Side {
            input,
            own,
            base: None,
        });
    };
    let max = MAX_UNCOMMITTED_FOR_OVERLAP;
    let merge_base = match &input.tip {
        Some(tip) => {
            let merge_base = reader.merge_bases(tip, base_tip)?.into_iter().next();
            let committed =
                changed_work(reader.changed_paths(merge_base.as_deref(), tip, max, deadline)?);
            own.paths.extend(committed.paths);
            own.partial |= committed.partial;
            merge_base
        }
        None => None,
    };
    let base_changed =
        changed_work(reader.changed_paths(merge_base.as_deref(), base_tip, max, deadline)?);
    Ok(Side {
        input,
        own,
        base: Some((merge_base, base_changed)),
    })
}

/// The paths of a comparison; partial when it found more than it kept.
fn changed_work(changed: ChangedPaths) -> OwnWork {
    let kept = changed.paths.len();
    OwnWork {
        partial: usize::try_from(changed.total).map_or(true, |total| total > kept),
        paths: changed.paths.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_paths_over_its_max_gives_a_partial_overlap_never_an_empty_one() {
        let cut = changed_work(ChangedPaths {
            paths: vec!["a.txt".into()],
            total: 3,
            first_parent: false,
        });
        assert!(cut.partial);
        let other = OwnWork {
            paths: ["b.txt".to_owned()].into(),
            partial: false,
        };
        let overlap = intersect(&other, &cut);
        assert!(overlap.files.is_empty());
        assert!(
            overlap.partial,
            "a cut comparison never reads as no overlap"
        );

        let whole = changed_work(ChangedPaths {
            paths: vec!["a.txt".into()],
            total: 1,
            first_parent: false,
        });
        assert!(!whole.partial);
    }

    #[test]
    fn a_cut_comparison_alone_is_own_work() {
        let cut = changed_work(ChangedPaths {
            paths: Vec::new(),
            total: 1,
            first_parent: false,
        });
        assert!(!cut.is_empty());
    }
}
