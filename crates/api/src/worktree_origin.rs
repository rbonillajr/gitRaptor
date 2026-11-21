//! Who created a linked worktree and how that is known (US-WTL-005, BR-WTL-011).
//!
//! Compile-only skeleton for the contract tests; the behaviour lands with T001.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The tool that created a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OriginToolView {
    Gitraptor,
    Orca,
    Worktrunk,
    Codex,
    Gitkraken,
    ClaudeCode,
    Unknown,
}

/// How the origin is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OriginEvidenceView {
    Registered,
    AdminMarker,
    PathPattern,
    None,
}

/// Why a foreign worktree is "hands off".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HandsOffReasonView {
    Detached,
    Locked,
    Unreadable,
}

/// Who created a linked worktree and how that is known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorktreeOriginView {
    pub tool: OriginToolView,
    pub evidence: OriginEvidenceView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hands_off: Option<HandsOffReasonView>,
}
