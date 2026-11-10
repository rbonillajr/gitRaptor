//! `diff.index`: a worktree split in staged (`HEAD` against the index) and unstaged (the index
//! against the working tree), with the content id of each side and the conflicted paths
//! (DS-TS-CKP-006 D7, D8). Parameters: [`super::DiffParams`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DiffCut, FileDiff};
use crate::untrusted::Untrusted;

/// Share of [`super::MAX_DIFF_RESPONSE_BYTES`] reserved for `conflicts` (M-01).
pub const CONFLICTS_RESERVE_BYTES: usize = 64 * 1024;

/// `diff.index` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexDiff {
    pub computed_at_utc_ms: i64,
    /// `HEAD` against the index.
    pub staged: IndexSection,
    /// The index against the working tree, untracked files included.
    pub unstaged: IndexSection,
    /// Paths with entries of stages 1 to 3, without content; out of both sections.
    pub conflicts: Vec<Untrusted>,
    /// Every conflicted path, also the ones that did not fit.
    pub conflicts_total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexSection {
    pub files: Vec<IndexFileDiff>,
    /// Every changed file, also the ones that did not fit.
    pub files_total: u32,
    pub cut: Option<DiffCut>,
}

/// A file of a section with the content id of each side: a hunk is identified by (section,
/// path, `old`, `new`, its four bounds) (D8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexFileDiff {
    pub file: FileDiff,
    pub old: ContentId,
    pub new: ContentId,
}

/// The content of one side of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContentId {
    /// Lowercase hex oid of the side's blob (for the working tree, the Git hash of its raw bytes).
    Blob { id: String },
    /// Absent, too large, a link, a submodule or unreadable.
    None {},
}
