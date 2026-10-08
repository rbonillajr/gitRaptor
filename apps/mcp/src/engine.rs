//! `raptor-mcp` as a client of the engine (ADR-MCP-001 § 1, BR-MCP-CONS-001):
//! the connection opens on the first tool call, never at the handshake
//! (BR-MCP-TIME-003), and starts the engine on demand with the `raptor`
//! installed next to this binary. Nothing is read from the repo here: every
//! answer comes from the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, TryLockError};

use gitraptor_api::mcp_view::McpToolError;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods::{self, McpStatus};
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;

/// The lazy connection to the engine.
#[derive(Default)]
pub struct Engine {
    client: Mutex<Option<Client>>,
    /// Whether the last call ended with an open connection to the engine.
    connected: AtomicBool,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl Engine {
    /// `mcp.status`: the caller's repo, resolved by the engine. A broken
    /// connection (the engine restarted) is opened again once.
    pub fn status(&self) -> Result<McpStatus, McpToolError> {
        // A call still running past its time limit holds the connection:
        // this one answers at once rather than queue behind it, spending
        // the connection's read budget on an answer nobody reads.
        let mut slot = match self.client.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(self.late()),
        };
        let reused = slot.is_some();
        let result = match call_status(&mut slot) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) if reused => call_status(&mut slot),
            other => other,
        };
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        result.map_err(|err| refusal(&err))
    }

    /// What a call that did not end in time means: with no connection yet,
    /// the engine is still being started or replaced and counts as
    /// unavailable (BR-MCP-EDGE-001); with one, it was late.
    pub fn late(&self) -> McpToolError {
        if self.connected.load(Ordering::Relaxed) {
            McpToolError::TimeLimit
        } else {
            McpToolError::EngineUnavailable
        }
    }
}

/// One `mcp.status`, connecting first when there is no connection; a
/// connection that breaks is dropped.
fn call_status(slot: &mut Option<Client>) -> Result<McpStatus, ClientError> {
    if slot.is_none() {
        let dirs = ProfileDirs::resolve().map_err(|_| ClientError::NotRunning)?;
        *slot = Some(ensure_daemon(&ClientOptions::new(dirs, ClientKind::Mcp))?);
    }
    let client = slot.as_mut().ok_or(ClientError::NotRunning)?;
    let result = client.call::<_, McpStatus>(methods::MCP_STATUS, serde_json::json!({}));
    if matches!(result, Err(ClientError::Io(_) | ClientError::Protocol(_))) {
        *slot = None;
    }
    result
}

fn refusal(err: &ClientError) -> McpToolError {
    match err {
        ClientError::Rpc(e) if e.code == code::SCOPE_REFUSED => {
            let reason = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<ScopeRefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(ScopeRefusal::NotAllowlisted) => McpToolError::RepoNotEnabled,
                _ => McpToolError::NotInObservedWorktree,
            }
        }
        ClientError::Rpc(e) if e.code == code::IDENTITY_UNVERIFIED => {
            McpToolError::IdentityUnverified
        }
        // The connection's read budget (US-MCP-005, ADR-MCP-001 § 6).
        ClientError::Rpc(e) if e.code == code::RATE_LIMITED => McpToolError::RateLimited,
        ClientError::Rpc(_) | ClientError::Protocol(_) | ClientError::Unsupported(_) => {
            McpToolError::Internal
        }
        // Not running and not startable, an incompatible engine, a rejected
        // channel or a broken connection: the installation must be checked.
        _ => McpToolError::EngineUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::rpc::ErrorObject;

    #[test]
    fn engine_refusals_map_to_the_tool_codes() {
        let scope = |reason| {
            ClientError::Rpc(
                ErrorObject::new(code::SCOPE_REFUSED, "x").with_data(ScopeRefusedData { reason }),
            )
        };
        assert_eq!(
            refusal(&scope(ScopeRefusal::NotAllowlisted)),
            McpToolError::RepoNotEnabled
        );
        for reason in [ScopeRefusal::NotObserved, ScopeRefusal::NoWorkingFolder] {
            assert_eq!(refusal(&scope(reason)), McpToolError::NotInObservedWorktree);
        }
        let limited = ClientError::Rpc(ErrorObject::new(code::RATE_LIMITED, "rate limited"));
        assert_eq!(refusal(&limited), McpToolError::RateLimited);
    }

    /// Past the time limit: an engine never reached is unavailable; a
    /// connected one was late. A second call never queues behind the first.
    #[test]
    fn a_late_call_is_told_apart_and_never_queued() {
        let engine = Engine::default();
        assert_eq!(engine.late(), McpToolError::EngineUnavailable);
        engine.connected.store(true, Ordering::Relaxed);
        assert_eq!(engine.late(), McpToolError::TimeLimit);
        let _held = engine.client.lock().unwrap();
        assert_eq!(engine.status(), Err(McpToolError::TimeLimit));
    }
}
