//! The conflict prediction's wire types as the TUI keeps them: every path and root becomes
//! `SafeText` here, the only place that reads them raw (SEC-12). No file content exists on the
//! wire, so none is kept.

use gitraptor_api::prediction::{PairDetail, PairSideView, PairSummary};

use super::ingest;
use crate::model::SafeText;
use crate::tui::prediction::{ConflictDetail, ConflictFileRow, PairDetailRow, PairRow, Side};

fn side(view: &PairSideView) -> Side {
    match view {
        PairSideView::Worktree { root } => Side::Worktree {
            key: ingest::key(root.raw()),
            folder: SafeText::name(&ingest::leaf(root.raw())),
        },
        PairSideView::Base {} => Side::Base,
    }
}

pub fn pair_row(summary: &PairSummary) -> PairRow {
    PairRow {
        pair_id: summary.pair_id.clone(),
        rev: summary.rev,
        a: side(&summary.a),
        b: side(&summary.b),
        state: summary.state,
        reason: summary.reason,
        level: summary.level,
        computed_at_utc_ms: summary.computed_at_utc_ms,
        overlap_files: summary.overlap_files,
        overlap_partial: summary.overlap_partial,
        conflict_files: summary.conflict_files,
        conflict_truncated: summary.conflict_truncated,
    }
}

pub fn detail_row(detail: &PairDetail) -> PairDetailRow {
    PairDetailRow {
        rev: detail.summary.rev,
        overlap: detail
            .overlap
            .iter()
            .map(SafeText::from_untrusted)
            .collect(),
        conflict: detail.conflict.as_ref().map(|c| ConflictDetail {
            truncated: c.truncated,
            files: c
                .files
                .iter()
                .map(|f| ConflictFileRow {
                    path: SafeText::from_untrusted(&f.path),
                    kind: f.kind,
                    ours: f.ours.clone(),
                    theirs: f.theirs.clone(),
                    hunks_truncated: f.hunks_truncated,
                    has_merge_rule: f.has_merge_rule,
                })
                .collect(),
        }),
    }
}
