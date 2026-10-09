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

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, method};
use crate::Actor;
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};
use crate::guard::{Diagnostic, LossCause};
use crate::mcp_view::MAX_MCP_NAME_CHARS;
use crate::messages::{RepoStateView, SessionStateView, UnavailableReason};
use crate::rpc::ErrorSpec;
use crate::untrusted::{Untrusted, UntrustedName};

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

/// `mcp.status` carries the caller's situation and the repo's, and pages of
/// worktrees and paths by cursor.
pub const CAP_MCP_STATUS_FULL: Capability = Capability::new("mcp.status-full");

/// The module's error block (ADR-GRP-016).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 4 * ERROR_BLOCK_LEN;

/// The repo or the worktree of the caller cannot be read now; `data` is a
/// [`McpUnavailableData`].
pub const MCP_UNAVAILABLE: ErrorSpec = ErrorSpec::new(BLOCK, "mcp-unavailable");

/// Characters of a cursor: lowercase hexadecimal.
pub const MCP_CURSOR_LEN: usize = 16;

/// Whether `text` has the form of a cursor the daemon hands out: exactly
/// [`MCP_CURSOR_LEN`] characters of `[0-9a-f]`.
pub fn valid_cursor(text: &str) -> bool {
    text.len() == MCP_CURSOR_LEN && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) const GROUP: Group = Group {
    methods: &[
        // New after the freeze: protocol 9 and later (clients of 5 to 8
        // keep the contract they were built for).
        method(MCP_ENABLE, true, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_DISABLE, true, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_ALLOWLIST, false, false).since(CAPABILITIES_PROTOCOL),
        method(MCP_STATUS, false, true).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[CAP_MCP_STATUS_BRANCH, CAP_MCP_STATUS_FULL],
    errors: &[MCP_UNAVAILABLE],
    error_block: Some(BLOCK),
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
    /// The caller's worktree; absent when there is nothing to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub here: Option<McpHere>,
    /// The repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<McpRepo>,
    /// A page asked for with a cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<McpPage>,
}

impl McpStatus {
    /// The status as the `status` tool gives it: names cut at their MCP
    /// bound (ADR-MCP-001 § 6). Escaping is `mcp_view::for_mcp`.
    pub fn for_mcp(&self) -> Self {
        let max = MAX_MCP_NAME_CHARS;
        Self {
            worktree: self.worktree.capped_chars(max),
            branch: self.branch.as_ref().map(|n| n.capped_chars(max)),
            requester: cap_actor(&self.requester, max),
            here: self.here.as_ref().map(|h| h.capped(max)),
            repo: self.repo.as_ref().map(|r| r.capped(max)),
            page: self.page.as_ref().map(|p| p.capped(max)),
            ..self.clone()
        }
    }
}

/// The actor with its declared name cut to `max` characters.
fn cap_actor(actor: &Actor, max: usize) -> Actor {
    match actor {
        Actor::Agent { kind, name, origin } => Actor::Agent {
            kind: *kind,
            name: name.as_ref().map(|n| n.capped_chars(max)),
            origin: *origin,
        },
        Actor::Unattributed => Actor::Unattributed,
    }
}

impl McpHere {
    /// The same situation with every name cut to `max` characters.
    pub fn capped(&self, max: usize) -> Self {
        Self {
            sessions: self
                .sessions
                .iter()
                .map(|s| McpSession {
                    actor: cap_actor(&s.actor, max),
                    state: s.state,
                })
                .collect(),
            ..self.clone()
        }
    }
}

impl McpRepo {
    /// The same repo with every name cut to `max` characters.
    pub fn capped(&self, max: usize) -> Self {
        Self {
            base: McpBase {
                name: self.base.name.as_ref().map(|n| n.capped_chars(max)),
                state: self.base.state,
            },
            ..self.clone()
        }
    }
}

impl McpPage {
    /// The same page with every name cut to `max` characters.
    pub fn capped(&self, max: usize) -> Self {
        Self {
            of: self.of.as_ref().map(|n| n.capped_chars(max)),
            worktrees: self
                .worktrees
                .iter()
                .map(|w| McpWorktree {
                    name: w.name.capped_chars(max),
                    branch: w.branch.as_ref().map(|n| n.capped_chars(max)),
                    state: w.state.capped(max),
                    ..w.clone()
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// `mcp.status` parameters: the cursor of a list the last answer did not
/// include.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpStatusParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A list the answer does not include: how many there are and the cursor to
/// ask for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpListRef {
    pub total: u64,
    pub cursor: String,
}

/// A present session of a worktree; never an ended one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpSession {
    pub actor: Actor,
    pub state: SessionStateView,
}

/// Why a worktree's ahead/behind is not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpUncounted {
    BaseMissing,
    NoBase,
    NoCommits,
    Unreadable,
}

/// The situation of one worktree. `ahead` and `behind` are left out at 0.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpHere {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<McpSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<McpListRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<u64>,
    /// The counts are lower bounds.
    #[serde(default, skip_serializing_if = "is_false")]
    pub at_least: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncounted: Option<McpUncounted>,
}

/// Whether the base branch is the one the developer confirmed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpBaseState {
    /// Left out of the answer.
    #[default]
    Confirmed,
    Unconfirmed,
    Pending,
    Invalid,
}

impl McpBaseState {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpBase {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<UntrustedName>,
    #[serde(default, skip_serializing_if = "McpBaseState::is_confirmed")]
    pub state: McpBaseState,
}

/// The protection of the repo as the MCP layer sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpProtectionState {
    McpOnly,
    Full,
}

/// What the engine is doing with the repo; absent means observing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpEngineState {
    Reconciling,
    Dormant,
    WaitingForGit,
}

/// A period the engine did not observe, in seconds before the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpGap {
    pub from_s_ago: u64,
    /// Absent: the gap is still open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_s_ago: Option<u64>,
}

/// The repo as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpRepo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<McpEngineState>,
    pub base: McpBase,
    pub protection: McpProtectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_lost: Option<LossCause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch_age_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<McpGap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gaps_total: Option<u32>,
    /// Sessions cannot be detected on this system: "unknown", not "none".
    #[serde(default, skip_serializing_if = "is_false")]
    pub sessions_unknown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees: Option<McpListRef>,
}

/// Another worktree of the repo, in a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct McpWorktree {
    pub name: UntrustedName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<UntrustedName>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub main: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<UnavailableReason>,
    #[serde(flatten)]
    pub state: McpHere,
}

/// One page of a list: other worktrees, or the paths of one worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpPage {
    /// The worktree whose paths these are; only in a page of paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<UntrustedName>,
    pub total: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktrees: Vec<McpWorktree>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Untrusted>,
    /// There is more after this page.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Why the caller's repo or worktree cannot be read, as the channel says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum McpUnavailable {
    WorktreeMissing,
    OtherOwner,
    WorktreeUntrusted,
    RepoUnreadable,
}

/// `data` of [`MCP_UNAVAILABLE`]: the reason and nothing of the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpUnavailableData {
    pub reason: McpUnavailable,
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
            here: None,
            repo: None,
            page: None,
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
            here: None,
            repo: None,
            page: None,
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
