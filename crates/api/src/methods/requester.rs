//! Who the daemon sees on a connection (ADR-TMC-005 § 1).

use super::{Group, method};

/// How the daemon sees the caller: "agent X" or "unattributed" (ADR-TMC-005
/// § 1). Read-only.
pub const REQUESTER_RESOLVE: &str = "requester.resolve";

pub(super) const GROUP: Group = Group {
    methods: &[method(REQUESTER_RESOLVE, false, true)],
    ..Group::new("requester")
};
