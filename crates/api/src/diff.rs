//! Types of `diff.worktree`: what a worktree would integrate and what it has uncommitted.
//!
//! Paths, the base name and every line are untrusted text (SEC-12). Nothing here names an actor,
//! a session or a commit message (BR-CKP-EDGE-004), and the response is never persisted (SEC-05).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::untrusted::{Untrusted, UntrustedName};

/// Files per section the daemon returns (⚠️ ASSUMPTION, D4 of DS-US-CKP-012).
pub const MAX_DIFF_FILES: usize = 1_000;
/// Diff lines per file (⚠️ ASSUMPTION, D4).
pub const MAX_DIFF_LINES_PER_FILE: usize = 2_000;
/// Largest blob or file whose content is read, in bytes (⚠️ ASSUMPTION, D4).
pub const MAX_DIFF_BLOB_BYTES: u64 = 8 * 1024 * 1024;
/// Serialized response cap, under `MAX_MESSAGE_BYTES` (D4).
pub const MAX_DIFF_RESPONSE_BYTES: usize = 768 * 1024;
/// Deadline of one query, in milliseconds (⚠️ ASSUMPTION, D4).
pub const DIFF_DEADLINE_MS: u64 = 2_000;
/// Queries one connection may have in flight (⚠️ ASSUMPTION, D4).
pub const QUERIES_PER_CONNECTION: u32 = 1;
/// Queries waiting in the shared queue (⚠️ ASSUMPTION, D4).
pub const QUERY_QUEUE: usize = 4;

/// `diff.worktree` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffParams {
    pub repo_id: String,
    /// Root of the worktree.
    pub worktree: String,
}

/// `diff.worktree` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorktreeDiff {
    pub computed_at_utc_ms: i64,
    /// The branch against its merge-base with the confirmed base.
    pub committed: CommittedDiff,
    /// `HEAD` against the working tree, untracked files included.
    pub uncommitted: DiffSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CommittedDiff {
    Ready { base: UntrustedName, section: DiffSection },
    NotComputable { reason: CommittedGap },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CommittedGap {
    BaseUnconfirmed,
    NoBase,
    BaseMissing,
    NoMergeBase,
    /// Listing the changed paths went past the deadline.
    TooSlow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffSection {
    pub files: Vec<FileDiff>,
    /// Every changed file, also the ones that did not fit.
    pub files_total: u32,
    pub cut: Option<DiffCut>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiffCut {
    Files,
    Bytes,
    Time,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileDiff {
    pub path: Untrusted,
    pub change: FileChange,
    pub added: u32,
    pub removed: u32,
    pub body: FileBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    TypeChanged,
}

/// How a file shows: only `Text` carries content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileBody {
    Text { hunks: Vec<Hunk>, truncated: bool },
    Binary {},
    TooLarge {},
    Submodule {},
    Symlink {},
    /// A cap of the section was reached before this file.
    Omitted {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<DiffLineView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffLineView {
    pub kind: LineKind,
    pub text: Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// `data` of `diff-unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffUnavailableData {
    pub reason: DiffUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiffUnavailable {
    WorktreeUnavailable,
    RepoUnavailable,
}
