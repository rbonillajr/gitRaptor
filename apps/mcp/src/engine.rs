//! `raptor-mcp` as a client of the engine (ADR-MCP-001 § 1, BR-MCP-CONS-001):
//! the connection opens on the first tool call, never at the handshake
//! (BR-MCP-TIME-003), and starts the engine on demand with the `raptor`
//! installed next to this binary. Nothing is read from the repo here: every
//! answer comes from the engine.

use std::sync::Mutex;

use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods::{self, McpStatus};
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;
use serde::Serialize;

/// Why a tool gives no data, with what resolves it: stable codes in English
/// (the texts in en/es arrive with US-MCP-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refusal {
    /// The repo is observed but the developer did not enable it.
    RepoNotEnabled,
    /// The session's folder is in no observed worktree.
    NotInObservedWorktree,
    /// GitRaptor is not running and could not be started.
    EngineUnavailable,
    /// The engine could not verify who is calling.
    IdentityUnverified,
    Internal,
}

/// What the agent can do about a [`Refusal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// The developer runs `raptor mcp enable` in the repo.
    AskTheDeveloperToRunRaptorMcpEnable,
    StartTheSessionInsideAnObservedRepo,
    CheckTheGitraptorInstallation,
    RetryLater,
}

/// The `structuredContent` of a refused tool call: no data of any repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub reason: Refusal,
    pub action: Action,
}

impl Refusal {
    pub fn refused(self) -> Refused {
        let action = match self {
            Self::RepoNotEnabled => Action::AskTheDeveloperToRunRaptorMcpEnable,
            Self::NotInObservedWorktree => Action::StartTheSessionInsideAnObservedRepo,
            Self::EngineUnavailable => Action::CheckTheGitraptorInstallation,
            Self::IdentityUnverified | Self::Internal => Action::RetryLater,
        };
        Refused {
            reason: self,
            action,
        }
    }
}

/// The lazy connection to the engine.
#[derive(Default)]
pub struct Engine {
    client: Mutex<Option<Client>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl Engine {
    /// `mcp.status`: the caller's repo, resolved by the engine. A broken
    /// connection (the engine restarted) is opened again once.
    pub fn status(&self) -> Result<McpStatus, Refusal> {
        let mut slot = self.client.lock().unwrap_or_else(|e| e.into_inner());
        let reused = slot.is_some();
        match call_status(&mut slot) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) if reused => {
                call_status(&mut slot).map_err(|err| refusal(&err))
            }
            other => other.map_err(|err| refusal(&err)),
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

fn refusal(err: &ClientError) -> Refusal {
    match err {
        ClientError::Rpc(e) if e.code == code::SCOPE_REFUSED => {
            let reason = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<ScopeRefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(ScopeRefusal::NotAllowlisted) => Refusal::RepoNotEnabled,
                _ => Refusal::NotInObservedWorktree,
            }
        }
        ClientError::Rpc(e) if e.code == code::IDENTITY_UNVERIFIED => Refusal::IdentityUnverified,
        ClientError::Rpc(_) | ClientError::Protocol(_) | ClientError::Unsupported(_) => {
            Refusal::Internal
        }
        // Not running and not startable, an incompatible engine, a rejected
        // channel or a broken connection: the installation must be checked.
        _ => Refusal::EngineUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::rpc::ErrorObject;

    #[test]
    fn scope_refusals_map_to_the_tool_reasons() {
        let scope = |reason| {
            ClientError::Rpc(
                ErrorObject::new(code::SCOPE_REFUSED, "x").with_data(ScopeRefusedData { reason }),
            )
        };
        assert_eq!(
            refusal(&scope(ScopeRefusal::NotAllowlisted)),
            Refusal::RepoNotEnabled
        );
        for reason in [ScopeRefusal::NotObserved, ScopeRefusal::NoWorkingFolder] {
            assert_eq!(refusal(&scope(reason)), Refusal::NotInObservedWorktree);
        }
        let refused = serde_json::to_value(Refusal::RepoNotEnabled.refused()).unwrap();
        assert_eq!(
            refused,
            serde_json::json!({
                "reason": "repo-not-enabled",
                "action": "ask-the-developer-to-run-raptor-mcp-enable"
            })
        );
    }
}
