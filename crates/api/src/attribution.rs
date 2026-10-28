//! Types of the correction of a detected attribution: `attribution.correct` and
//! `attribution.withdraw-correction` (reserved to the developer, never offered to `raptor-mcp`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Actor;
use crate::messages::DeclaredAgent;

/// Event kind published after a correction or its withdrawal, in the repo's scope. Data:
/// [`AttributionChangedData`].
pub const ATTRIBUTION_CHANGED: &str = "attribution.changed";

/// Most candidates an `ambiguous-session` rejection lists.
pub const MAX_SESSION_CANDIDATES: usize = 10;

/// `attribution.correct` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionCorrectParams {
    /// The agent the session really is.
    pub agent: DeclaredAgent,
    /// The worktree of the session. Required: the daemon never reads the caller's working folder
    /// for a reserved command.
    pub worktree: String,
    /// The session to correct; without it, the only detected session present in the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// `attribution.withdraw-correction` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionWithdrawParams {
    pub worktree: String,
    /// The session whose correction is withdrawn; without it, the only corrected session present
    /// in the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Result of both methods: the attribution of the session before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionChangeResult {
    pub repo_id: String,
    pub session_id: String,
    pub previous: Actor,
    pub actor: Actor,
    /// The new correction is the agent the engine detected: withdrawing the correction would
    /// have reached the same attribution.
    #[serde(default)]
    pub matches_detected: bool,
}

/// Why a correction or its withdrawal was refused. Nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AttributionRejection {
    /// The folder is not a worktree of an observed repo.
    NotAWorktree,
    /// A worktree of a repo the engine does not observe.
    RepoNotObserved,
    /// No detected session is present in the worktree.
    NoDetectedAttribution,
    /// The session was created by a registration: withdraw it and register the right agent.
    RegisteredSession,
    /// Several detected sessions are present: the caller must choose one.
    AmbiguousSession,
    /// The worktree has no session with that id.
    UnknownSession,
    /// The session is already attributed to that agent.
    SameAgent,
    /// Withdrawal: the session has no active correction.
    NotCorrected,
}

/// `data` of an `ATTRIBUTION_REJECTED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionRejectedData {
    pub reason: AttributionRejection,
    /// Only with `ambiguous-session`, at most [`MAX_SESSION_CANDIDATES`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<SessionCandidate>,
}

/// A session the caller may choose with `session_id`. No paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionCandidate {
    pub session_id: String,
    pub actor: Actor,
    pub started_utc_ms: i64,
    pub present: bool,
}

/// Data of an `attribution.changed` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributionChangedData {
    pub repo_id: String,
    pub session_id: String,
    pub actor: Actor,
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
