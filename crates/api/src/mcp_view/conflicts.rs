//! The `check_conflicts` tool's result: `mcp.conflicts` without what the model never needs, and
//! the cut of a page to its budget.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{MAX_MCP_PATH_BYTES, MCP_CURSOR_PLACEHOLDER};
use crate::methods::{
    McpConflictFile, McpConflicts, McpConflictsScope, McpPair, McpPairConflict, McpPairCounts,
    McpScopeReason, McpSide,
};
use crate::prediction::LimitCode;
use crate::untrusted::{Untrusted, UntrustedName};

/// The most pairs of one page; the byte budget usually cuts before.
pub const MCP_CONFLICTS_PAGE_PAIRS: usize = 50;

/// [`McpConflicts`] without `repo_id` nor `repo_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpConflictsView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<UntrustedName>,
    pub scope: McpConflictsScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<McpScopeReason>,
    pub limits: Vec<LimitCode>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unpublished: bool,
    /// Opaque in the output schema (RES-MCP-01).
    #[schemars(schema_with = "super::opaque_object")]
    pub counts: McpPairCounts,
    /// Opaque in the output schema (RES-MCP-01).
    #[schemars(schema_with = "opaque_array")]
    pub pairs: Vec<McpPair>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl McpConflictsView {
    /// The view the tool gives: names cut at [`MAX_MCP_NAME_CHARS`](super::MAX_MCP_NAME_CHARS) characters and paths at
    /// [`MAX_MCP_PATH_BYTES`]. Escaping is [`for_mcp`](super::for_mcp).
    pub fn for_mcp(c: &McpConflicts) -> Self {
        Self {
            worktree: c.worktree.as_ref().map(UntrustedName::mcp_name),
            scope: c.scope,
            reason: c.reason,
            limits: c.limits.clone(),
            unpublished: c.unpublished,
            counts: c.counts,
            pairs: c.pairs.iter().map(capped_pair).collect(),
            truncated: c.truncated,
            cursor: c.cursor.clone(),
        }
    }
}

fn capped_side(side: &McpSide) -> McpSide {
    match side {
        McpSide::Worktree(name) => McpSide::Worktree(name.mcp_name()),
        McpSide::Base(name) => McpSide::Base(name.as_ref().map(UntrustedName::mcp_name)),
    }
}

fn capped_path(path: &Untrusted) -> Untrusted {
    path.capped(MAX_MCP_PATH_BYTES)
}

fn capped_pair(pair: &McpPair) -> McpPair {
    McpPair {
        a: capped_side(&pair.a),
        b: capped_side(&pair.b),
        overlap: pair.overlap.iter().map(capped_path).collect(),
        conflict: pair.conflict.as_ref().map(|c| McpPairConflict {
            files: c
                .files
                .iter()
                .map(|f| McpConflictFile {
                    path: capped_path(&f.path),
                    ..f.clone()
                })
                .collect(),
            truncated: c.truncated,
        }),
        ..pair.clone()
    }
}

/// Bytes of `c` as the tool sends it: its view after [`for_mcp`](super::for_mcp), compact JSON.
pub fn conflicts_wire_len(c: &McpConflicts) -> usize {
    let mut value = serde_json::to_value(McpConflictsView::for_mcp(c)).unwrap_or(Value::Null);
    super::for_mcp(&mut value);
    value.to_string().len()
}

/// The longest `keep` in `0..=items` for which `cut(keep)` measures within `budget`, by
/// bisection; `floor` when not even that fits.
fn longest_fit(items: usize, floor: usize, budget: usize, cut: impl Fn(usize) -> usize) -> usize {
    let (mut low, mut high) = (floor.min(items), items);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if cut(mid) <= budget {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

/// Takes pairs off the end of `c` until it measures within `target`, keeping always the first
/// one, and leaves `truncated` and [`MCP_CURSOR_PLACEHOLDER`] when it did. A first pair that
/// alone is over `hard` is cut in its files (a ⚡) or its overlap list (a ⚠), and marked.
pub fn fit_conflicts(c: &mut McpConflicts, target: usize, hard: usize) {
    if conflicts_wire_len(c) <= target {
        return;
    }
    let total = c.pairs.len();
    let with_pairs = |c: &McpConflicts, keep: usize| {
        let mut cut = c.clone();
        cut.pairs.truncate(keep);
        cut.truncated = true;
        cut.cursor = Some(MCP_CURSOR_PLACEHOLDER.to_owned());
        cut
    };
    if total > 1 {
        let keep = longest_fit(total - 1, 1, target, |k| {
            conflicts_wire_len(&with_pairs(c, k))
        });
        *c = with_pairs(c, keep);
    }
    if conflicts_wire_len(c) <= hard {
        return;
    }
    let Some(first) = c.pairs.first() else {
        return;
    };
    if let Some(conflict) = &first.conflict {
        let files = conflict.files.len();
        let with_files = |c: &McpConflicts, keep: usize| {
            let mut cut = c.clone();
            if let Some(conflict) = cut.pairs[0].conflict.as_mut() {
                conflict.files.truncate(keep);
                conflict.truncated = true;
            }
            cut
        };
        let keep = longest_fit(files, 0, hard, |k| conflicts_wire_len(&with_files(c, k)));
        *c = with_files(c, keep);
    } else {
        let paths = first.overlap.len();
        let with_paths = |c: &McpConflicts, keep: usize| {
            let mut cut = c.clone();
            cut.pairs[0].overlap.truncate(keep);
            cut.pairs[0].overlap_partial = true;
            cut
        };
        let keep = longest_fit(paths, 0, hard, |k| conflicts_wire_len(&with_paths(c, k)));
        *c = with_paths(c, keep);
    }
}

/// `{"type":"array"}`: spelled out, the pairs would cost more than the budget of the schema.
fn opaque_array(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": "array"})
}
