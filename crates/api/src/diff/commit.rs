//! `diff.commit` and `diff.between`: the diff of a commit against its first parent (or the empty
//! tree) and the tree to tree diff between two commits (DS-TS-CKP-006 D3, D6).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DiffSection;

/// `diff.commit` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitParams {
    pub repo_id: String,
    /// Full oid: 40 or 64 lowercase hex characters, nothing else (D4).
    pub commit: String,
}

/// `diff.between` parameters: `old → new`, tree to tree, with no merge-base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BetweenParams {
    pub repo_id: String,
    /// Full oid of the old side.
    pub old: String,
    /// Full oid of the new side.
    pub new: String,
    /// One literal file, relative to the repository root; no patterns, no directories.
    #[serde(default)]
    pub path: Option<String>,
}

/// `diff.commit` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitDiff {
    pub computed_at_utc_ms: i64,
    pub parents: u32,
    /// `parents > 1`; a merge is diffed against its first parent.
    pub merge: bool,
    pub section: DiffSection,
}

/// `diff.between` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BetweenDiff {
    pub computed_at_utc_ms: i64,
    pub section: DiffSection,
}
