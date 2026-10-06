//! The engine as a whole.

use super::{Group, method};

/// Coherent snapshot with the sequence it reflects (DEP-CKP-6).
pub const ENGINE_SNAPSHOT: &str = "engine.snapshot";
/// What the engine consumes on the machine (US-GRP-017, ADR-GRP-015 § 4).
/// Read-only.
pub const ENGINE_RESOURCES: &str = "engine.resources";

pub(super) const GROUP: Group = Group {
    methods: &[
        method(ENGINE_SNAPSHOT, false, true),
        // Read-only, like the snapshot; not offered to `raptor-mcp`
        // (SEC-MCP-01): an agent does not see the engine's consumption.
        method(ENGINE_RESOURCES, false, false),
    ],
    ..Group::new("engine")
};
