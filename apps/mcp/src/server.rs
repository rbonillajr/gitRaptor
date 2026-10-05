//! The MCP surface of `raptor-mcp` (ADR-MCP-001 § 1, SEC-MCP-07): every
//! field of `initialize` is a constant of the binary, with no text from the
//! repo or the environment. US-MCP-001 ships the server with no tools; they
//! arrive with US-MCP-003 and later stories, and so does the connection to
//! the engine, which opens on the first tool call (BR-MCP-TIME-003).

use rmcp::ServerHandler;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig, ToolsCapability};

/// The name Claude Code registers and the server announces.
pub const SERVER_NAME: &str = "gitraptor";

/// The version of this binary.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Constant, in English, never built from the repo or the environment (S-12).
pub const INSTRUCTIONS: &str = "GitRaptor: safe Git operations for coding agents. \
Tools act only on the repo and worktree this session was started in, and only \
when the developer has enabled that repo for MCP.";

#[derive(Clone, Copy, Debug, Default)]
pub struct Raptor;

impl ServerHandler for Raptor {
    fn get_info(&self) -> ServerConfig {
        let mut tools = ToolsCapability::default();
        tools.list_changed = Some(false);
        let mut capabilities = ServerCapabilities::default();
        capabilities.tools = Some(tools);
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(INSTRUCTIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only `tools`, without `listChanged` (Q-MCP-12, Q-MCP-13).
    #[test]
    fn announces_only_the_tools_capability() {
        let info = serde_json::to_value(Raptor.get_info()).unwrap();
        assert_eq!(
            info["capabilities"],
            serde_json::json!({"tools": {"listChanged": false}})
        );
        assert_eq!(info["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(info["serverInfo"]["version"], SERVER_VERSION);
        assert_eq!(info["instructions"], INSTRUCTIONS);
    }
}
