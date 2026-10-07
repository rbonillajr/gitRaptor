//! The MCP surface of `raptor-mcp` (ADR-MCP-001 § 1, SEC-MCP-07): every
//! field of `initialize` is a constant of the binary, with no text from the
//! repo or the environment. The tools take the repo and the worktree from
//! the session's folder, resolved by the engine, never from an argument
//! (ADR-MCP-001 § 2); the connection to the engine opens on the first tool
//! call (BR-MCP-TIME-003).

use std::sync::Arc;

use gitraptor_api::methods::McpStatus;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolsCapability,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};

use crate::engine::Engine;

/// The name Claude Code registers and the server announces.
pub const SERVER_NAME: &str = "gitraptor";

/// The version of this binary.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Constant, in English, never built from the repo or the environment (S-12).
pub const INSTRUCTIONS: &str = "GitRaptor: safe Git operations for coding agents. \
Tools act only on the repo and worktree this session was started in, and only \
when the developer has enabled that repo for MCP.";

/// The one tool of US-MCP-003: the state of the session's repo.
pub const STATUS_TOOL: &str = "status";

const STATUS_DESCRIPTION: &str = "State of the repo and worktree this session was \
started in, as GitRaptor's engine sees it, and who the engine sees as the caller. \
Takes no arguments: the repo is never chosen by the caller.";

#[derive(Clone, Debug, Default)]
pub struct Raptor {
    engine: Arc<Engine>,
}

/// `{"type": "object", "properties": {}, "additionalProperties": false}`.
fn no_arguments() -> JsonObject {
    let mut schema = JsonObject::new();
    schema.insert("type".into(), "object".into());
    schema.insert("properties".into(), JsonObject::new().into());
    schema.insert("additionalProperties".into(), false.into());
    schema
}

fn status_tool() -> Tool {
    let output = serde_json::to_value(schemars::schema_for!(McpStatus))
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    Tool::new(STATUS_TOOL, STATUS_DESCRIPTION, no_arguments())
        .with_raw_output_schema(Arc::new(output))
}

/// A status: the same JSON as structured content (it matches the output
/// schema) and as text.
fn success(value: serde_json::Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
    result.structured_content = Some(value);
    result
}

/// A refusal: `{reason, action}` as text only, since it does not match the
/// status's output schema.
fn refused(value: serde_json::Value) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(value.to_string())])
}

impl ServerHandler for Raptor {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![status_tool()]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != STATUS_TOOL {
            return Err(ErrorData::invalid_params("unknown tool", None));
        }
        // NFR-02: no argument is accepted, so none can name another repo.
        if request.arguments.as_ref().is_some_and(|a| !a.is_empty()) {
            return Err(ErrorData::invalid_params("status takes no arguments", None));
        }
        // The client is blocking: off the runtime's only thread, so the
        // session keeps reading stdin meanwhile.
        let engine = Arc::clone(&self.engine);
        let status = tokio::task::spawn_blocking(move || engine.status())
            .await
            .map_err(|_| ErrorData::internal_error("internal-error", None))?;
        let response = match status {
            Ok(status) => serde_json::to_value(status).map(success),
            Err(refusal) => serde_json::to_value(refusal.refused()).map(refused),
        };
        response
            .map(Into::into)
            .map_err(|_| ErrorData::internal_error("internal-error", None))
    }

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
        let info = serde_json::to_value(Raptor::default().get_info()).unwrap();
        assert_eq!(
            info["capabilities"],
            serde_json::json!({"tools": {"listChanged": false}})
        );
        assert_eq!(info["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(info["serverInfo"]["version"], SERVER_VERSION);
        assert_eq!(info["instructions"], INSTRUCTIONS);
    }

    /// One tool, with no arguments and the status as its output schema.
    #[test]
    fn the_status_tool_takes_no_arguments() {
        let tool = serde_json::to_value(status_tool()).unwrap();
        assert_eq!(tool["name"], STATUS_TOOL);
        assert_eq!(
            tool["inputSchema"],
            serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false})
        );
        let required = tool["outputSchema"]["required"].as_array().unwrap();
        assert!(required.iter().any(|f| f == "repo_id"), "{tool}");
    }
}
