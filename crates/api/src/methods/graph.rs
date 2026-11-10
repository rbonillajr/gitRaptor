//! The commits of each worktree's branch over the base, on demand (types in [`crate::graph`]).
//! Never offered to `raptor-mcp`: it shares the global query queue and has no budget of its own
//! per requester. No error block: everything not computable goes in the result.

use super::{Group, method};
use crate::capability::CAPABILITIES_PROTOCOL;

/// One lane per worktree, with its newest commits. Answered out of order.
pub const GRAPH_LANES: &str = "graph.lanes";

pub(super) const GROUP: Group = Group {
    methods: &[method(GRAPH_LANES, false, false).since(CAPABILITIES_PROTOCOL)],
    ..Group::new("graph")
};
