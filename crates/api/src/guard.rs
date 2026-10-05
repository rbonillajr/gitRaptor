//! Guardrails contract (ADR-GRD-003 § 3, ADR-GRD-005 § 3, ADR-GRD-007 § 1):
//! the decision on a governed operation, the install plan shown before the
//! developer grants the permission, and the protection status.
//!
//! Codes travel in English kebab-case and are translated by each client
//! (NFR-10). Every parameter is labelled data marked untrusted (SEC-12): a
//! client prints it only sanitized, inside a fixed template per code (M-05).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Untrusted;

/// Effect of a decision, ordered by restriction: `deny > ask > allow`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    Allow,
    Ask,
    Deny,
}

/// Where a rule comes from (ADR-GRD-003 § 3). `system` covers the causes the
/// layer adds itself: degraded mode, channel not authentic, repo mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Minimum,
    Floor,
    Worktree,
    Profile,
    Local,
    System,
}

/// The rule behind a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Rule {
    /// Forbid force-push, any branch, any remote (BR-EDGE-001).
    #[serde(rename = "minimum.force-push")]
    MinimumForcePush,
    /// Protect the base branch from deletion, local or remote.
    #[serde(rename = "minimum.base-branch-delete")]
    MinimumBaseBranchDelete,
    /// The daemon could not be used: the client evaluated alone, stricter.
    #[serde(rename = "system.degraded")]
    Degraded,
    /// The channel server is not the installed binary (SEC-GRD-16).
    #[serde(rename = "system.channel-not-authentic")]
    ChannelNotAuthentic,
    /// The transaction is not in the repo the dispatcher was installed for
    /// (M-02, SEC-GRD-19).
    #[serde(rename = "system.repo-mismatch")]
    RepoMismatch,
    /// The hook input is malformed or exceeds a bound (SEC-GRD-07).
    #[serde(rename = "system.input-rejected")]
    InputRejected,
    /// The evaluation failed: fail-closed on governed refs (ADR-GRD-001 § 3).
    #[serde(rename = "system.internal-error")]
    InternalError,
}

/// Why a rule matched (ADR-GRD-003 § 3 and its 2026-10-04 amendment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// The remote tip is not an ancestor of the pushed commit.
    NotFastForward,
    /// The remote tip is missing locally: treated as forced (H-05).
    RemoteObjectMissing,
    /// `historia-superficial`: a shallow clone cannot prove a fast-forward.
    ShallowHistory,
    /// The base branch itself is deleted.
    Delete,
    /// A name that only differs from the base branch in case or Unicode form
    /// (SEC-GRD-18): denied as ambiguous.
    Alias,
    /// `renombrado-sobre-base`: `branch -M x <base>` with the files backend;
    /// Git already deleted `x`, the parameters carry the oid to recover it.
    RenameOntoBase,
    /// Degraded mode: the daemon is not running or cannot be reached.
    DaemonUnreachable,
    /// Degraded mode: an authentic daemon of another profile instance.
    InstanceMismatch,
}

/// Kind of a labelled parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ParamKind {
    Branch,
    Ref,
    Remote,
    Oid,
    Base,
}

/// One labelled, untrusted parameter of a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub kind: ParamKind,
    pub value: Untrusted,
}

impl Param {
    pub fn new(kind: ParamKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: Untrusted::new(value),
        }
    }
}

/// One rule that produces the decision (BR-CALC-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    pub rule: Rule,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
}

/// Exception state of a decision (ADR-GRD-007). Always `none` until US-GRD-006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExceptionState {
    None,
    Applied,
    Rejected,
}

/// State of one configuration source (ADR-GRD-003 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigStatus {
    Ok,
    Absent,
    Unreadable,
    Partial,
    PendingConfirmation,
    /// Not read by this evaluation: US-GRD-001 applies the minimum only.
    NotRead,
}

/// A configuration source and its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigSource {
    pub source: Level,
    pub status: ConfigStatus,
}

/// The decision on one governed operation (ADR-GRD-003 § 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Decision {
    /// Opaque identifier.
    pub decision_id: String,
    pub effect: Effect,
    /// What the layer applies: without the queue, `ask` is applied as `deny`
    /// (S-GRD-9).
    pub applied_effect: Effect,
    pub reasons: Vec<Reason>,
    pub exception: ExceptionState,
    pub config_status: Vec<ConfigSource>,
    /// Ids of the configuration blobs evaluated.
    pub config_ref: Vec<String>,
}

/// The hook a dispatcher serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Hook {
    PrePush,
    PreRebase,
    ReferenceTransaction,
}

impl Hook {
    /// The mandatory dispatchers of US-GRD-001 (ADR-GRD-001 § 2, Enmienda).
    pub const MANDATORY: [Self; 3] = [Self::PrePush, Self::ReferenceTransaction, Self::PreRebase];

    /// Git's name of the hook.
    pub fn git_name(self) -> &'static str {
        match self {
            Self::PrePush => "pre-push",
            Self::PreRebase => "pre-rebase",
            Self::ReferenceTransaction => "reference-transaction",
        }
    }

    pub fn from_git_name(name: &str) -> Option<Self> {
        Self::MANDATORY.into_iter().find(|h| h.git_name() == name)
    }
}

/// A ref value of a transition: an object id, zero (absent) or a symbolic
/// target (`ref:<target>`, ADR-GRD-002 § 4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RefValue {
    Zero,
    Oid(String),
    Symbolic(String),
}

impl RefValue {
    pub fn is_zero(&self) -> bool {
        matches!(self, Self::Zero)
    }

    pub fn oid(&self) -> Option<&str> {
        match self {
            Self::Oid(o) => Some(o),
            _ => None,
        }
    }
}

/// One ref update of a `reference-transaction`, with the ref already
/// normalized by the hook client (`HEAD` resolved, ADR-GRD-002 § 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefUpdate {
    pub refname: String,
    pub old: RefValue,
    pub new: RefValue,
}

/// One line of `pre-push`: the local side and the remote ref it updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PushUpdate {
    /// `None` for a deletion (`(delete)` in Git's input).
    pub local_ref: Option<String>,
    pub local: RefValue,
    pub remote_ref: String,
    pub remote: RefValue,
}

/// The normalized operation the hook client sends (ADR-GRD-002 § 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Operation {
    Push {
        /// Remote name, or the URL without `userinfo` (M-06).
        remote: Untrusted,
        updates: Vec<PushUpdate>,
    },
    RefTransaction {
        updates: Vec<RefUpdate>,
        /// `HEAD` of the transaction is symbolic to a branch that no longer
        /// exists: the source of `branch -M x <base>` (oid from the `HEAD`
        /// reflog), to recover it if the rename is denied.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        orphan_head: Option<OrphanHead>,
    },
    Rebase {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upstream: Option<Untrusted>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<Untrusted>,
    },
}

/// The branch `HEAD` names but that is gone, and the last oid it had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrphanHead {
    pub branch: String,
    pub oid: String,
}

/// `guard.evaluate` parameters: sent by `raptor hook` with the constants of
/// its dispatcher (ADR-GRD-001 § 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateParams {
    /// Repo id fixed in the dispatcher.
    pub repo_id: String,
    /// Canonical common directory fixed in the dispatcher.
    pub common_dir: String,
    pub hook: Hook,
    pub operation: Operation,
}

/// Protection state of the hook layer (BR-WF-002, ADR-GRD-005 § 3). Until the
/// MCP layer exists, only these two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectionState {
    Unprotected,
    HooksOnly,
}

/// The developer's answer to the permission (BR-AUTH-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    NotAsked,
    Granted,
    Denied,
}

/// Why the hook layer cannot be installed in a repo (ADR-GRD-001 § 4 paso 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InstallBlocker {
    /// Hooks to chain (own hooks, husky, lefthook, pre-commit or any
    /// `core.hooksPath`): US-GRD-002.
    PriorHooks,
    /// `extensions.worktreeConfig` with `core.hooksPath` in a worktree.
    WorktreeConfig,
    /// An `include`/`includeIf` that defines `core.hooksPath`.
    IncludeDefinesHooksPath,
    /// Any `includeIf "onbranch:…"` at local or worktree level.
    IncludeIfOnbranch,
    /// A constant of the dispatcher with a newline, tab or NUL (M-04).
    NotRepresentable,
    /// A `gitraptor/` folder the journal does not know (`instalacion-huerfana`).
    OrphanFolder,
    /// Already installed.
    AlreadyInstalled,
    /// The installed `raptor-hook` dispatcher is missing next to `raptor`.
    DispatcherMissing,
    /// The repo is bare: no worktree to protect in this story.
    Bare,
    /// The platform has no daemon channel yet (Windows).
    PlatformUnsupported,
}

/// What the developer cannot prevent with the hook layer in this repo
/// (ADR-GRD-002 § 3), as codes the client translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NotPreventable {
    ResetHard,
    Merge,
    RemoveWorktree,
    /// Create a worktree without a new branch (`no-reconocible`).
    CreateWorktreeUnrecognized,
    /// Reftable only: `branch -m/-M` renames over or away from the base.
    RenameBaseReftable,
    /// Voluntary skips: `--no-verify`, `-c core.hooksPath`, `GIT_CONFIG_*`,
    /// `send-pack`, hand edits of `refs/`.
    VoluntarySkips,
}

/// Refs backend of the repo (`extensions.refStorage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RefBackend {
    Files,
    Reftable,
}

/// `guard.plan`, `guard.install`, `guard.decline` and `guard.status`
/// parameters: the repo by path, as the developer gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardRepoParams {
    pub path: String,
}

/// `guard.plan` result: everything the explanation needs (BR-AUTH-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardPlan {
    pub repo_id: String,
    /// Canonical common directory.
    pub common_dir: Untrusted,
    /// Worktrees the key reaches today (it also reaches future ones).
    pub worktrees: Vec<Untrusted>,
    /// Dispatchers that would be written.
    pub hooks: Vec<Hook>,
    /// Folder of the dispatchers (`<common>/gitraptor/hooks`).
    pub hooks_dir: Untrusted,
    pub backend: RefBackend,
    pub not_preventable: Vec<NotPreventable>,
    /// Branch the install confirms (`main`), or `None` when the repo has a
    /// team configuration and the base stays unconfirmed (Q-GRD-23).
    pub confirms_base: Option<Untrusted>,
    /// Base branches the minimum protects after the install.
    pub protected_bases: Vec<Untrusted>,
    /// Why it cannot be installed; empty when it can.
    pub blockers: Vec<InstallBlocker>,
    pub status: GuardStatus,
}

/// `guard.status` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardStatus {
    pub repo_id: String,
    pub state: ProtectionState,
    pub permission: Permission,
    /// GitRaptor may offer the permission (never after a denial).
    pub offer: bool,
    /// Base branches the minimum protects.
    pub protected_bases: Vec<Untrusted>,
    /// Whether the base branch is the confirmed one.
    pub base_confirmed: bool,
    pub not_preventable: Vec<NotPreventable>,
    /// The last install attempt that was refused, with its causes
    /// (BR-EDGE-002); a refusal is not a denial of the permission.
    pub last_refusal: Vec<InstallBlocker>,
}

/// `data` of a `GUARD_REJECTED` error: the install did not happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardRejectedData {
    pub blockers: Vec<InstallBlocker>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_travel_in_kebab_case() {
        let reason = Reason {
            rule: Rule::MinimumForcePush,
            level: Level::Minimum,
            cause: Some(Cause::ShallowHistory),
            params: vec![Param::new(ParamKind::Branch, "feat-x")],
        };
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(
            json,
            r#"{"rule":"minimum.force-push","level":"minimum","cause":"shallow-history","params":[{"kind":"branch","value":{"untrusted":"feat-x"}}]}"#
        );
        assert_eq!(serde_json::from_str::<Reason>(&json).unwrap(), reason);
    }

    #[test]
    fn effects_are_ordered_by_restriction() {
        assert!(Effect::Deny > Effect::Ask && Effect::Ask > Effect::Allow);
    }

    #[test]
    fn hooks_round_trip_their_git_names() {
        for h in Hook::MANDATORY {
            assert_eq!(Hook::from_git_name(h.git_name()), Some(h));
        }
        assert_eq!(Hook::from_git_name("pre-commit"), None);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<GuardRepoParams>(r#"{"path":"/r","x":1}"#).is_err());
    }
}
