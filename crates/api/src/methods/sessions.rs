//! Agent sessions (US-GRP-007).

use super::{Group, method};

/// Lists the agent sessions of the observed repos (US-GRP-007, ADR-GRP-013
/// § 6).
pub const SESSIONS_LIST: &str = "sessions.list";

pub(super) const GROUP: Group = Group {
    // Carries worktree paths: not offered to `raptor-mcp` (SEC-12).
    methods: &[method(SESSIONS_LIST, false, false)],
    ..Group::new("sessions")
};
