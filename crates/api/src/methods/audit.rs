//! The audit of reserved commands.

use super::{Group, method};
use crate::capability::Capability;

/// Reads the append-only audit of reserved commands (ADR-GRP-013 § 1).
pub const AUDIT_LIST: &str = "audit.list";

/// The end of an announced action in the audit and in `reserved.audit`: outcomes `applied`,
/// `cancelled`, `failed` and `expired`, and entries whose client is only a process
/// (`client_partial`). A connection without it receives exactly what it received before them.
pub const CAP_AUDIT_OUTCOMES: Capability = Capability::new("audit.outcomes");

pub(super) const GROUP: Group = Group {
    methods: &[method(AUDIT_LIST, false, false)],
    capabilities: &[CAP_AUDIT_OUTCOMES],
    ..Group::new("audit")
};
