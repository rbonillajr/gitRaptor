//! Typed settings document (ADR-GRP-007). The JSON Schema is generated from these types, and
//! each key declares the levels that admit it with `x-gitraptor-levels`, the single source of
//! that rule.

use schemars::JsonSchema;
use serde::Deserialize;

/// One settings document: `.gitraptor/settings.json` (team), `settings.json` (profile) or
/// `settings.local.json` (local). Strict JSON, no comments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(title = "GitRaptor settings")]
pub struct Settings {
    /// URL of the published schema, for editors. Never downloaded.
    #[serde(rename = "$schema")]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub schema: Option<String>,
    /// Engine values (ADR-GRP-007).
    pub engine: Option<Engine>,
    /// Permission of each governed operation (ADR-GRD-003, BR-VAL-002).
    pub permissions: Option<Permissions>,
    /// Repository policies (ADR-GRD-003). No key is supported yet: US-GRD-008, US-GRD-009 and
    /// US-GRD-015 add them.
    pub policies: Option<Policies>,
    /// Time Machine values (ADR-TMC-007).
    #[serde(rename = "timeMachine")]
    pub time_machine: Option<TimeMachine>,
}

/// `engine` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    /// Base branch of the repository. Only read from the copy of the main branch, and only
    /// effective once confirmed by the developer (ADR-GRD-004 § 3).
    #[schemars(extend("x-gitraptor-levels" = ["team"]))]
    pub base_branch: Option<String>,
    /// Minutes without activity before a session is idle.
    #[schemars(range(min = 1, max = 1440))]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "local"]))]
    pub idle_threshold_minutes: Option<u32>,
    /// Absolute path to the Git executable.
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub git_path: Option<String>,
    /// Watcher intervals (ADR-GRP-010 § 5).
    pub watcher: Option<Watcher>,
}

/// `engine.watcher` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Watcher {
    /// Fallback polling interval, in seconds.
    #[schemars(range(min = 5, max = 3600))]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "local"]))]
    pub fallback_poll_seconds: Option<u32>,
    /// Polling interval in degraded mode, in seconds.
    #[schemars(range(min = 1, max = 60))]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "local"]))]
    pub degraded_poll_seconds: Option<u32>,
}

/// `timeMachine` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TimeMachine {
    /// Days snapshots are kept.
    #[schemars(range(min = 1, max = 3650))]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "local"]))]
    pub retention_days: Option<u32>,
    /// Quota of the snapshot store, in GB.
    #[serde(rename = "storeQuotaGB")]
    #[schemars(range(min = 1))]
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub store_quota_gb: Option<u32>,
    /// Include credential files in snapshots.
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub include_credential_files: Option<bool>,
}

/// `permissions` section: a permission for each governed operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    /// Operations allowed. Only the copy of the main branch can relax.
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub allow: Option<Vec<Operation>>,
    /// Operations that need the developer's confirmation (denied while there is no queue).
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub ask: Option<Vec<Operation>>,
    /// Operations denied.
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub deny: Option<Vec<Operation>>,
    /// Turn off the safe minimum (deny force-push and the deletion of the base branch).
    /// Only effective from the confirmed copy of the main branch (ADR-GRD-003 § 2, D6).
    #[schemars(extend("x-gitraptor-levels" = ["team"]))]
    pub disable_safe_minimum: Option<bool>,
}

/// `policies` section. Open: its keys arrive with their stories.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
pub struct Policies {}

/// An operation governed by Guardrails (BR-VAL-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Commit,
    Push,
    ForcePush,
    ResetHard,
    BranchDelete,
    Rebase,
    Merge,
    WorktreeAdd,
    WorktreeRemove,
}

impl Operation {
    /// Every operation of the catalog.
    pub const ALL: [Operation; 9] = [
        Self::Commit,
        Self::Push,
        Self::ForcePush,
        Self::ResetHard,
        Self::BranchDelete,
        Self::Rebase,
        Self::Merge,
        Self::WorktreeAdd,
        Self::WorktreeRemove,
    ];

    /// Identifier used in the document.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Push => "push",
            Self::ForcePush => "force-push",
            Self::ResetHard => "reset-hard",
            Self::BranchDelete => "branch-delete",
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::WorktreeAdd => "worktree-add",
            Self::WorktreeRemove => "worktree-remove",
        }
    }
}

/// A configuration level (ADR-GRP-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Profile,
    Team,
    Local,
}

impl Level {
    /// Name used in `x-gitraptor-levels`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Team => "team",
            Self::Local => "local",
        }
    }
}
