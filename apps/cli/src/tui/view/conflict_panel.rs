//! The conflict panel under the fleet, from what the engine published and `prediction.list`
//! returned (DS-US-CKP-006, AD5 to AD7). Nothing is computed here: a pair that is not in the
//! publication is not drawn (P7), and a detail of another `rev` was already dropped by the model.

use gitraptor_api::prediction::{ConflictKindView, LineRange, PairLevel, PairStateView};

use super::{Say, repo, row_name};
use crate::model::{Model, SafeText};
use crate::present::prediction_text::PredictionText as T;
use crate::tui::fleet_order;
use crate::tui::prediction::{PairRow, PredictionModel, Side};
use crate::tui::widgets::conflict_alert::{AgentRef, AlertFile, ConflictEntry, ConflictPanelModel};

/// Line ranges shown per side of a file; the rest is "+N".
const RANGES_SHOWN: usize = 3;

/// The panel in at most `max_height` rows, or `None` without a publication (no capability).
pub(super) fn build(model: &Model, say: Say, max_height: u16) -> Option<ConflictPanelModel> {
    let repo = repo(model)?;
    let prediction = repo.prediction.as_ref()?;
    let listed = |p: &&PairRow| {
        matches!(p.level, Some(PairLevel::Conflict | PairLevel::Overlap))
            && matches!(
                p.state,
                PairStateView::Current | PairStateView::Recalculating
            )
    };
    let mut pairs: Vec<&PairRow> = prediction.pairs.iter().filter(listed).collect();
    // ⚡ first, then ⚠; the engine's order within each (stable).
    pairs.sort_by_key(|p| p.level != Some(PairLevel::Conflict));
    let entries: Vec<ConflictEntry> = pairs
        .iter()
        .map(|p| entry(model, repo, prediction, p, say))
        .collect();
    let (entries, hidden) = fit(entries, usize::from(max_height.saturating_sub(3)), say);
    Some(ConflictPanelModel {
        title: say.prediction(T::PanelTitle),
        entries,
        hidden,
        limits: [
            say.prediction(T::LineUncommitted),
            say.prediction(T::LineCommitted),
            say.prediction(T::LineDrivers),
        ],
    })
}

/// The pairs that fit `rows` content rows, each with as many files as there is room for, and
/// the "+N pairs more" row when some do not. One row is kept for it while pairs remain.
fn fit(entries: Vec<ConflictEntry>, rows: usize, say: Say) -> (Vec<ConflictEntry>, SafeText) {
    let total = entries.len();
    let mut left = rows;
    let mut shown = Vec::new();
    for (i, mut entry) in entries.into_iter().enumerate() {
        let reserve = usize::from(i + 1 < total);
        if left < 1 + reserve {
            return (shown, say.prediction(T::MorePairs(total - i)));
        }
        entry.files.truncate(left - 1 - reserve);
        left -= 1 + entry.files.len();
        shown.push(entry);
    }
    (shown, SafeText::default())
}

fn entry(
    model: &Model,
    repo: &crate::model::RepoView,
    prediction: &PredictionModel,
    pair: &PairRow,
    say: Say,
) -> ConflictEntry {
    let conflict = pair.level == Some(PairLevel::Conflict);
    ConflictEntry {
        conflict,
        left: side(model, repo, &pair.a, say),
        right: side(model, repo, &pair.b, say),
        status: (pair.state == PairStateView::Recalculating)
            .then(|| say.prediction(T::Recalculating)),
        files: files(prediction, pair, conflict, say),
    }
}

/// A side by the name that heads its row, or the base branch (P5, AD5). A root no row has is
/// named by its folder, never empty.
fn side(model: &Model, repo: &crate::model::RepoView, side: &Side, say: Say) -> AgentRef {
    match side {
        Side::Base => AgentRef {
            color_index: AgentRef::BASE,
            name: repo.base.clone().unwrap_or_else(|| say.prediction(T::Base)),
        },
        Side::Worktree { key, folder } => {
            let Some(worktree) = repo.worktrees.iter().find(|w| w.key == *key) else {
                return AgentRef {
                    color_index: 0,
                    name: folder.clone(),
                };
            };
            let shown = fleet_order::shown(
                &repo.sessions,
                worktree.key,
                model.now_ms,
                model.ui.show_ended,
            );
            let (name, state, color_index, _) = row_name(repo, worktree, &shown, model, say);
            // Without detection the row says "not available": the folder tells the sides apart.
            let name = if state == super::AgentState::Unknown {
                worktree.name.clone()
            } else {
                name
            };
            AgentRef { color_index, name }
        }
    }
}

fn files(prediction: &PredictionModel, pair: &PairRow, conflict: bool, say: Say) -> Vec<AlertFile> {
    let plain = |path: SafeText| AlertFile {
        path,
        detail: SafeText::default(),
        merge_rule: None,
    };
    let count = if conflict {
        pair.conflict_files
    } else {
        pair.overlap_files
    };
    // Only a detail of the summary's `rev` is shown; before it comes, the count, never old paths.
    let Some(detail) = prediction.details.get(&pair.pair_id) else {
        return vec![plain(say.prediction(T::Files(count)))];
    };
    let (mut files, partial): (Vec<AlertFile>, bool) = match (&detail.conflict, conflict) {
        (Some(c), true) => (
            c.files
                .iter()
                .map(|f| AlertFile {
                    path: f.path.clone(),
                    detail: SafeText::text(&clash(f.kind, &f.ours, &f.theirs, say)),
                    // The wire carries the mark only on ⚡ files (AD7).
                    merge_rule: f.has_merge_rule.then(|| say.prediction(T::MergeRule)),
                })
                .collect(),
            c.truncated || pair.conflict_truncated,
        ),
        _ => (
            detail.overlap.iter().cloned().map(plain).collect(),
            pair.overlap_partial,
        ),
    };
    if files.is_empty() {
        return vec![plain(say.prediction(T::Files(count)))];
    }
    if partial {
        files.push(plain(say.prediction(T::AndMore)));
    }
    files
}

/// How a file clashes: the ranges of each side side by side (`12–18 ↔ 14–20`, three at most and
/// "+N" for the rest), or the kind's name when it is not a content conflict.
fn clash(kind: ConflictKindView, ours: &[LineRange], theirs: &[LineRange], say: Say) -> String {
    if kind != ConflictKindView::Content {
        return say.prediction(T::Kind(kind)).as_str().to_owned();
    }
    let n = ours.len().max(theirs.len());
    let one = |r: Option<&LineRange>| match r {
        Some(r) if r.len > 1 => format!("{}–{}", r.start, r.start + r.len - 1),
        Some(r) => r.start.to_string(),
        None => say.glyphs.unknown.to_owned(),
    };
    let mut text = (0..n.min(RANGES_SHOWN))
        .map(|i| format!("{} ↔ {}", one(ours.get(i)), one(theirs.get(i))))
        .collect::<Vec<_>>()
        .join(", ");
    if n > RANGES_SHOWN {
        text.push(' ');
        text.push_str(say.prediction(T::MoreRanges(n - RANGES_SHOWN)).as_str());
    }
    say.glyphs.fold(&text).into_owned()
}
