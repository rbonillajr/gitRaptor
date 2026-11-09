//! How fresh each pair of the prediction is, and the lines that group the pairs without a
//! result. Pure functions over the model's `PairRow`s: no I/O, no clock of their own.
//!
//! A pair that is not `current` is never presented as having no conflicts.

use gitraptor_api::prediction::{PairLevel, PairReason, PairStateView};

use crate::present::freshness_text::{AgeUnit, FreshnessText};
use crate::present::i18n::age;
use crate::tui::prediction::PairRow;
use crate::tui::widgets::conflict_alert::Prediction;

/// The reasons in the order their lines are drawn.
const REASON_ORDER: [PairReason; 9] = [
    PairReason::BaseUnconfirmed,
    PairReason::OperationInProgress,
    PairReason::PowerSaving,
    PairReason::Dormant,
    PairReason::BaseMissing,
    PairReason::MissingObject,
    PairReason::Shallow,
    PairReason::LimitExceeded,
    PairReason::Error,
];

/// The age of a result computed at `computed_at_ms`, told in its largest whole unit. A clock
/// behind the result's gives 0.
pub fn age_of(now_ms: i64, computed_at_ms: i64) -> (i64, AgeUnit) {
    let (n, unit) = age(now_ms.saturating_sub(computed_at_ms));
    let unit = match unit {
        "s" => AgeUnit::Seconds,
        "min" => AgeUnit::Minutes,
        "h" => AgeUnit::Hours,
        _ => AgeUnit::Days,
    };
    (n, unit)
}

/// The state of a pair as the TUI presents it: a result without the time it was computed at is
/// no result to show, so the pair counts as calculating.
fn effective(row: &PairRow) -> PairStateView {
    match row.state {
        PairStateView::Current | PairStateView::Recalculating
            if row.computed_at_utc_ms.is_none() =>
        {
            PairStateView::Calculating
        }
        // A `current` result with no level, or a `pending` with no reason, says nothing the
        // TUI could show: it does not invent one (E4).
        PairStateView::Current if row.level.is_none() => PairStateView::Calculating,
        PairStateView::Pending if row.reason.is_none() => PairStateView::Calculating,
        state => state,
    }
}

fn pending_reason(row: &PairRow) -> PairReason {
    row.reason.unwrap_or(PairReason::OperationInProgress)
}

fn not_computable_reason(row: &PairRow) -> PairReason {
    row.reason.unwrap_or(PairReason::Error)
}

/// What the pair is for the widget's style.
pub fn prediction_of(row: &PairRow) -> Prediction {
    match effective(row) {
        PairStateView::Current => Prediction::Current,
        PairStateView::Recalculating => Prediction::Stale,
        PairStateView::Calculating => Prediction::Calculating,
        PairStateView::Pending => reason_prediction(pending_reason(row)),
        PairStateView::NotComputable => Prediction::NotComputable,
    }
}

fn reason_prediction(reason: PairReason) -> Prediction {
    match reason {
        PairReason::BaseUnconfirmed => Prediction::PendingBase,
        PairReason::OperationInProgress
        | PairReason::PowerSaving
        | PairReason::Dormant
        | PairReason::BaseMissing
        | PairReason::MissingObject
        | PairReason::Shallow
        | PairReason::LimitExceeded
        | PairReason::Error => Prediction::Pending,
    }
}

fn marked(level: Option<PairLevel>) -> bool {
    matches!(level, Some(PairLevel::Conflict | PairLevel::Overlap))
}

/// Whether the pair has a row of its own: a ⚡ or ⚠ result, or one being recalculated.
pub fn listed(row: &PairRow) -> bool {
    match effective(row) {
        PairStateView::Current => marked(row.level),
        PairStateView::Recalculating => true,
        PairStateView::Calculating | PairStateView::Pending | PairStateView::NotComputable => false,
    }
}

/// The order of the rows: ⚡, then ⚠, then the recalculating pairs without either.
pub fn row_rank(row: &PairRow) -> u8 {
    match row.level {
        Some(PairLevel::Conflict) => 0,
        Some(PairLevel::Overlap) => 1,
        Some(PairLevel::Clean) | None => 2,
    }
}

/// The text of a listed pair's row: its age, or "outdated … recalculating".
pub fn row_text(row: &PairRow, now_ms: i64) -> Option<FreshnessText> {
    let computed = row.computed_at_utc_ms?;
    let (n, unit) = age_of(now_ms, computed);
    match effective(row) {
        PairStateView::Current => Some(FreshnessText::Age { n, unit }),
        PairStateView::Recalculating => Some(FreshnessText::Stale { n, unit }),
        PairStateView::Calculating | PairStateView::Pending | PairStateView::NotComputable => None,
    }
}

/// The aggregate lines, in order: pairs calculating, one line per reason of the pending and the
/// not computable, and "no conflicts" only when every pair is current and clean.
pub fn summary(pairs: &[PairRow], now_ms: i64) -> Vec<(Prediction, FreshnessText)> {
    let mut lines = Vec::new();
    let count = |state: PairStateView| pairs.iter().filter(|p| effective(p) == state).count();
    let calculating = count(PairStateView::Calculating);
    if calculating >= 1 {
        lines.push((
            Prediction::Calculating,
            FreshnessText::Calculating {
                n: calculating,
                m: pairs.len(),
            },
        ));
    }
    for reason in REASON_ORDER {
        let n = pairs
            .iter()
            .filter(|p| effective(p) == PairStateView::Pending && pending_reason(p) == reason)
            .count();
        if n >= 1 {
            lines.push((
                reason_prediction(reason),
                FreshnessText::Pending { reason, n },
            ));
        }
    }
    for reason in REASON_ORDER {
        let n = pairs
            .iter()
            .filter(|p| {
                effective(p) == PairStateView::NotComputable && not_computable_reason(p) == reason
            })
            .count();
        if n >= 1 {
            lines.push((
                Prediction::NotComputable,
                FreshnessText::NotComputable { reason, n },
            ));
        }
    }
    let all_clear = !pairs.is_empty()
        && pairs
            .iter()
            .all(|p| effective(p) == PairStateView::Current && p.level == Some(PairLevel::Clean));
    if all_clear && let Some(oldest) = pairs.iter().filter_map(|p| p.computed_at_utc_ms).min() {
        let (n, unit) = age_of(now_ms, oldest);
        lines.push((Prediction::Current, FreshnessText::Clear { n, unit }));
    }
    lines
}

/// Some pair carries the time of its result, so the clock moves what the panel says.
pub fn has_ages(pairs: &[PairRow]) -> bool {
    pairs.iter().any(|p| p.computed_at_utc_ms.is_some())
}

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod tests;
