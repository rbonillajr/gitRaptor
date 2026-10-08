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
    /// Repository policies (ADR-GRD-003). `commitAuthorship` (US-GRD-018) is the first key;
    /// US-GRD-008, US-GRD-009 and US-GRD-015 add others.
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
    /// Observation tiers (ADR-GRP-010, Enmienda 2026-10-07, N7).
    pub observation: Option<Observation>,
}

/// `engine.observation` section: when an observed repo goes dormant and how
/// its safety nets run. Never at the team level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// Hours without activity before a repo goes dormant.
    #[schemars(range(min = 1, max = 720))]
    #[schemars(extend("x-gitraptor-levels" = ["profile", "local"]))]
    pub dormant_after_hours: Option<u32>,
    /// Interval of the metadata sweep of the dormant repos, in seconds.
    #[schemars(range(min = 30, max = 900))]
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub dormant_poll_seconds: Option<u32>,
    /// Shortest interval of the slow reconciliation of a dormant repo, in
    /// minutes; the CPU budget may make it longer.
    #[schemars(range(min = 15, max = 1440))]
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub dormant_reconcile_minutes: Option<u32>,
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
    /// Backend of the file watcher on macOS: `fsevents` (default, the engine's own FSEvents
    /// stream) or `notify` (the library, the behaviour before ADR-GRP-010 Enmienda 2026-10-08;
    /// fallback for one release). Read when the daemon starts. No effect on Linux or Windows.
    #[schemars(extend("x-gitraptor-levels" = ["profile"]))]
    pub backend: Option<WatchBackend>,
}

/// `engine.watcher.backend`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WatchBackend {
    #[default]
    Fsevents,
    Notify,
}

impl WatchBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fsevents => "fsevents",
            Self::Notify => "notify",
        }
    }
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
#[serde(rename_all = "camelCase")]
#[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
pub struct Policies {
    /// Who makes the commits (US-GRD-018, BR-AUTH-005). Only the floor relaxes it to
    /// `flexible` (Q-GRD-20).
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub commit_authorship: Option<CommitAuthorship>,
    /// Branches that cannot be moved (US-GRD-008, BR-VAL-003). Only adds protection: every
    /// level's patterns apply.
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub protected_branches: Option<PatternPolicy>,
    /// Paths a commit cannot touch (US-GRD-008, BR-VAL-003). Only adds protection.
    #[schemars(extend("x-gitraptor-levels" = ["profile", "team", "local"]))]
    pub forbidden_paths: Option<PatternPolicy>,
}

/// `policies.protectedBranches` and `policies.forbiddenPaths`: a list of patterns and who they
/// apply to. A branch pattern matches the whole short name (`release/*` does not cover
/// `release/1.0/x`; `release/**` does). A path pattern follows `.gitignore`, reduced: with no
/// `/` apart from a trailing one it is found at any depth (`secrets/`, `*.pem`), with a `/`
/// in the middle or at the start it is anchored to the root (`config/prod.yml`, `/secrets`),
/// and a directory covers what is below it. No negations (`!`) and no comments (`#`). With
/// `appliesTo: everyone` the person is held too and, with no conscious exception in the MVP,
/// a protected branch is frozen for local pulls and fast-forwards as well.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatternPolicy {
    /// At most 64 patterns of at most 256 bytes. An invalid one (empty, with control bytes,
    /// starting with `!` or `#`, or a branch written as `refs/heads/…`) is dropped alone.
    #[schemars(extend("x-gitraptor-patterns" = true))]
    pub patterns: Option<Vec<String>>,
    /// `agents` (default): only detected or registered agents; `everyone`: also the person.
    pub applies_to: Option<AppliesTo>,
}

/// Who a `protectedBranches` or `forbiddenPaths` rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AppliesTo {
    Agents,
    Everyone,
}

/// `policies.commitAuthorship`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommitAuthorship {
    /// `agents-commit` (default), `human-author` or `flexible`.
    pub mode: Option<AuthorshipMode>,
    /// With `human-author` only: `deny` (default) or `warn`.
    pub on_agent_commit: Option<OnAgentCommit>,
}

/// The authorship mode of a repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorshipMode {
    AgentsCommit,
    HumanAuthor,
    Flexible,
}

/// What `human-author` does with an agent's commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OnAgentCommit {
    Deny,
    Warn,
}

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
