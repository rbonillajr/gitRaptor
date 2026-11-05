//! `mcp.conflicts`: where the caller is and the page of the pairs the predictor published for
//! it. Nothing is computed per request: only the published state is read.

use std::path::{Path, PathBuf};

use gitraptor_api::UntrustedName;
use gitraptor_api::mcp_view::MCP_CONFLICTS_PAGE_PAIRS;
use gitraptor_api::messages::RepoView;
use gitraptor_api::methods::{McpConflictFile, McpPair, McpPairConflict, McpPairCounts, McpSide};
use gitraptor_api::prediction::{
    ConflictFileView, LineRange, PairDetail, PairLevel, PairSideView, PairStateView,
};

use super::mcp_status::{self, Located};

/// Where the caller of `mcp.conflicts` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictsLocated {
    /// In worktree `worktree` of repo `repo`.
    Worktree { repo: usize, worktree: usize },
    /// Under the common directory of repo `repo` but under no worktree root: a bare repo with
    /// linked worktrees, or `--separate-git-dir`.
    RepoOnly { repo: usize },
    /// The worktree this connection last served was deleted.
    Missing { repo: usize },
    /// In no observed repo.
    Outside,
}

/// The scope of a call: `cwd` canonical or `None`, `last` the repo id and root last served on
/// this connection. A worktree root wins over a common directory that contains it.
pub(crate) fn locate_conflicts(
    cwd: Option<&Path>,
    repos: &[RepoView],
    last: Option<&(String, PathBuf)>,
) -> ConflictsLocated {
    match mcp_status::locate_with_last(cwd, repos, last) {
        Located::Worktree { repo, worktree } => ConflictsLocated::Worktree { repo, worktree },
        Located::Missing { repo } => ConflictsLocated::Missing { repo },
        Located::Outside => cwd
            .and_then(|cwd| {
                repos.iter().position(|r| {
                    let common = Path::new(r.path.raw());
                    // `starts_with` compares whole components.
                    common.is_absolute() && cwd.starts_with(common)
                })
            })
            .map_or(ConflictsLocated::Outside, |repo| {
                ConflictsLocated::RepoOnly { repo }
            }),
    }
}

/// The key of a pair in page order: its rank, then its `pair_id`.
pub(crate) type PageKey = (u8, String);

/// One page of pairs, each with its key, the counts of the whole scope, and whether more pairs
/// follow the page.
pub(crate) struct Page {
    pub counts: McpPairCounts,
    pub pairs: Vec<(PageKey, McpPair)>,
    pub more: bool,
}

/// The rank of a pair (DS § La forma): ⚡ current, ⚡ recalculating, ⚠ current, ⚠
/// recalculating, calculating, pending, not-computable, none.
fn rank(state: PairStateView, level: Option<PairLevel>) -> u8 {
    match (state, level) {
        (PairStateView::Current, Some(PairLevel::Conflict)) => 0,
        (PairStateView::Recalculating, Some(PairLevel::Conflict)) => 1,
        (PairStateView::Current, Some(PairLevel::Overlap)) => 2,
        (PairStateView::Recalculating, Some(PairLevel::Overlap)) => 3,
        (PairStateView::Current | PairStateView::Recalculating, Some(PairLevel::Clean)) => 7,
        (PairStateView::Pending, _) => 5,
        (PairStateView::NotComputable, _) => 6,
        // Calculating, or a result-bearing state with no result yet.
        _ => 4,
    }
}

fn is_caller(side: &PairSideView, caller: &Path) -> bool {
    matches!(side, PairSideView::Worktree { root } if Path::new(root.raw()) == caller)
}

fn side(view: &PairSideView, base: Option<&UntrustedName>) -> McpSide {
    match view {
        PairSideView::Worktree { root } => McpSide::Worktree(mcp_status::worktree_name(root.raw())),
        PairSideView::Base {} => McpSide::Base(base.cloned()),
    }
}

fn ranges(lines: &[LineRange]) -> Vec<[u32; 2]> {
    lines.iter().map(|r| [r.start, r.len]).collect()
}

fn file(view: &ConflictFileView, swap: bool) -> McpConflictFile {
    let (a, b) = if swap {
        (&view.theirs, &view.ours)
    } else {
        (&view.ours, &view.theirs)
    };
    McpConflictFile {
        path: view.path.clone(),
        kind: view.kind,
        a: ranges(a),
        b: ranges(b),
        hunks_truncated: view.hunks_truncated,
        merge_rule: view.has_merge_rule,
    }
}

/// The pair as the model gets it: the caller on side a (D14), `level` only with a result, no
/// state when current, the overlap only in a ⚠ and the files only in a ⚡ (D12).
fn pair(
    detail: &PairDetail,
    caller: Option<&Path>,
    base: Option<&UntrustedName>,
    now_ms: i64,
) -> McpPair {
    let s = &detail.summary;
    let swap = caller.is_some_and(|c| is_caller(&s.b, c) && !is_caller(&s.a, c));
    let (a, b) = if swap { (&s.b, &s.a) } else { (&s.a, &s.b) };
    let result = matches!(
        s.state,
        PairStateView::Current | PairStateView::Recalculating
    );
    let level = s.level.filter(|_| result);
    let reason = s.reason.filter(|_| {
        matches!(
            s.state,
            PairStateView::Pending | PairStateView::NotComputable
        )
    });
    let overlap = level == Some(PairLevel::Overlap);
    McpPair {
        a: side(a, base),
        b: side(b, base),
        state: (s.state != PairStateView::Current).then_some(s.state),
        reason,
        level,
        age_s: s
            .computed_at_utc_ms
            .map(|at| u64::try_from(now_ms.saturating_sub(at) / 1000).unwrap_or(0)),
        computed_at_utc_ms: s.computed_at_utc_ms,
        overlap: if overlap {
            detail.overlap.clone()
        } else {
            Vec::new()
        },
        overlap_partial: overlap && s.overlap_partial,
        conflict: detail
            .conflict
            .as_ref()
            .filter(|_| level == Some(PairLevel::Conflict))
            .map(|c| McpPairConflict {
                files: c.files.iter().map(|f| file(f, swap)).collect(),
                truncated: c.truncated,
            }),
    }
}

fn count(counts: &mut McpPairCounts, state: PairStateView, level: Option<PairLevel>) {
    let slot = match (state, level) {
        (PairStateView::Pending, _) => &mut counts.pending,
        (PairStateView::NotComputable, _) => &mut counts.not_computable,
        (PairStateView::Current | PairStateView::Recalculating, Some(PairLevel::Conflict)) => {
            &mut counts.conflict
        }
        (PairStateView::Current | PairStateView::Recalculating, Some(PairLevel::Overlap)) => {
            &mut counts.overlap
        }
        (PairStateView::Current | PairStateView::Recalculating, Some(PairLevel::Clean)) => {
            &mut counts.none
        }
        _ => &mut counts.calculating,
    };
    *slot = slot.saturating_add(1);
}

/// The page of `details` after `after` (at most [`MCP_CONFLICTS_PAGE_PAIRS`]), in page order,
/// with each pair's key. With `caller`, only the pairs that include it, the caller on side a.
pub(crate) fn page(
    details: &[PairDetail],
    caller: Option<&Path>,
    base: Option<&UntrustedName>,
    after: Option<&PageKey>,
    now_ms: i64,
) -> Page {
    let mut counts = McpPairCounts::default();
    let mut ordered: Vec<(PageKey, &PairDetail)> = details
        .iter()
        .filter(|d| caller.is_none_or(|c| is_caller(&d.summary.a, c) || is_caller(&d.summary.b, c)))
        .map(|d| {
            let s = &d.summary;
            count(&mut counts, s.state, s.level);
            ((rank(s.state, s.level), s.pair_id.clone()), d)
        })
        .collect();
    ordered.sort_by(|x, y| x.0.cmp(&y.0));
    let mut rest = ordered
        .into_iter()
        .filter(|(key, _)| after.is_none_or(|after| key > after));
    let pairs: Vec<_> = rest
        .by_ref()
        .take(MCP_CONFLICTS_PAGE_PAIRS)
        .map(|(key, d)| (key, pair(d, caller, base, now_ms)))
        .collect();
    Page {
        counts,
        pairs,
        more: rest.next().is_some(),
    }
}

/// Where the page after this one starts: after the last of the `kept` first pairs of `keys`,
/// when the cut kept fewer than the page had or more pairs follow the page; `None` otherwise.
pub(crate) fn next_key(keys: &[PageKey], kept: usize, more: bool) -> Option<PageKey> {
    if kept < keys.len() || more {
        kept.checked_sub(1).and_then(|last| keys.get(last)).cloned()
    } else {
        None
    }
}

/// The counts of the whole scope, the page after `after` and where the next one starts: the
/// keyless form of [`page`], for the tests of the order.
#[cfg(test)]
pub(crate) fn build_page(
    details: &[PairDetail],
    caller: Option<&Path>,
    base: Option<&UntrustedName>,
    after: Option<&PageKey>,
    now_ms: i64,
) -> (McpPairCounts, Vec<McpPair>, Option<PageKey>) {
    let page = page(details, caller, base, after, now_ms);
    let (keys, pairs): (Vec<_>, Vec<_>) = page.pairs.into_iter().unzip();
    let next = next_key(&keys, pairs.len(), page.more);
    (page.counts, pairs, next)
}

#[cfg(test)]
// The frozen criterion tests keep a helper none of them calls.
#[allow(dead_code)]
#[path = "mcp_conflicts_tests.rs"]
mod tests;
