//! Agent sessions (US-GRP-007).

use super::{Group, method};
use crate::capability::Capability;

/// Lists the agent sessions of the observed repos (US-GRP-007, ADR-GRP-013
/// § 6).
pub const SESSIONS_LIST: &str = "sessions.list";

/// `SessionEndCauseView::WorktreeRemoved`. A connection without it receives the session ended
/// without a cause.
pub const CAP_SESSIONS_WORKTREE_REMOVED: Capability = Capability::new("sessions.worktree-removed");

pub(super) const GROUP: Group = Group {
    // Carries worktree paths: not offered to `raptor-mcp` (SEC-12).
    methods: &[method(SESSIONS_LIST, false, false)],
    capabilities: &[CAP_SESSIONS_WORKTREE_REMOVED],
    ..Group::new("sessions")
};
