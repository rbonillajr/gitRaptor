//! Scopes with their own sequence (protocol 6, ADR-CKP-003 § 4).

use super::{Group, method};
use crate::capability::Capability;

/// Snapshot of one scope with its own sequence (protocol 6, N1 and N3).
pub const SCOPE_SNAPSHOT: &str = "scope.snapshot";
/// Subscription to one scope, from a scope sequence (protocol 6, N1, N2).
pub const SCOPE_SUBSCRIBE: &str = "scope.subscribe";

/// Notification that carries one event of a scoped subscription.
pub const NOTIFY_SCOPE_EVENT: &str = "scope.event";
/// Notification that one scope cannot continue: take a new snapshot of it.
pub const NOTIFY_SCOPE_RESYNC: &str = "scope.resync";

/// The last activity of each worktree (`WorktreeView::last_activity_utc_ms`, with its gap
/// mark `WorktreeView::last_activity_in_gap`, DS-US-CKP-001 § 8) and the last
/// fetch of each repo (`RepoView::fetched_utc_ms`, `WorktreeStateData::fetched_utc_ms`):
/// DEP-CKP-4, ADR-GRP-013 (amendment 2026-10-04). Also the commit of a detached `HEAD`
/// (`WorktreeView::detached_at`, US-CKP-001). A connection without it never sees them.
pub const CAP_SCOPE_ACTIVITY: Capability = Capability::new("scope.activity");

pub(super) const GROUP: Group = Group {
    // Not reserved and not offered to `raptor-mcp`: they carry paths (SEC-12).
    methods: &[
        method(SCOPE_SNAPSHOT, false, false).since(6),
        method(SCOPE_SUBSCRIBE, false, false).since(6),
    ],
    notifications: &[NOTIFY_SCOPE_EVENT, NOTIFY_SCOPE_RESYNC],
    capabilities: &[CAP_SCOPE_ACTIVITY],
    ..Group::new("scope")
};
