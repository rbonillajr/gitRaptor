//! `raptor mcp`: the GitRaptor MCP server in the coding agent, and the repos
//! it may use (US-MCP-001, US-MCP-002).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gitraptor_api::messages::{RepoRejectedData, RepoRejection};
use gitraptor_api::methods::{self, McpAllowlistResult, McpRepoParams, McpRepoResult};
use gitraptor_api::rpc::code;
use gitraptor_core::client::ClientError;

use super::Global;
use crate::i18n::t;
use crate::mcp;
use crate::support::refusal_text;
use crate::{command_path, engine, repo_error, shown};

/// Register or remove the GitRaptor MCP server in your coding agent, and choose the repos it may use.
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
    /// Let agents use an observed repo through the MCP server (reserved to the developer).
    Enable {
        /// Any folder of the repo; defaults to the current folder.
        path: Option<PathBuf>,
    },
    /// Stop agents from using a repo through the MCP server; it stays observed (reserved).
    Disable {
        /// Any folder of the repo; defaults to the current folder.
        path: Option<PathBuf>,
    },
    /// List the repos agents may use through the MCP server.
    List,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            McpAction::Install { agent } => mcp::install(&agent),
            McpAction::Uninstall { agent } => mcp::uninstall(&agent),
            McpAction::Enable { path } => mark(path, true),
            McpAction::Disable { path } => mark(path, false),
            McpAction::List => list(),
        }
    }
}

/// `raptor mcp enable|disable`: reserved commands (US-MCP-002, BR-MCP-AUTH-004).
/// The engine decides who may run them and whether the repo is observed.
fn mark(path: Option<PathBuf>, enable: bool) -> ExitCode {
    let cmd = if enable {
        "raptor mcp enable"
    } else {
        "raptor mcp disable"
    };
    let path = command_path(path);
    let mut client = match engine(cmd) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let method = if enable {
        methods::MCP_ENABLE
    } else {
        methods::MCP_DISABLE
    };
    let params = McpRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, McpRepoResult>(method, &params) {
        Ok(result) => {
            let key = match (enable, result.changed) {
                (true, true) => "mcp.enabled",
                (true, false) => "mcp.already-enabled",
                (false, true) => "mcp.disabled",
                (false, false) => "mcp.already-disabled",
            };
            println!("{}", t(key, &[("path", &shown(&path))]));
            ExitCode::SUCCESS
        }
        Err(err) => mark_error(cmd, &path, err),
    }
}

fn mark_error(cmd: &str, path: &Path, err: ClientError) -> ExitCode {
    let message = match &err {
        ClientError::Rpc(e) if e.code == code::RESERVED_REFUSED => {
            Some(refusal_text(e, "mcp.refused-agent"))
        }
        ClientError::Rpc(e) if e.code == code::REPO_REJECTED => e
            .data
            .clone()
            .and_then(|d| serde_json::from_value::<RepoRejectedData>(d).ok())
            .filter(|d| {
                matches!(
                    d.reason,
                    RepoRejection::NotObserved | RepoRejection::UnknownRepo
                )
            })
            .map(|_| t("mcp.observe-first", &[("path", &shown(path))])),
        _ => None,
    };
    match message {
        Some(message) => {
            eprintln!("{cmd}: {message}");
            ExitCode::FAILURE
        }
        None => repo_error(cmd, path, err),
    }
}

/// `raptor mcp list`: the enabled repos, by key.
fn list() -> ExitCode {
    const CMD: &str = "raptor mcp list";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    match client.call::<_, McpAllowlistResult>(methods::MCP_ALLOWLIST, &serde_json::json!({})) {
        Ok(result) if result.repo_ids.is_empty() => {
            println!("{}", t("mcp.list-empty", &[]));
            ExitCode::SUCCESS
        }
        Ok(result) => {
            for id in result.repo_ids {
                println!("{id}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{CMD}: {}", crate::support::error_text(err));
            ExitCode::FAILURE
        }
    }
}

/// Whether `repo_id` is in the MCP allowlist; `None` if the engine cannot
/// tell (an older engine, or a refusal): the caller goes on without it.
pub(crate) fn is_enabled(client: &mut gitraptor_core::client::Client, repo_id: &str) -> Option<bool> {
    if !crate::support::offers(client, methods::MCP_ALLOWLIST) {
        return None;
    }
    client
        .call::<_, McpAllowlistResult>(methods::MCP_ALLOWLIST, &serde_json::json!({}))
        .ok()
        .map(|r| r.repo_ids.iter().any(|id| id == repo_id))
}
