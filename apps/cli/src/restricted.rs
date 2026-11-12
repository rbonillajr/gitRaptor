//! The restricted view of `raptor` (TS-GRP-010 D11, ADR-GRP-004 Enmienda
//! 2026-10-05 MCP): when the daemon fixes the `mcp` profile for this
//! connection because an agent asks (S-01), `raptor status`, `raptor daemon
//! status` and the TUI show what the MCP shows, never more. The daemon
//! decides; this only presents. The versioned `--json` envelope is
//! US-MCP-023's.

use std::path::Path;
use std::process::ExitCode;

use gitraptor_api::mcp_view::McpToolError;
use gitraptor_api::messages::ConnectionProfile;
use gitraptor_api::methods::{self, McpStatus, McpStatusParams};
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_core::client::{Client, ClientError};

use crate::error_text;
use crate::i18n::t;

/// The parameters of a Time Machine command for `worktree` (the one of the cwd), with the `cli`
/// surface. With the `mcp` profile the worktree is not sent: the daemon takes it from the
/// caller's working folder and refuses a named one (TS-GRP-010 D15).
pub(crate) fn tm_params(client: &Client, worktree: &Path) -> serde_json::Value {
    if is_restricted(client) {
        serde_json::json!({ "surface": "cli" })
    } else {
        serde_json::json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" })
    }
}

/// The text of a refused Time Machine command of a restricted connection, when the refusal is
/// one the MCP names (a repo not enabled): `None` for the command's own texts.
pub(crate) fn refusal_text(client: &Client, err: &ClientError) -> Option<String> {
    (is_restricted(client) && refusal_name(err) == McpToolError::RepoNotEnabled.as_str())
        .then(|| t("restricted.not-enabled", &[]))
}

/// Whether this binary may call `method`: the engine offers it, or it is a reserved command
/// of the connection's protocol on a connection with the `mcp` profile. The engine does not
/// offer reserved commands there but still decides them (and audits): the CLI calls and shows
/// the engine's refusal (TS-GRP-010 D15).
pub(crate) fn may_call(client: &Client, method: &str) -> bool {
    let hello = client.hello();
    hello.methods.iter().any(|m| m == method)
        || (hello.profile == ConnectionProfile::Mcp
            && methods::spec(method).is_some_and(|s| s.reserved && s.exists_in(hello.protocol)))
}

/// Whether the daemon gave this connection the `mcp` profile.
pub(crate) fn is_restricted(client: &Client) -> bool {
    client.hello().profile == ConnectionProfile::Mcp
}

/// `mcp.status` of the caller's worktree, as the MCP's `status` tool sees it.
/// With `json`, its result, or `{"error": "<refusal>"}` and exit code 1.
pub(crate) fn print_status(client: &mut Client, json: bool) -> ExitCode {
    let result =
        client.call::<_, serde_json::Value>(methods::MCP_STATUS, McpStatusParams::default());
    match (result, json) {
        (Ok(value), true) => match serde_json::to_string_pretty(&value) {
            Ok(text) => {
                // serde_json escapes every control character: no raw escape
                // sequence of an untrusted name reaches the terminal (SEC-12).
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::FAILURE,
        },
        (Ok(value), false) => match serde_json::from_value::<McpStatus>(value) {
            Ok(status) => {
                let worktree = status.worktree.sanitized();
                let branch = status
                    .branch
                    .as_ref()
                    .map(|b| b.sanitized())
                    .unwrap_or_else(|| "-".to_owned());
                println!(
                    "{}",
                    t(
                        "restricted.status",
                        &[("worktree", &worktree), ("branch", &branch)],
                    )
                );
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::FAILURE,
        },
        (Err(err), true) => {
            let refusal = refusal_name(&err);
            println!("{}", serde_json::json!({ "error": refusal }));
            ExitCode::FAILURE
        }
        (Err(err), false) => {
            if refusal_name(&err) == McpToolError::RepoNotEnabled.as_str() {
                eprintln!("raptor status: {}", t("restricted.not-enabled", &[]));
            } else {
                eprintln!("raptor status: {}", error_text(err));
            }
            ExitCode::FAILURE
        }
    }
}

/// The MCP's name of a refused `mcp.status`, the same the `status` tool gives.
pub(crate) fn refusal_name(err: &ClientError) -> &'static str {
    match err {
        ClientError::Rpc(e) if e.code == code::SCOPE_REFUSED => {
            let reason = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<ScopeRefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(ScopeRefusal::NotAllowlisted) => McpToolError::RepoNotEnabled.as_str(),
                _ => McpToolError::NotInObservedWorktree.as_str(),
            }
        }
        ClientError::Rpc(e) if e.code == code::RATE_LIMITED => McpToolError::RateLimited.as_str(),
        ClientError::Rpc(e) if e.code == methods::MCP_UNAVAILABLE.code => {
            McpToolError::RepoUnavailable.as_str()
        }
        _ => McpToolError::Internal.as_str(),
    }
}
