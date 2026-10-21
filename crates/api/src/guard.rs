//! Guardrails contract (ADR-GRD-003 § 3, ADR-GRD-005 § 3, ADR-GRD-007 § 1):
//! the decision on a governed operation, the install plan shown before the
//! developer grants the permission, and the protection status.
//!
//! Codes travel in English kebab-case and are translated by each client
//! (NFR-10). Every parameter is labelled data marked untrusted (SEC-12): a
//! client prints it only sanitized, inside a fixed template per code (M-05).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AgentKind, Untrusted, UntrustedName};

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
    /// An agent's commit must carry its trailer (`agents-commit`, US-GRD-018).
    #[serde(rename = "authorship.trailer-required")]
    AuthorshipTrailerRequired,
    /// Commits are made by the person (`human-author`, US-GRD-018).
    #[serde(rename = "authorship.human-author")]
    AuthorshipHumanAuthor,
    /// A protected branch is moved (`policies.protectedBranches`, US-GRD-008).
    #[serde(rename = "policy.protected-branch")]
    ProtectedBranch,
    /// A commit touches a forbidden path (`policies.forbiddenPaths`, US-GRD-008).
    #[serde(rename = "policy.forbidden-path")]
    ForbiddenPath,
    /// An agent's commit changes the Guardrails configuration (`.gitraptor/`). A product rule:
    /// no key turns it off. Only with `guard.config-protection`; without it the daemon sends it
    /// as `policy.forbidden-path`.
    #[serde(rename = "policy.config-protected")]
    ConfigProtected,
    /// A level that only hardens (worktree, profile, local) tried to relax a team rule; it was
    /// ignored. Only in decision-log notices, never in a decision sent to a hook.
    #[serde(rename = "config.relax-ignored")]
    RelaxIgnored,
    /// An allowed destructive operation whose prior snapshot failed (US-GRD-017, BR-EDGE-005).
    /// Only with `guard.snapshot-required`; without it the daemon sends `system.internal-error`.
    #[serde(rename = "system.snapshot-failed")]
    SnapshotFailed,
    /// The permission of `commit` (BR-VAL-002, US-GRD-007). Each `permission.*` rule names the
    /// operation of the catalog whose permission (`ask` or `deny`) produces the effect. Only with
    /// `guard.permissions`.
    #[serde(rename = "permission.commit")]
    PermissionCommit,
    /// The permission of `push`.
    #[serde(rename = "permission.push")]
    PermissionPush,
    /// The permission of `force-push`.
    #[serde(rename = "permission.force-push")]
    PermissionForcePush,
    /// The permission of `reset-hard`: not produced by the hook layer, reserved for the MCP one.
    #[serde(rename = "permission.reset-hard")]
    PermissionResetHard,
    /// The permission of `branch-delete`.
    #[serde(rename = "permission.branch-delete")]
    PermissionBranchDelete,
    /// The permission of `rebase`.
    #[serde(rename = "permission.rebase")]
    PermissionRebase,
    /// The permission of `merge`.
    #[serde(rename = "permission.merge")]
    PermissionMerge,
    /// The permission of `worktree-add`.
    #[serde(rename = "permission.worktree-add")]
    PermissionWorktreeAdd,
    /// The permission of `worktree-remove`: not produced by the hook layer, reserved for the MCP
    /// one.
    #[serde(rename = "permission.worktree-remove")]
    PermissionWorktreeRemove,
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
    /// The commit message could not be read (missing, larger than 64 KiB, not a regular
    /// file): no trailer can be proved (US-GRD-018, D7).
    MessageUnreadable,
    /// The second line saw a shape it cannot classify as one commit (US-GRD-018, § 5.3).
    AuthorshipUnclassified,
    /// The commits of the movement could not be read within the bounds, or a name could not be
    /// compared: nothing can be said, so it is denied (US-GRD-008, D5).
    Unverifiable,
    /// `system.snapshot-failed` (US-GRD-017): the prior snapshot ran out of time.
    SnapshotTimeLimit,
    /// `system.snapshot-failed`: not enough disk space for the prior snapshot.
    SnapshotNoSpace,
    /// `system.snapshot-failed`: too many prior snapshots in the window.
    SnapshotQuotaExceeded,
    /// `system.snapshot-failed`: the repo changed while the prior snapshot was saved.
    SnapshotDiscarded,
    /// `system.snapshot-failed`: no working tree to save.
    SnapshotNoWorktree,
    /// `system.snapshot-failed`: no Time Machine for this repo, or no daemon (degraded mode).
    SnapshotUnavailable,
    /// `system.snapshot-failed`: an internal error of the Time Machine.
    SnapshotInternal,
    /// A `permission.*` rule with `ask`: it needs a human confirmation that is not available, so
    /// it is applied as `deny` (US-GRD-007). Only with `guard.permissions`.
    AskUnavailable,
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
    /// The agent a rule names (`claude-code`).
    Agent,
    /// The example trailer of the agents' table.
    Example,
    /// The pattern of a protected branch or forbidden path that was not met (US-GRD-008).
    Pattern,
    /// A path a commit touches (US-GRD-008).
    Path,
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
    /// Rules that warn without changing the effect (capability `guard.authorship`). Only with
    /// `appliedEffect = allow`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Reason>,
    /// The `hook-prior` snapshot the daemon took inside this call (capability
    /// `guard.prior-snapshot`). Absent when none was due or the connection did not ask for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_snapshot: Option<crate::timemachine::HookPriorSnapshot>,
}

/// The hook a dispatcher serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Hook {
    PrePush,
    PreRebase,
    ReferenceTransaction,
    /// Commit authorship (US-GRD-018, template 2).
    PreCommit,
    /// Commit authorship (US-GRD-018, template 2).
    CommitMsg,
}

impl Hook {
    /// The mandatory dispatchers of US-GRD-001 (ADR-GRD-001 § 2, Enmienda).
    pub const MANDATORY: [Self; 3] = [Self::PrePush, Self::ReferenceTransaction, Self::PreRebase];

    /// Every dispatcher of the current template (2): the mandatory ones and the commit ones
    /// of US-GRD-018 (ADR-GRD-002 § 1, fila Commit).
    pub const ALL: [Self; 5] = [
        Self::PrePush,
        Self::ReferenceTransaction,
        Self::PreRebase,
        Self::PreCommit,
        Self::CommitMsg,
    ];

    /// Git's name of the hook.
    pub fn git_name(self) -> &'static str {
        match self {
            Self::PrePush => "pre-push",
            Self::PreRebase => "pre-rebase",
            Self::ReferenceTransaction => "reference-transaction",
            Self::PreCommit => "pre-commit",
            Self::CommitMsg => "commit-msg",
        }
    }

    pub fn from_git_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.git_name() == name)
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
    /// A commit (also `--amend` and a merge commit), only sent when the daemon granted
    /// `guard.authorship` or `guard.permissions`.
    Commit { stage: CommitStage },
}

/// Where a commit is evaluated (DS-US-GRD-018 § 5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CommitStage {
    /// Before the message: only `human-author` with `deny` applies.
    PreCommit,
    /// With the facts of the message: every authorship rule.
    CommitMsg,
    /// `reference-transaction` `prepared`, with the facts of the new commit: every authorship
    /// rule, unless the same `git` process already had its decision or certainly is a rebase,
    /// cherry-pick, revert or am (§ 5.3). Only with `guard.authorship.second-line`.
    SecondLine,
}

/// What the hook client derived from a commit message (D7): never its text, names or emails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthorshipFacts {
    /// One entry per `Co-Authored-By`; `None` = not recognised by the table.
    pub coauthors: Vec<Option<AgentKind>>,
    /// Version of the agents' trailer table.
    pub trailer_table: u32,
    /// The message was missing, larger than 64 KiB or not a regular file.
    #[serde(default)]
    pub unreadable: bool,
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
    /// Commit facts the hook client derived from the message (never the text). Sent only when
    /// the daemon granted `guard.authorship` (D11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorship: Option<AuthorshipFacts>,
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
    /// `core.hooksPath`). Only US-GRD-001 refused them; since US-GRD-002 they are chained, and
    /// this is what a connection without `guard.prior-hooks` reads for [`Self::ChainImpossible`]
    /// and what an old stored refusal says.
    PriorHooks,
    /// The prior hooks cannot be chained without altering them (`encadenado-imposible`,
    /// BR-EDGE-002): a prior `core.hooksPath` that is not representable, that points into the
    /// Guardrails folder, that Git cannot resolve the same way for every run (`~user`,
    /// `%(prefix)`, a `~/` under another `HOME`), defined more than once at local level, or a
    /// configuration that could not be read. Needs `guard.prior-hooks` (US-GRD-002).
    ChainImpossible,
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
    /// The protected branches and forbidden paths tell an agent from the person by its process:
    /// an agent the daemon does not detect or know counts as the person, and without the daemon
    /// only the rules for everyone apply (US-GRD-008). The same holds for the protection of the
    /// Guardrails configuration.
    PolicyActor,
    /// A forbidden path is checked in the commits that reach a branch or are pushed to any ref:
    /// not in a commit nobody moves to a branch or pushes, nor in what is not committed
    /// (US-GRD-008).
    PolicyReach,
    /// The rules of the team are read from the copy of the main branch the repo holds of the
    /// remote, which an agent can rewrite by hand; the high-water mark that closes it waits for
    /// the confirmation command (US-GRD-008).
    PolicyFloor,
    /// The permissions per operation are not applied with Git directly to `reset --hard`, to
    /// removing a worktree, to creating one without a new branch nor to a push of tags only;
    /// `merge` only stops at the branch (US-GRD-007). Only with `guard.permissions`.
    PermissionReach,
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

/// Level of the `core.hooksPath` a repo had before the install (ADR-GRD-001 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HooksPathLevel {
    /// No `core.hooksPath`: Git ran the hooks of `<common>/hooks`.
    None,
    Local,
    Global,
    System,
}

/// The hooks a repo already had, which the install keeps and chains (US-GRD-002, BR-AUTH-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorHooks {
    /// The prior `core.hooksPath`, as Git had it (`None` without one).
    pub hooks_path: Option<Untrusted>,
    pub level: HooksPathLevel,
    /// Where Git ran them from (relative to each worktree when the value is relative).
    pub dir: Untrusted,
    /// Names of the executable hooks found there (githooks(5) names only), sorted.
    pub hooks: Vec<Untrusted>,
}

/// A reserved action that relaxes, announced and waiting for its window to close (ADR-GRD-007
/// § 1, D5). Only the requester can apply it; anyone can cancel it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingAction {
    /// Opaque, random: names this announcement only.
    pub action_id: String,
    pub action: PendingKind,
    /// When the window closes (Unix ms): the requester may apply it from then on.
    pub applies_at_ms: i64,
    /// Length of the window.
    pub window_ms: u64,
}

/// What a [`PendingAction`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PendingKind {
    /// Remove the hook layer (US-GRD-003).
    Uninstall,
    /// Confirm the team baseline of the main branch (US-GRD-014). Only with
    /// `guard.team-confirmation`: an older client could not decode it.
    ConfirmTeam,
}

/// `guard.uninstall` parameters. Without `confirm`, announces the uninstall and opens the
/// window; with the `action_id` of that announcement, once the window closed, applies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardUninstallParams {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
}

/// `guard.uninstall` result: the window that opened, or the status once applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardUninstallResult {
    /// Set while the window is open (the first call); `None` once applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingAction>,
    pub status: GuardStatus,
    /// Removing a folder this profile does not know (an orphan): the local `core.hooksPath` its
    /// manifest says the repo had, which the uninstall writes back. Shown before the window, as
    /// read from the repo. Only with `guard.recovery`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restores_hooks_path: Option<Untrusted>,
}

/// Why `guard.uninstall`, `guard.cancel` or the window of `guard.confirm` did nothing (`data`
/// of `GUARD_UNINSTALL_REFUSED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum UninstallRefusal {
    /// The repo has no confirmed install of this profile.
    NotInstalled,
    /// Another reserved action is already waiting in this repo.
    AlreadyPending,
    /// The window has not closed yet.
    WindowOpen,
    /// No such announcement: cancelled, expired, already applied or never made.
    NoPending,
    /// The announcement is someone else's: only its requester applies it.
    NotTheRequester,
}

/// `data` of a `GUARD_UNINSTALL_REFUSED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardUninstallRefusedData {
    pub reason: UninstallRefusal,
    /// With `window-open`: how long until it closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_ms: Option<u64>,
}

/// What waits for the developer's confirmation (`GuardStatus.confirm`): the client puts the
/// text of the command. Absent with nothing to confirm and with an invalid team base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmHint {
    pub action: ConfirmAction,
}

/// The action that confirms (a closed code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmAction {
    /// `guard.confirm` (`raptor guard confirm`).
    GuardConfirm,
}

/// `guard.confirm` parameters. With `path` alone, the plan (nothing is written or announced);
/// with `accept`, the fingerprint of the plan the developer saw; with `confirm`, the
/// `action_id` of the window once it closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardConfirmParams {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
}

/// `guard.confirm` result: an answer, never a JSON-RPC error, except `GUARD_UNINSTALL_REFUSED`
/// for the window (already pending, still open, gone, someone else's) and `RESERVED_REFUSED` for
/// an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "result")]
pub enum GuardConfirmResult {
    /// The plan and its fingerprint: nothing was written or announced.
    Plan {
        plan: ConfirmPlanView,
        fingerprint: String,
    },
    /// Confirmed.
    Applied { plan: ConfirmPlanView },
    /// It relaxes: the window opened (announced in `GuardStatus.pending`).
    Pending {
        plan: ConfirmPlanView,
        action: PendingAction,
    },
    /// What the main branch resolves already is the confirmed baseline, or only hardens it.
    NothingToConfirm,
    /// The team's `engine.baseBranch` is not a valid branch name: the team has to fix it.
    BaseInvalid,
    /// The team settings are ignored or partial.
    FloorUnreadable,
    /// The team settings relax something the plan cannot show.
    UnshowableRelaxation,
    /// The team baseline or the confirmation changed after the plan was shown: nothing written.
    ChangedDuringWindow,
    /// The repo is not observed by this profile.
    NotObserved,
}

/// The plan of a confirmation, as shown to the developer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmPlanView {
    /// The copy of the main branch the team settings were read from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ConfirmSourceView>,
    pub base: BaseChangeView,
    /// Each relaxation, rule by rule.
    pub relaxations: Vec<RelaxationView>,
    /// The confirmation waits for the cancellable window: the base changes or something
    /// relaxes.
    pub relaxes: bool,
}

/// Where the team settings of a plan come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmSourceView {
    pub reference: UntrustedName,
    pub commit: String,
    /// The blob of `.gitraptor/settings.json`; `None` with no team settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_blob: Option<String>,
}

/// The base branch before (confirmed, or `main` with nothing confirmed) and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BaseChangeView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<UntrustedName>,
    pub to: UntrustedName,
    /// Branches protected today that stop being protected once confirmed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unguarded: Vec<UntrustedName>,
}

/// One key that relaxes, with its value before and after (the repo's text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelaxationView {
    pub key: RelaxedKey,
    pub before: Vec<Untrusted>,
    pub after: Vec<Untrusted>,
}

/// The closed list of keys a plan shows as relaxations (the key path in the settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum RelaxedKey {
    #[serde(rename = "permissions.allow")]
    PermissionsAllow,
    #[serde(rename = "permissions.ask")]
    PermissionsAsk,
    #[serde(rename = "permissions.deny")]
    PermissionsDeny,
    #[serde(rename = "permissions.disableSafeMinimum")]
    DisableSafeMinimum,
    #[serde(rename = "policies.protectedBranches")]
    ProtectedBranches,
    #[serde(rename = "policies.forbiddenPaths")]
    ForbiddenPaths,
    #[serde(rename = "policies.commitAuthorship")]
    CommitAuthorship,
}

impl RelaxedKey {
    /// The key path, as the settings document writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PermissionsAllow => "permissions.allow",
            Self::PermissionsAsk => "permissions.ask",
            Self::PermissionsDeny => "permissions.deny",
            Self::DisableSafeMinimum => "permissions.disableSafeMinimum",
            Self::ProtectedBranches => "policies.protectedBranches",
            Self::ForbiddenPaths => "policies.forbiddenPaths",
            Self::CommitAuthorship => "policies.commitAuthorship",
        }
    }
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
    /// The hooks the repo already has, kept and chained (US-GRD-002). Only with
    /// `guard.prior-hooks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<PriorHooks>,
    /// Present when installing again would repair a protection that stopped being active
    /// (US-GRD-004): what changes, shown before the developer grants it. Only with
    /// `guard.protection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<RepairPlan>,
}

/// What a repair rewrites (US-GRD-004, D9): the files of Guardrails' folder and, when another
/// tool changed `core.hooksPath`, the value that becomes the chained prior hooks folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairPlan {
    pub cause: LossCause,
    /// Files of `<common>/gitraptor/` that are written again, relative to it.
    pub files: Vec<Untrusted>,
    /// The current `core.hooksPath` that the dispatchers chain from now on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chains: Option<Untrusted>,
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
    /// Committed `.gitraptor/*.json` files other than `settings.json`, on the floor or the
    /// `HEAD`: never read, likely a misnamed settings file (ADR-GRP-007). Informational.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub misnamed_settings: Vec<Untrusted>,
    /// A reserved action announced and waiting for its window (US-GRD-003, D5). Only with
    /// `guard.pending-action`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingAction>,
    /// Whether the hook layer is still active and, if not, why (ADR-GRD-005 § 3, US-GRD-004).
    /// Only with `guard.protection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksLayer>,
    /// What is worth the developer's attention without changing the state. Only with
    /// `guard.protection`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// The safe minimum (BR-EDGE-001). Only with `guard.protection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_set: Option<MinimumSet>,
    /// Something waits for the developer's confirmation (US-GRD-014). Only with
    /// `guard.team-confirmation`, never for `raptor-mcp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<ConfirmHint>,
}

/// State of the hook layer of a repo (ADR-GRD-005 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HooksStatus {
    /// Installed and in place: every condition of ADR-GRD-005 § 1 holds.
    Active,
    /// Installed by Guardrails and no longer working; `cause` says why.
    Inactive,
    /// Never installed, or removed from Guardrails.
    NotInstalled,
    /// The profile was deleted but the repo still carries the key and the manifest.
    Orphaned,
}

/// Why the hook layer stopped being active (ADR-GRD-005 § 1, H2 to H4). English codes of the
/// ADR's names; the client puts the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LossCause {
    /// `core.hooksPath` is not Guardrails' folder any more (`hookspath-cambiado`).
    HookspathChanged,
    /// The repo moved: the key still names the old folder (`repo-movido`).
    RepoMoved,
    /// `<common>/gitraptor/hooks` is gone (`carpeta-ausente`).
    FolderMissing,
    /// A dispatcher is gone.
    DispatcherMissing,
    /// A dispatcher differs from the integrity reference of the journal (`dispatcher-alterado`).
    DispatcherAltered,
    /// A dispatcher lost its execute permission: Git skips it.
    DispatcherNotExecutable,
    /// The installed `raptor` the dispatchers start is gone (`binario-ausente`).
    BinaryMissing,
}

/// `hooks` of the status: the state, the cause when it is not active, and the worktree when the
/// cause is its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HooksLayer {
    pub status: HooksStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<LossCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Untrusted>,
}

impl HooksLayer {
    pub fn of(status: HooksStatus) -> Self {
        Self {
            status,
            cause: None,
            worktree: None,
        }
    }

    pub fn lost(cause: LossCause) -> Self {
        Self {
            status: HooksStatus::Inactive,
            cause: Some(cause),
            worktree: None,
        }
    }
}

/// A diagnostic that does not change the state (ADR-GRD-005 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Diagnostic {
    /// The dispatchers come from an older template: still active, refreshed by installing again.
    TemplateOutdated,
    /// The base branch was not confirmed (a team configuration without a confirmation).
    BaseUnconfirmed,
    /// Git's configuration could not be read: the check could not tell, and says so instead of
    /// reporting a loss.
    ConfigUnreadable,
    /// No active protection, but a `gitraptor/` folder is left in the repo (an uninstall whose
    /// cleanup stopped after the key went back, or a folder this profile does not know).
    /// `raptor guard uninstall` removes what GitRaptor wrote in it. Only with `guard.recovery`.
    LeftoverFolder,
    /// A base branch different from the confirmed one reached the main branch: both are
    /// protected until the developer confirms it. Only with `guard.team-confirmation`.
    BaseChangePending,
    /// The main branch declares an invalid base branch over a confirmed one: the team has to fix
    /// it. Only with `guard.team-confirmation`.
    BaseChangePendingInvalid,
    /// The team settings relax the confirmed ones: they only harden until the developer
    /// confirms them. Only with `guard.team-confirmation`.
    FloorRelaxPending,
}

/// The step of an install, upgrade, repair or uninstall that failed: a closed list, the only
/// thing the daemon's log and the client learn about the failure (its detail carries paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GuardStep {
    /// The native dispatcher next to `raptor` could not be read.
    Stub,
    /// The journal in the profile could not be read or written.
    Journal,
    /// The worktrees of the repo could not be listed.
    Worktrees,
    /// `gitraptor/` could not be written or removed.
    Folder,
    /// Windows: the permissions of `gitraptor/` are not private.
    FolderAcl,
    /// `gitraptor/` is not a folder, or not the one the journal recorded and its files do not
    /// prove it is.
    FolderIdentity,
    /// What an interrupted write left in `gitraptor/` could not be removed.
    Temporaries,
    /// The dispatchers or the constants could not be replaced (upgrade or repair).
    Dispatchers,
    /// The dispatchers an older install had and the new one does not could not be removed.
    Leftovers,
    /// `core.hooksPath` could not be written or restored.
    Key,
    /// The repo's `config` is not a regular file.
    Config,
    /// A worktree does not see the key after it was written.
    Verify,
    /// The install could not be confirmed in the profile.
    Confirm,
    /// The profile could not record that the uninstall finished.
    Clear,
}

impl GuardStep {
    pub const ALL: [Self; 14] = [
        Self::Stub,
        Self::Journal,
        Self::Worktrees,
        Self::Folder,
        Self::FolderAcl,
        Self::FolderIdentity,
        Self::Temporaries,
        Self::Dispatchers,
        Self::Leftovers,
        Self::Key,
        Self::Config,
        Self::Verify,
        Self::Confirm,
        Self::Clear,
    ];

    /// The kebab-case name, the one the log writes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stub => "stub",
            Self::Journal => "journal",
            Self::Worktrees => "worktrees",
            Self::Folder => "folder",
            Self::FolderAcl => "folder-acl",
            Self::FolderIdentity => "folder-identity",
            Self::Temporaries => "temporaries",
            Self::Dispatchers => "dispatchers",
            Self::Leftovers => "leftovers",
            Self::Key => "key",
            Self::Config => "config",
            Self::Verify => "verify",
            Self::Confirm => "confirm",
            Self::Clear => "clear",
        }
    }
}

/// Whether the hooks still run after a failure, read again from the repo's `core.hooksPath`
/// once the failure happened (never deduced from the step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectionAfter {
    /// The key is Guardrails' own: the hooks run as before the command.
    InPlace,
    /// The key is not Guardrails': no hook of GitRaptor runs in the repo.
    Absent,
}

/// `data` of a `guard-failed` error: which step failed and where that leaves the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardFailedData {
    pub step: GuardStep,
    pub protection: ProtectionAfter,
}

/// Whether the safe minimum applies (ADR-GRD-005 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MinimumSetStatus {
    Active,
    /// Only with TS-GRD-001 and the developer's confirmation (Q-GRD-21).
    DisabledByTeam,
}

/// `minimum_set` of the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MinimumSet {
    pub status: MinimumSetStatus,
}

/// Data of the `guard.protection-lost` event: the layer of a repo stopped being active without
/// Guardrails having done it (ADR-GRD-005 § 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtectionLostData {
    pub repo_id: String,
    pub hooks: HooksLayer,
}

/// `data` of a `GUARD_REJECTED` error: the install did not happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardRejectedData {
    pub blockers: Vec<InstallBlocker>,
}

/// Kind of a decision log entry (ADR-GRD-006 § 1; `notice`: Enmienda 2026-10-07). Only
/// `denial` counts as a blocked action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LogKind {
    /// The applied effect was not `allow`.
    Denial,
    /// An agent's commit went in with a warning, or under `flexible` (BR-AUTH-005).
    Notice,
    /// The hook layer changed state (ADR-GRD-005 § 5). Only with `guard.protection`.
    ProtectionState,
}

/// Whether an entry carries every field or only the counters over the insert cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LogDetail {
    Full,
    /// Occurrences over the cap, aggregated by kind, operation and rule (ADR-GRD-006 § 2).
    RateLimited,
}

/// Layer that took the decision. Entries other than `hooks` are listed only to a connection
/// with `guard.log-layers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LogLayer {
    Hooks,
    Mcp,
    Cockpit,
}

/// Who wrote the entry. Until the degraded-mode spool, only the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LogOrigin {
    Daemon,
}

/// What an operation did to one ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RefChange {
    Create,
    Update,
    /// Not a fast-forward.
    Force,
    Delete,
}

/// One ref of a logged operation, by its short name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoggedRef {
    pub name: Untrusted,
    pub change: RefChange,
}

/// The normalized operation of an entry: never argv, oids or a message (M-06).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum LoggedOperation {
    Push {
        /// Remote name, or the URL without userinfo, query or fragment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<Untrusted>,
        refs: Vec<LoggedRef>,
    },
    RefTransaction {
        refs: Vec<LoggedRef>,
    },
    Rebase {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upstream: Option<Untrusted>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<Untrusted>,
    },
    Commit {
        stage: CommitStage,
    },
    /// The hook layer went from one state to another (US-GRD-004). `expected` is true when
    /// Guardrails did it (install, uninstall): no alert. Only with `guard.protection`.
    ProtectionState {
        from: HooksStatus,
        to: HooksStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause: Option<LossCause>,
        expected: bool,
    },
}

/// A reason as logged: the rule, its level and its cause, without parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoggedReason {
    pub rule: Rule,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
}

/// What a commit entry says about authorship (DS-US-GRD-018 D12): agent kinds, never names or
/// emails. The author and the committer are shown by joining the event, not copied here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LoggedAuthorship {
    /// One entry per `Co-Authored-By`; `None` = not recognised by the table.
    pub coauthors: Vec<Option<AgentKind>>,
    /// At least one trailer identifies an agent.
    pub agent_trailer: bool,
    /// The message could not be read.
    pub unreadable: bool,
    /// The policy in force when it was applied (`agents-commit`, `human-author`, `flexible`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
}

/// One entry of the decision log (ADR-GRD-006 § 1). The repo is the one asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuardLogEntry {
    /// First occurrence (UTC) and the local offset then.
    pub at_ms: i64,
    pub utc_offset_s: i32,
    /// Last occurrence aggregated here.
    pub last_ms: i64,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Untrusted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<Untrusted>,
    /// The agent behind the operation; `None` = unattributed.
    pub actor: Option<AgentKind>,
    pub operation: LoggedOperation,
    pub kind: LogKind,
    pub detail: LogDetail,
    pub effect: Effect,
    pub applied_effect: Effect,
    pub reasons: Vec<LoggedReason>,
    pub layer: LogLayer,
    pub origin: LogOrigin,
    pub decision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorship: Option<LoggedAuthorship>,
}

/// `guard.log` parameters (US-GRD-005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuardLogParams {
    pub path: String,
    /// Start of the period; never earlier than the retention (90 days).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<i64>,
    /// Entries returned, most recent first (default 50, at most 500).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Occurrences of the period, summing `count` (ADR-GRD-006 § 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuardLogSummary {
    /// The KPI: denials, rows over the cap included.
    pub blocked: u64,
    /// Warnings and `flexible` commits: outside the KPI.
    pub notices: u64,
    /// Of the above, how many are counted only in the rows over the cap.
    pub rate_limited: u64,
}

/// A period the engine was not running: the hooks decided alone and nothing was logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UnloggedPeriod {
    pub from_ms: i64,
    /// `None`: still open.
    pub to_ms: Option<i64>,
}

/// `guard.log` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuardLogResult {
    /// The start of the period actually read.
    pub since_ms: i64,
    pub summary: GuardLogSummary,
    pub entries: Vec<GuardLogEntry>,
    pub unlogged_periods: Vec<UnloggedPeriod>,
}

/// Longest page of `guard.log`.
pub const MAX_LOG_PAGE: u32 = 500;
/// Days an entry is kept, from its last occurrence (BR-TIME-002).
pub const LOG_RETENTION_DAYS: i64 = 90;

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
    fn permission_codes_round_trip_in_kebab_case() {
        let rules = [
            (Rule::PermissionCommit, "permission.commit"),
            (Rule::PermissionPush, "permission.push"),
            (Rule::PermissionForcePush, "permission.force-push"),
            (Rule::PermissionResetHard, "permission.reset-hard"),
            (Rule::PermissionBranchDelete, "permission.branch-delete"),
            (Rule::PermissionRebase, "permission.rebase"),
            (Rule::PermissionMerge, "permission.merge"),
            (Rule::PermissionWorktreeAdd, "permission.worktree-add"),
            (Rule::PermissionWorktreeRemove, "permission.worktree-remove"),
        ];
        for (rule, code) in rules {
            let json = serde_json::to_string(&rule).unwrap();
            assert_eq!(json, format!("\"{code}\""));
            assert_eq!(serde_json::from_str::<Rule>(&json).unwrap(), rule);
        }
        let json = serde_json::to_string(&Cause::AskUnavailable).unwrap();
        assert_eq!(json, "\"ask-unavailable\"");
        assert_eq!(
            serde_json::from_str::<Cause>(&json).unwrap(),
            Cause::AskUnavailable
        );
        let json = serde_json::to_string(&NotPreventable::PermissionReach).unwrap();
        assert_eq!(json, "\"permission-reach\"");
        assert_eq!(
            serde_json::from_str::<NotPreventable>(&json).unwrap(),
            NotPreventable::PermissionReach
        );
    }

    #[test]
    fn effects_are_ordered_by_restriction() {
        assert!(Effect::Deny > Effect::Ask && Effect::Ask > Effect::Allow);
    }

    #[test]
    fn hooks_round_trip_their_git_names() {
        for h in Hook::ALL {
            assert_eq!(Hook::from_git_name(h.git_name()), Some(h));
        }
        assert_eq!(Hook::from_git_name("post-commit"), None);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<GuardRepoParams>(r#"{"path":"/r","x":1}"#).is_err());
    }
}
