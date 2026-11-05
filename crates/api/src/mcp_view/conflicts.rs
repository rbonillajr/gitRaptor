//! The `check_conflicts` tool's result: `mcp.conflicts` without what the model never needs, and
//! the cut of a page to its budget.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::methods::{McpConflicts, McpConflictsScope, McpPair, McpPairCounts, McpScopeReason};
use crate::prediction::LimitCode;
use crate::untrusted::UntrustedName;

/// The most pairs of one page; the byte budget usually cuts before.
pub const MCP_CONFLICTS_PAGE_PAIRS: usize = 50;

/// [`McpConflicts`] without `repo_id` nor `repo_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpConflictsView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<UntrustedName>,
    pub scope: McpConflictsScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<McpScopeReason>,
    pub limits: Vec<LimitCode>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unpublished: bool,
    /// Opaque in the output schema (RES-MCP-01).
    #[schemars(schema_with = "super::opaque_object")]
    pub counts: McpPairCounts,
    /// Opaque in the output schema (RES-MCP-01).
    #[schemars(schema_with = "opaque_array")]
    pub pairs: Vec<McpPair>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl McpConflictsView {
    /// The view the tool gives: names cut at their MCP bound, paths at theirs.
    pub fn for_mcp(_c: &McpConflicts) -> Self {
        unimplemented!("check_conflicts view")
    }
}

/// Bytes of the compact JSON of the view of `c`, after the escape pass.
pub fn conflicts_wire_len(_c: &McpConflicts) -> usize {
    unimplemented!("check_conflicts wire length")
}

/// Cuts `c` to `target` bytes, never past `hard`, keeping its first pairs and marking the cut.
pub fn fit_conflicts(_c: &mut McpConflicts, _target: usize, _hard: usize) {
    unimplemented!("check_conflicts fit")
}

/// `{"type":"array"}`: spelled out, the pairs would cost more than the budget of the schema.
fn opaque_array(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": "array"})
}
