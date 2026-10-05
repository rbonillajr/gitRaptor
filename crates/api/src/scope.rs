//! Scopes of the event stream and what the Cockpit needs from the channel
//! (protocol 5, ADR-CKP-003 § 4 N1 to N5).
//!
//! Every event belongs to one scope: the global one (engine state, observed
//! repos) or one repo's (its worktrees, Git events and operations). Each
//! scope has its own contiguous sequence, so a client keeps a coherent
//! replica per scope: a snapshot at `scope_seq = N`, then a subscription
//! from `N + 1`; an event above `last + 1` is a gap and asks for a new
//! snapshot (DEP-CKP-6).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::Layer;
use crate::event::Event;
use crate::messages::{DaemonView, EngineView, RepoStateView, RepoView, ResyncReason};
use crate::{Actor, Untrusted};

/// A scope of the event stream.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "scope", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Scope {
    /// Engine state and the observed repos.
    Global,
    /// One observed repo: its worktrees, Git events and operations.
    Repo { repo_id: String },
}

/// `scope.snapshot` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeSnapshotParams {
    pub scope: Scope,
}

/// `scope.snapshot` result: it reflects every event of its scope up to
/// `scope_seq` included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "kebab-case")]
pub enum ScopeSnapshot {
    Global(GlobalSnapshot),
    Repo(RepoSnapshot),
}

impl ScopeSnapshot {
    pub fn scope_seq(&self) -> u64 {
        match self {
            Self::Global(s) => s.scope_seq,
            Self::Repo(s) => s.scope_seq,
        }
    }

    pub fn run_id(&self) -> &str {
        match self {
            Self::Global(s) => &s.run_id,
            Self::Repo(s) => &s.run_id,
        }
    }
}

/// The global scope (N3): engine state, the observed repos with what asks
/// for attention in each, and whether the login autostart is registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobalSnapshot {
    pub run_id: String,
    pub scope_seq: u64,
    pub engine: EngineView,
    pub daemon: DaemonView,
    pub autostart: AutostartView,
    pub repos: Vec<RepoSummaryView>,
}

/// One repo's scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoSnapshot {
    pub run_id: String,
    pub scope_seq: u64,
    pub repo: RepoView,
}

/// A repo in the global scope, for the repo selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoSummaryView {
    pub repo_id: String,
    pub state: RepoStateView,
    /// Canonical path of the Git common directory.
    pub path: Untrusted,
    pub attention: AttentionView,
}

/// What asks for attention in a repo (Q-CKP-28): predicted conflicts (⚡),
/// Guardrails denials (⛔) and unattributed gaps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttentionView {
    pub conflicts: AttentionCount,
    pub denials: AttentionCount,
    pub gaps: AttentionCount,
}

impl AttentionView {
    /// Nothing is published yet: the predictor (TS-CKP-001), Guardrails and
    /// US-GRP-005 publish each count.
    pub const fn unpublished() -> Self {
        let none = AttentionCount::Unavailable {
            reason: UnavailableCause::NotPublished,
        };
        Self {
            conflicts: none,
            denials: none,
            gaps: none,
        }
    }
}

/// A count, or why there is none: never a zero that was not counted
/// (BR-CKP-CALC-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AttentionCount {
    Counted { count: u32 },
    Unavailable { reason: UnavailableCause },
}

/// Why a value is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableCause {
    /// No component of this engine publishes it yet.
    NotPublished,
}

impl UnavailableCause {
    pub const ALL: [Self; 1] = [Self::NotPublished];
}

/// Whether the login autostart is registered (ADR-GRP-005 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AutostartView {
    Registered,
    NotRegistered,
    /// The engine cannot tell yet (registration belongs to US-GRP-004).
    Unknown,
}

impl AutostartView {
    pub const ALL: [Self; 3] = [Self::Registered, Self::NotRegistered, Self::Unknown];
}

/// Data of a `repo.attention` event (global scope): the attention of one
/// repo changed. Declared ahead of its publishers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoAttentionData {
    pub repo_id: String,
    pub attention: AttentionView,
}

/// `scope.subscribe` parameters. `from_seq` is a sequence of the scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeSubscribeParams {
    pub scope: Scope,
    /// First scope sequence wanted. `None`: only events from now on.
    #[serde(default)]
    pub from_seq: Option<u64>,
    /// `run_id` of the snapshot `from_seq` continues.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeSubscribeResult {
    pub subscription: u32,
    pub scope: Scope,
    /// Scope sequence of the first event this subscription delivers.
    pub from_seq: u64,
}

/// Params of a `scope.event` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeEventNotification {
    pub subscription: u32,
    pub scope: Scope,
    /// Contiguous within the scope: the previous event of this scope had
    /// `scope_seq - 1` (N2).
    pub scope_seq: u64,
    pub event: Event,
}

/// Params of a `scope.resync` notification: the scope cannot continue from
/// the requested point; take a new snapshot of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeResyncNotification {
    pub scope: Scope,
    pub reason: ResyncReason,
}

/// `repo.locate` parameters (N4): any path inside a worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoLocateParams {
    pub path: String,
}

/// `repo.locate` result: the observed repo and the root of its worktree
/// that contain the path, as the daemon canonicalized it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoLocateResult {
    pub repo_id: String,
    pub worktree: Untrusted,
}

/// Who the daemon sees on a connection (N5). UX only: the daemon resolves
/// the requester again on every request (ADR-CKP-002 § 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConnectionRequester {
    Resolved {
        /// "Agent X" or "unattributed".
        actor: Actor,
        /// The layer the daemon fixes (ADR-CKP-002 § 4): what this
        /// connection may run.
        layer: Layer,
        /// Whether the daemon would issue a confirmation challenge.
        confirmable: bool,
    },
    /// The caller's identity could not be verified: no writes.
    Unverified,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_forms() {
        assert_eq!(
            serde_json::to_value(Scope::Global).unwrap(),
            json!({"scope": "global"})
        );
        let repo = Scope::Repo {
            repo_id: "r1".into(),
        };
        assert_eq!(
            serde_json::to_value(&repo).unwrap(),
            json!({"scope": "repo", "repo_id": "r1"})
        );
        assert!(serde_json::from_value::<Scope>(json!({"scope": "repo"})).is_err());
        assert_eq!(
            serde_json::to_value(AttentionView::unpublished().gaps).unwrap(),
            json!({"state": "unavailable", "reason": "not-published"})
        );
        let requester = ConnectionRequester::Resolved {
            actor: Actor::Unattributed,
            layer: Layer::Cockpit,
            confirmable: true,
        };
        let value = serde_json::to_value(&requester).unwrap();
        assert_eq!(value["state"], "resolved");
        assert_eq!(value["layer"], "cockpit");
        assert_eq!(
            serde_json::from_value::<ConnectionRequester>(value).unwrap(),
            requester
        );
    }

    #[test]
    fn snapshot_is_tagged_by_scope() {
        let snapshot = ScopeSnapshot::Repo(RepoSnapshot {
            run_id: "run".into(),
            scope_seq: 4,
            repo: RepoView {
                repo_id: "r1".into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/.git"),
                base: crate::messages::BaseBranchView {
                    name: None,
                    status: crate::messages::BaseStatusView::Invalid,
                },
                worktrees: Vec::new(),
            },
        });
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["scope"], "repo");
        assert_eq!(value["scope_seq"], 4);
        let back: ScopeSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(back, snapshot);
        assert_eq!(back.scope_seq(), 4);
        assert_eq!(back.run_id(), "run");
    }
}
