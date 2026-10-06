//! Scopes with their own sequence (protocol 6, ADR-CKP-003 § 4).

use super::{Group, method};

/// Snapshot of one scope with its own sequence (protocol 6, N1 and N3).
pub const SCOPE_SNAPSHOT: &str = "scope.snapshot";
/// Subscription to one scope, from a scope sequence (protocol 6, N1, N2).
pub const SCOPE_SUBSCRIBE: &str = "scope.subscribe";

/// Notification that carries one event of a scoped subscription.
pub const NOTIFY_SCOPE_EVENT: &str = "scope.event";
/// Notification that one scope cannot continue: take a new snapshot of it.
pub const NOTIFY_SCOPE_RESYNC: &str = "scope.resync";

pub(super) const GROUP: Group = Group {
    // Not reserved and not offered to `raptor-mcp`: they carry paths (SEC-12).
    methods: &[
        method(SCOPE_SNAPSHOT, false, false).since(6),
        method(SCOPE_SUBSCRIBE, false, false).since(6),
    ],
    notifications: &[NOTIFY_SCOPE_EVENT, NOTIFY_SCOPE_RESYNC],
    ..Group::new("scope")
};
