//! Discovery roots and discovered repos (US-GRP-020, US-GRP-022; ADR-GRP-010,
//! Enmienda 2026-10-07, N6 and N8). Types in [`crate::discovery`].
//!
//! Nothing here is offered to `raptor-mcp` (SEC-MCP-01, SEC-15): neither the
//! roots, nor the candidates, nor their event. Declaring or removing a root
//! and dismissing a candidate are reserved commands (SEC-03): an agent's
//! file write cannot declare a root, which is why roots are profile state and
//! not settings. Accepting a candidate is `repo.add`.

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, method};
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};
use crate::rpc::ErrorSpec;

/// The declared roots. Read-only.
pub const DISCOVERY_ROOTS: &str = "discovery.roots";
/// The discovered repos waiting for the developer's decision. Read-only.
pub const DISCOVERY_CANDIDATES: &str = "discovery.candidates";
/// Declares a root (reserved). A broad root needs `confirm_broad`.
pub const DISCOVERY_ROOT_ADD: &str = "discovery.root.add";
/// Removes a root and its pending candidates (reserved). Observed repos and
/// dismissals stay.
pub const DISCOVERY_ROOT_REMOVE: &str = "discovery.root.remove";
/// Dismisses a candidate by its path (reserved): it is never proposed again.
pub const DISCOVERY_DISMISS: &str = "discovery.dismiss";

/// The `repo.discovered` event ([`crate::event::REPO_DISCOVERED`]). A
/// connection without it never receives it.
pub const CAP_DISCOVERY_EVENTS: Capability = Capability::new("discovery.events");

// The block after `FIRST_ERROR_BLOCK` (ADR-GRP-016 § 3).
const BLOCK: i64 = FIRST_ERROR_BLOCK - ERROR_BLOCK_LEN;

/// The path cannot be a root: `data` is [`crate::discovery::RootRejectedData`].
pub const ROOT_REJECTED: ErrorSpec = ErrorSpec::new(BLOCK, "root-rejected");
/// The root is broad and was not confirmed: `data` is
/// [`crate::discovery::RootBroadData`].
pub const ROOT_BROAD: ErrorSpec = ErrorSpec::new(BLOCK - 1, "root-broad");
/// The path is not a declared root.
pub const ROOT_UNKNOWN: ErrorSpec = ErrorSpec::new(BLOCK - 2, "root-unknown");
/// The path is not a discovered repo waiting for a decision.
pub const NOT_A_CANDIDATE: ErrorSpec = ErrorSpec::new(BLOCK - 3, "not-a-candidate");

pub(super) const GROUP: Group = Group {
    methods: &[
        method(DISCOVERY_ROOTS, false, false).since(CAPABILITIES_PROTOCOL),
        method(DISCOVERY_CANDIDATES, false, false).since(CAPABILITIES_PROTOCOL),
        method(DISCOVERY_ROOT_ADD, true, false).since(CAPABILITIES_PROTOCOL),
        method(DISCOVERY_ROOT_REMOVE, true, false).since(CAPABILITIES_PROTOCOL),
        method(DISCOVERY_DISMISS, true, false).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[CAP_DISCOVERY_EVENTS],
    error_block: Some(BLOCK),
    errors: &[ROOT_REJECTED, ROOT_BROAD, ROOT_UNKNOWN, NOT_A_CANDIDATE],
    ..Group::new("discovery")
};
