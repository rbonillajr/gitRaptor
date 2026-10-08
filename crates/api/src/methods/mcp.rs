//! The MCP allowlist and the `status` of the MCP (US-MCP-002, US-MCP-003,
//! ADR-MCP-001 § 2 and § 3).
//!
//! The allowlist is a mark of the observed repo, written only by the daemon
//! on the reserved `mcp.enable` and `mcp.disable`; `mcp.allowlist` reads it
//! for the CLI and the TUI, never over MCP. `mcp.status` is the one read of
//! the `status` tool: the repo and the worktree come from the caller's
//! working folder, never from a parameter.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Group, method};
use crate::Actor;
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};
use crate::messages::RepoStateView;
use crate::untrusted::UntrustedName;

/// Enables a repo for the MCP (reserved).
pub const MCP_ENABLE: &str = "mcp.enable";
/// Takes a repo out of the MCP allowlist (reserved).
pub const MCP_DISABLE: &str = "mcp.disable";
/// The repos in the MCP allowlist. Read-only; never over MCP.
pub const MCP_ALLOWLIST: &str = "mcp.allowlist";
/// The caller's repo and worktree, only if the repo is enabled.
pub const MCP_STATUS: &str = "mcp.status";

/// `mcp.status` carries the branch of the caller's worktree (US-MCP-005).
pub const CAP_MCP_STATUS_BRANCH: Capability = Capability::new("mcp.status-branch");

pub(super) const GROUP: Group = Group {
    methods: &[
        // New after the freeze: protocol 9 and later (clients of 5 to 8
        // keep the contract they were built for).
        method(MCP_ENABLE, true, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_DISABLE, true, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_ALLOWLIST, false, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_STATUS, false, true).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[CAP_MCP_STATUS_BRANCH],
    ..Group::new("mcp")
};

/// `mcp.enable` and `mcp.disable` parameters: any folder of the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpRepoParams {
    pub path: String,
}

/// `mcp.enable` and `mcp.disable` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpRepoResult {
    pub repo_id: String,
    /// Whether the repo is in the allowlist now.
    pub enabled: bool,
    /// Whether this call changed it (repeating is idempotent).
    pub changed: bool,
}

/// `mcp.allowlist` result: the enabled repos, by key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpAllowlistResult {
    pub repo_ids: Vec<String>,
}

/// `mcp.status` result (SEC-12: a field allowlist, no paths).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpStatus {
    pub repo_id: String,
    pub repo_state: RepoStateView,
    /// Name of the folder of the worktree the caller acts in, never its path.
    pub worktree: UntrustedName,
    /// The branch of that worktree; absent with a detached `HEAD`, a
    /// worktree that cannot be read, and for a connection without
    /// [`CAP_MCP_STATUS_BRANCH`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<UntrustedName>,
    /// Whether it is the repo's main worktree.
    pub main: bool,
    pub requester: Actor,
    /// What the caller can do next; absent for an attributed agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<McpStatusAction>,
}

impl McpStatus {
    /// The status as the `status` tool gives it: names cut at their MCP
    /// bound (ADR-MCP-001 § 6). Escaping is `mcp_view::for_mcp`.
    pub fn for_mcp(&self) -> Self {
        let requester = match &self.requester {
            Actor::Agent { kind, name, origin } => Actor::Agent {
                kind: *kind,
                name: name.as_ref().map(UntrustedName::mcp_name),
                origin: *origin,
            },
            Actor::Unattributed => Actor::Unattributed,
        };
        Self {
            worktree: self.worktree.mcp_name(),
            branch: self.branch.as_ref().map(UntrustedName::mcp_name),
            requester,
            ..self.clone()
        }
    }
}

/// What an `mcp.status` caller can do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpStatusAction {
    /// "Unattributed" reads, but must register to write (BR-MCP-ELIG-006).
    RegisterToWrite,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentKind, AgentOrigin};

    /// SEC-12: the status carries exactly the allowlisted fields.
    #[test]
    fn mcp_status_is_the_field_allowlist() {
        let status = McpStatus {
            repo_id: "abc".into(),
            repo_state: RepoStateView::Observed,
            worktree: UntrustedName::new("shop-feat-a"),
            branch: None,
            main: false,
            requester: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            action: None,
        };
        let value = serde_json::to_value(&status).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["main", "repo_id", "repo_state", "requester", "worktree"]
        );
        let unattributed = McpStatus {
            requester: Actor::Unattributed,
            action: Some(McpStatusAction::RegisterToWrite),
            ..status
        };
        let value = serde_json::to_value(&unattributed).unwrap();
        assert_eq!(value["action"], "register-to-write");
        let with_branch = McpStatus {
            branch: Some(UntrustedName::new("feat-a")),
            ..unattributed
        };
        let value = serde_json::to_value(&with_branch).unwrap();
        assert_eq!(value["branch"], serde_json::json!({"untrusted": "feat-a"}));
    }

    /// ADR-MCP-001 § 6: every name of the tool's status is cut at 100
    /// characters, marked `truncated`.
    #[test]
    fn the_mcp_status_cuts_every_name() {
        let long = UntrustedName::new("n".repeat(1024));
        let status = McpStatus {
            repo_id: "abc".into(),
            repo_state: RepoStateView::Observed,
            worktree: long.clone(),
            branch: Some(long.clone()),
            main: true,
            requester: Actor::Agent {
                kind: AgentKind::Other,
                name: Some(long),
                origin: AgentOrigin::Registered,
            },
            action: None,
        }
        .for_mcp();
        let Actor::Agent {
            name: Some(name), ..
        } = &status.requester
        else {
            panic!("an agent");
        };
        for text in [&status.worktree, status.branch.as_ref().unwrap(), name] {
            assert_eq!(
                text.raw().chars().count(),
                crate::mcp_view::MAX_MCP_NAME_CHARS
            );
            assert!(text.is_truncated());
        }
    }

    /// Reserved commands are never offered over MCP; the read of the
    /// allowlist is not either.
    #[test]
    fn only_status_is_offered_over_mcp() {
        let mcp: Vec<_> = GROUP
            .methods
            .iter()
            .filter(|m| m.mcp)
            .map(|m| m.name)
            .collect();
        assert_eq!(mcp, [MCP_STATUS]);
        let reserved: Vec<_> = GROUP
            .methods
            .iter()
            .filter(|m| m.reserved)
            .map(|m| m.name)
            .collect();
        assert_eq!(reserved, [MCP_ENABLE, MCP_DISABLE]);
    }
}
