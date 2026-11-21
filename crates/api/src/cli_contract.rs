//! The JSON contract of the `raptor` CLI for agents (US-MCP-023, DS-US-MCP-023).
//! Stub: the criterion tests compile against these signatures; T001 fills them in.

use crate::mcp_view::McpToolError;

pub const CLI_SCHEMA_MAJOR: u16 = 1;
pub const CLI_SCHEMA_MINOR: u16 = 0;
pub const CLI_SCHEMA_VERSION: &str = "1.0";

/// Codes only the CLI emits (the MCP never does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliOnlyCode {
    CommandUnavailable,
    ReservedCommand,
    InvalidArgs,
    CommandFailed,
}

/// A stable error code of the CLI's JSON: the MCP's or a CLI-only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliErrorCode {
    Tool(McpToolError),
    Cli(CliOnlyCode),
}

impl CliErrorCode {
    pub const ALL: &'static [Self] = &[];

    pub const fn as_str(self) -> &'static str {
        ""
    }

    pub const fn exit_code(self) -> u8 {
        1
    }
}

/// How a new schema differs from the published one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    None,
    Additive(Vec<String>),
    Breaking(Vec<String>),
}

/// Every stable code, for the agent guide.
pub fn stable_codes() -> Vec<&'static str> {
    unimplemented!("US-MCP-023 T001")
}

/// The output schemas of the agent view, in a fixed order.
pub fn agent_output_schemas() -> Vec<(&'static str, serde_json::Value)> {
    unimplemented!("US-MCP-023 T001")
}

/// Classifies the change from `old` to `new` (flattened JSON schemas).
pub fn compat(_old: &serde_json::Value, _new: &serde_json::Value) -> Change {
    unimplemented!("US-MCP-023 T001")
}
