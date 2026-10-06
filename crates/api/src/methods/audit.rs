//! The audit of reserved commands.

use super::{Group, method};

/// Reads the append-only audit of reserved commands (ADR-GRP-013 § 1).
pub const AUDIT_LIST: &str = "audit.list";

pub(super) const GROUP: Group = Group {
    methods: &[method(AUDIT_LIST, false, false)],
    ..Group::new("audit")
};
