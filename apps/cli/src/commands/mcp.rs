//! `raptor mcp`: the GitRaptor MCP server in the coding agent.

use std::process::ExitCode;

use super::Global;
use crate::mcp;

/// Register or remove the GitRaptor MCP server in your coding agent.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(subcommand)]
    action: McpAction,
}

#[derive(clap::Subcommand)]
enum McpAction {
    /// Register the gitraptor MCP server in Claude Code, for every project (user scope).
    Install {
        /// The coding agent. Only claude-code is supported for now.
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },
    /// Remove the gitraptor MCP server from Claude Code, only if it is this GitRaptor's.
    Uninstall {
        /// The coding agent. Only claude-code is supported for now.
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            McpAction::Install { agent } => mcp::install(&agent),
            McpAction::Uninstall { agent } => mcp::uninstall(&agent),
        }
    }
}
