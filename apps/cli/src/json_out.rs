//! The one way `raptor … --json` writes: a single compact line on stdout carrying the
//! contract version and either `data` or an `error`, and nothing on stderr.
//!
//! Text that came from a repo or an agent is data (SEC-12): `data` and `error.params` go
//! through [`gitraptor_api::mcp_view::for_mcp`] before they are serialized, the same pass the
//! MCP uses. `error.params` never carries what the caller typed.

// The subcommands adopt these helpers one by one; until the last one does, some are unused.
#![allow(
    dead_code,
    reason = "the subcommands migrate to these helpers in later steps"
)]

use std::io::Write as _;
use std::process::ExitCode;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use gitraptor_api::cli_contract::{
    CLI_SCHEMA_VERSION, CliErr, CliError, CliErrorCode, CliOk, CliOnlyCode,
};
use gitraptor_api::mcp_view::{McpToolError, for_mcp};
use gitraptor_api::messages::{ClientKind, RefusalReason, RefusedData};
use gitraptor_api::methods;
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code, error_name};
use gitraptor_api::timemachine::{TmConfirmData, TmRejectReason};
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;
use serde::Serialize;
use serde_json::{Value, json};

use crate::i18n::t;

/// What is printed when the envelope itself cannot be serialized: fixed, English, no params.
const INTERNAL_LITERAL: &str = r#"{"schema_version":"1.0","error":{"code":"internal","message":"GitRaptor could not answer this command.","params":{},"action":"Retry later."}}"#;

/// The success envelope as one line, `data` escaped as repo text.
fn render_ok<T: Serialize>(data: &T) -> Option<String> {
    let mut data = serde_json::to_value(data).ok()?;
    for_mcp(&mut data);
    serde_json::to_string(&CliOk {
        schema_version: CLI_SCHEMA_VERSION,
        data,
    })
    .ok()
}

/// The rejection envelope as one line, with the fixed texts of `code` in the process language.
fn render_fail(code: CliErrorCode, params: Option<Value>) -> String {
    let mut params = match params {
        Some(object @ Value::Object(_)) => object,
        _ => json!({}),
    };
    for_mcp(&mut params);
    let envelope = CliErr {
        schema_version: CLI_SCHEMA_VERSION,
        error: CliError {
            code,
            message: t(&format!("jsonerr.{}.message", code.as_str()), &[]),
            params,
            action: t(&format!("jsonerr.{}.action", code.as_str()), &[]),
        },
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| INTERNAL_LITERAL.to_owned())
}

/// One line on stdout. A closed pipe is the reader's choice, not an error of ours.
fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Prints `{schema_version, data}` and gives the success exit code.
pub(crate) fn ok<T: Serialize>(data: &T) -> ExitCode {
    match render_ok(data) {
        Some(line) => {
            emit(&line);
            ExitCode::SUCCESS
        }
        None => {
            emit(INTERNAL_LITERAL);
            ExitCode::from(CliErrorCode::Tool(McpToolError::Internal).exit_code())
        }
    }
}

/// Prints `{schema_version, error}` and gives the exit code of the code's family.
pub(crate) fn fail(code: CliErrorCode, params: Option<Value>) -> ExitCode {
    emit(&render_fail(code, params));
    ExitCode::from(code.exit_code())
}

const fn tool(code: McpToolError) -> CliErrorCode {
    CliErrorCode::Tool(code)
}

const fn cli(code: CliOnlyCode) -> CliErrorCode {
    CliErrorCode::Cli(code)
}

/// A reason a daemon sent, kept only when it has the shape of a stable name: the daemon's
/// text is never forwarded.
fn stable_name(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

fn wire_name<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| stable_name(&v).map(str::to_owned))
}

/// The only place where an error of the channel becomes a stable code. `restricted` says the
/// daemon fixed the `mcp` profile for this connection (an agent asks).
pub(crate) fn from_client_error(
    err: &ClientError,
    restricted: bool,
) -> (CliErrorCode, Option<Value>) {
    let ClientError::Rpc(rpc) = err else {
        return match err {
            ClientError::Protocol(_) => (tool(McpToolError::Internal), None),
            // Not running, incompatible, timed out, I/O, launch, rejected channel…: no engine.
            _ => (tool(McpToolError::EngineUnavailable), None),
        };
    };
    match rpc.code {
        c if c == code::SCOPE_REFUSED => {
            let reason = rpc
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<ScopeRefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(ScopeRefusal::NotAllowlisted) => (tool(McpToolError::RepoNotEnabled), None),
                _ => (tool(McpToolError::NotInObservedWorktree), None),
            }
        }
        c if c == code::RATE_LIMITED => (tool(McpToolError::RateLimited), None),
        c if c == methods::MCP_UNAVAILABLE.code => (tool(McpToolError::RepoUnavailable), None),
        c if c == code::IDENTITY_UNVERIFIED => (tool(McpToolError::IdentityUnverified), None),
        c if c == code::RESERVED_REFUSED => {
            let reason = rpc
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<RefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(RefusalReason::IdentityUnverified) => {
                    (tool(McpToolError::IdentityUnverified), None)
                }
                Some(reason) => (
                    cli(CliOnlyCode::ReservedCommand),
                    wire_name(&reason).map(|r| json!({ "reason": r })),
                ),
                None => (cli(CliOnlyCode::ReservedCommand), None),
            }
        }
        c if c == code::METHOD_NOT_FOUND || c == code::NOT_IMPLEMENTED => {
            let reason = if restricted {
                "agent-view"
            } else {
                "engine-version"
            };
            (
                cli(CliOnlyCode::CommandUnavailable),
                Some(json!({ "reason": reason })),
            )
        }
        c if c == code::INVALID_PARAMS => (
            cli(CliOnlyCode::InvalidArgs),
            Some(json!({ "kind": "invalid-params" })),
        ),
        c if c == code::PRIOR_SNAPSHOT_FAILED => (tool(McpToolError::Aborted), None),
        c if c == code::OPERATION_REJECTED => {
            let overlap = rpc
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<TmConfirmData>(d).ok())
                .is_some_and(|d| {
                    matches!(
                        d.reason,
                        TmRejectReason::Overlap | TmRejectReason::OverlapUnverified
                    )
                });
            if overlap {
                (tool(McpToolError::ConfirmationRequired), rpc.data.clone())
            } else {
                failed(rpc.code, rpc.data.as_ref())
            }
        }
        c => failed(c, rpc.data.as_ref()),
    }
}

/// `command-failed`: the stable name of the channel's error and, when its `data` has one, its
/// `reason`.
fn failed(rpc_code: i64, data: Option<&Value>) -> (CliErrorCode, Option<Value>) {
    let reason = error_name(rpc_code).unwrap_or("unknown");
    let mut params = json!({ "reason": reason });
    if let Some(detail) = data.and_then(|d| d.get("reason")).and_then(stable_name) {
        params["detail"] = json!(detail);
    }
    (cli(CliOnlyCode::CommandFailed), Some(params))
}

/// A client of the engine, starting it on demand. With `json`, a failure is an
/// `engine-unavailable` rejection on stdout; without it, the text of `command` on stderr as
/// before.
pub(crate) fn engine(command: &str, json: bool) -> Result<Client, ExitCode> {
    if !json {
        return crate::engine(command);
    }
    let dirs =
        ProfileDirs::resolve().map_err(|_| fail(tool(McpToolError::EngineUnavailable), None))?;
    ensure_daemon(&ClientOptions::new(dirs, ClientKind::Cli))
        .map_err(|err| fail(from_client_error(&err, false).0, None))
}

/// Before a call with `--json`: refuses with `command-unavailable` when this connection cannot
/// call `method` (the agent view does not include it, or the engine is older than this binary).
pub(crate) fn require(client: &Client, method: &str) -> Result<(), ExitCode> {
    if crate::restricted::may_call(client, method) {
        return Ok(());
    }
    let reason = if crate::restricted::is_restricted(client) {
        "agent-view"
    } else {
        "engine-version"
    };
    Err(fail(
        cli(CliOnlyCode::CommandUnavailable),
        Some(json!({ "reason": reason })),
    ))
}

/// `PascalCase` to `kebab-case`: the kind of a usage error as a stable name.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The argument a usage error is about, by the binary's own definition (`--lang`, `<PATH>`).
/// Only for the kinds where clap names a defined argument: for an unknown flag or subcommand
/// the "argument" is what the caller typed, so it is left out.
fn defined_argument(err: &clap::Error) -> Option<String> {
    if matches!(
        err.kind(),
        ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand
    ) {
        return None;
    }
    let ContextValue::String(shown) = err.get(ContextKind::InvalidArg)? else {
        return None;
    };
    let name = shown.split_whitespace().next()?;
    let flag = name.strip_prefix("--").or_else(|| name.strip_prefix('-'));
    let positional = name.strip_prefix('<').and_then(|n| n.strip_suffix('>'));
    let plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    flag.or(positional)
        .filter(|s| plain(s))
        .map(|_| name.to_owned())
}

/// A clap error with `--json`: `invalid-args` with `params.kind` and, when the binary defines
/// it, `params.argument`; never the typed value nor clap's text. Exit code 2.
pub(crate) fn usage(err: &clap::Error) -> ExitCode {
    let mut params = json!({ "kind": kebab(&format!("{:?}", err.kind())) });
    if let Some(argument) = defined_argument(err) {
        params["argument"] = json!(argument);
    }
    fail(cli(CliOnlyCode::InvalidArgs), Some(params))
}

#[cfg(test)]
#[path = "json_out_tests.rs"]
mod tests;
