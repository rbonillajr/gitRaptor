//! `mcp.conflicts`: the conflicts the predictor published for the caller's worktree (or its
//! whole repo), shaped for the model. Files and line ranges, never content; the repo and the
//! worktree come from the caller's working folder, never from a parameter.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::messages::RepoStateView;
use crate::prediction::{ConflictKindView, LimitCode, PairLevel, PairReason, PairStateView};
use crate::untrusted::{Untrusted, UntrustedName};

/// The predicted conflicts of the caller's worktree, or of its repo; read-only, offered over MCP.
pub const MCP_CONFLICTS: &str = "mcp.conflicts";

/// `mcp.conflicts` parameters. With `cursor`, the scope is the cursor's and `scope` is ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpConflictsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<McpConflictsScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Which pairs: those where the caller's worktree is a side, or every pair of its repo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpConflictsScope {
    #[default]
    Worktree,
    Repo,
}

/// Why the answer has another scope than the one asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpScopeReason {
    /// The working folder is in the repo but under no worktree root.
    CallerWorktreeUnknown,
}

/// `mcp.conflicts` result (SEC-12: a field allowlist, no absolute paths).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpConflicts {
    pub repo_id: String,
    pub repo_state: RepoStateView,
    /// The caller's worktree by its folder name; absent when it cannot be identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<UntrustedName>,
    pub scope: McpConflictsScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<McpScopeReason>,
    /// The declared limits of the prediction, as the engine publishes them.
    pub limits: Vec<LimitCode>,
    /// The predictor has published nothing for this repo (not "no pairs").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unpublished: bool,
    /// Pairs per class over the whole scope, not only this page.
    pub counts: McpPairCounts,
    pub pairs: Vec<McpPair>,
    /// More pairs follow: they come with `cursor`. A cut inside one pair is that pair's own
    /// `conflict.truncated` or `overlap_partial`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// How many pairs of the scope are in each class; a zero is omitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpPairCounts {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub conflict: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overlap: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub none: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub calculating: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pending: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub not_computable: u32,
}

/// One pair. A `current` one omits `state`; one without a result has no `level`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpPair {
    pub a: McpSide,
    pub b: McpSide,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PairStateView>,
    /// Only in `pending` and `not-computable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PairReason>,
    /// Only in `current` and `recalculating`; `none` is explicit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<PairLevel>,
    /// Seconds since the result, by the daemon's clock; never negative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_at_utc_ms: Option<i64>,
    /// The files both sides touch; only in an overlap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlap: Vec<Untrusted>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overlap_partial: bool,
    /// Only in a conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<McpPairConflict>,
}

/// A side of a pair: a worktree by its folder name, or the base by its branch name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpSide {
    Worktree(UntrustedName),
    Base(Option<UntrustedName>),
}

/// The files in conflict of a pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpPairConflict {
    pub files: Vec<McpConflictFile>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// One file in conflict: its ranges `[start, len]` per side (1-based; `len` 0 is an insertion
/// point). Never content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpConflictFile {
    pub path: Untrusted,
    pub kind: ConflictKindView,
    pub a: Vec<[u32; 2]>,
    pub b: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hunks_truncated: bool,
    /// The file has a `merge=` rule of its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge_rule: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &u32) -> bool {
    *n == 0
}
