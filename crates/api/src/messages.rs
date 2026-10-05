//! Parameters and results of each method, and the payload of the engine's
//! own events. All of them reject unknown fields (SEC-02).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::{Untrusted, UntrustedName};

/// Longest accepted client version text.
pub const MAX_CLIENT_VERSION_LEN: usize = 32;

/// Who the client says it is. The daemon does not trust it to grant
/// anything: it only narrows the connection (a `raptor-mcp` peer is always
/// treated as MCP, whatever it declares).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Cli,
    Mcp,
    Other,
}

/// `hello` parameters. Frozen across protocol versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol: u32,
    pub client: ClientKind,
    pub client_version: String,
}

/// What a connection may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionProfile {
    /// CLI/TUI: every method, reserved ones subject to the daemon's checks.
    Full,
    /// `raptor-mcp`: the MCP allowlist, bounded projections (SEC-12, SEC-14).
    Mcp,
}

/// `hello` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HelloResult {
    pub protocol: u32,
    pub binary_version: String,
    /// Profile instance id (ADR-GRP-006 § 4), compared by the Guardrails
    /// hook client with its dispatcher constant (ADR-GRD-003 § 4).
    pub instance_id: String,
    pub daemon_pid: u32,
    pub profile: ConnectionProfile,
    pub max_message_bytes: u64,
    /// Methods this connection may call.
    pub methods: Vec<String>,
    /// Who the daemon sees on this connection and the layer it fixes
    /// (protocol 6, full profile, `cli` clients; ADR-CKP-003 § 4 N5). UX
    /// only: the daemon resolves the requester again on every request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<crate::scope::ConnectionRequester>,
}

/// `data` of an `INCOMPATIBLE_PROTOCOL` error. Frozen across versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IncompatibleData {
    pub daemon_protocol: u32,
    pub binary_version: String,
}

/// `daemon.replace` parameters. Frozen across versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceParams {
    /// Protocol of the replacing binary; must be newer than the daemon's.
    pub protocol: u32,
}

/// Parameters of methods that take none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// Result of `daemon.stop` and `daemon.replace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopResult {
    pub stopping: bool,
}

/// `repo.add` parameters (US-GRP-001). The path is validated before
/// anything touches the file system (SEC-02). It names the root of a
/// worktree or a Git directory: the daemon does not search upwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoAddParams {
    pub path: String,
}

/// `repo.retire` parameters. US-GRP-001 stops the observation; US-GRP-006
/// adds what a retired repo keeps and recovers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoRetireParams {
    pub repo_id: String,
}

/// Availability state of the engine (BR-WF-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EngineStateView {
    WaitingForGit,
    NoRepos,
    Observing,
}

/// The engine part of a snapshot, and the data of `engine.state` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineView {
    pub state: EngineStateView,
    /// Parsed version of the system Git, if found.
    pub git_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RepoStateView {
    Observed,
    /// In the profile but its store could not be opened.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoView {
    /// Opaque id of the repo in the profile.
    pub repo_id: String,
    pub state: RepoStateView,
    /// Canonical path of the Git common directory (text from the file
    /// system).
    pub path: Untrusted,
    /// The branch ahead/behind is counted against (US-GRP-012).
    pub base: BaseBranchView,
    /// Every worktree of the repo as of the last reconciliation
    /// (US-GRP-001): the main one first, then the linked ones by path.
    pub worktrees: Vec<WorktreeView>,
}

/// The base branch of a repo (US-GRP-012, ADR-GRD-004 § 3): the one the
/// developer confirmed or, until then, the resolved one marked unconfirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BaseBranchView {
    /// Short branch name; `None` only when the status is `invalid`.
    pub name: Option<UntrustedName>,
    pub status: BaseStatusView,
}

/// Whether the base branch is the confirmed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BaseStatusView {
    /// Confirmed by the developer: the only value of the repo.
    Confirmed,
    /// Nothing confirmed yet: the resolved branch (`base-unconfirmed`).
    Unconfirmed,
    /// Nothing confirmed and the team declares an invalid name: there is
    /// no base branch (Q42). Only the team settings of US-GRP-016 lead here.
    Invalid,
}

/// Most commits counted on each side of an ahead/behind.
pub const MAX_DIVERGENCE_WALK: u64 = 10_000;

/// Ahead/behind of a worktree against the base branch of its repo
/// (US-GRP-012), or why it is not counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DivergenceView {
    /// Commits only in the worktree's `HEAD` and only in the base branch.
    Counted {
        ahead: CommitCountView,
        behind: CommitCountView,
    },
    /// The base branch does not exist in the repo; no other branch is used
    /// instead (Q42).
    BaseMissing,
    /// The repo has no base branch (status `invalid`).
    NoBase,
    /// `HEAD` names a branch without commits.
    NoCommits,
    /// The commits could not be read now.
    Unreadable,
}

/// A bounded commit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitCountView {
    pub count: u64,
    /// `false`: the walk stopped at [`MAX_DIVERGENCE_WALK`]; there are at
    /// least `count`.
    pub exact: bool,
}

/// Most changed paths listed per worktree.
pub const MAX_WORKTREE_CHANGES: usize = 200;

/// Most bytes of listed paths per worktree. With the count bound, it keeps
/// a snapshot of many worktrees under the message limit; past the snapshot
/// budget the daemon drops the lists and keeps the counts.
pub const MAX_WORKTREE_CHANGE_BYTES: usize = 32 * 1024;

/// Where `HEAD` of a worktree points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HeadView {
    /// A branch with commits.
    Branch { name: UntrustedName },
    /// A branch without commits yet.
    Unborn { name: UntrustedName },
    /// Directly at a commit.
    Detached,
}

/// Why a worktree could not be read. What each one means for the developer
/// and the special states belong to US-GRP-003.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    /// Its folder is not there (BR-EDGE-001).
    Missing,
    /// Git's `safe.directory` rules do not trust it (SEC-11).
    Untrusted,
    /// It is there but could not be read now.
    Unreadable,
}

/// Whether a worktree could be read (US-GRP-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorktreeStatus {
    Ready {
        head: HeadView,
        counts: ChangeCounts,
        /// Changed paths, sorted, bounded by [`MAX_WORKTREE_CHANGES`] and
        /// [`MAX_WORKTREE_CHANGE_BYTES`]; `counts` says how many there are.
        changes: Vec<FileChangeView>,
        /// Against the base branch (US-GRP-012). `engine.snapshot` counts
        /// it again with the base branch as it is when asked.
        divergence: DivergenceView,
    },
    /// Registered in the repo but not readable now: nothing else is
    /// reported for it rather than made up.
    Unavailable { reason: UnavailableReason },
}

/// How many paths changed, by area. All zero: the worktree is clean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeCounts {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
}

impl ChangeCounts {
    pub fn total(&self) -> u64 {
        u64::from(self.staged) + u64::from(self.unstaged) + u64::from(self.untracked)
    }

    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }
}

/// One worktree of an observed repo (US-GRP-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorktreeView {
    /// Root of the working tree, canonical (text from the file system).
    pub path: Untrusted,
    /// The repo's main worktree (not a linked one).
    pub main: bool,
    /// Name of a linked worktree under `<common dir>/worktrees/`.
    pub admin_name: Option<UntrustedName>,
    pub status: WorktreeStatus,
}

impl WorktreeView {
    /// The listed changes are fewer than the counted ones.
    pub fn changes_truncated(&self) -> bool {
        match &self.status {
            WorktreeStatus::Ready {
                counts, changes, ..
            } => (changes.len() as u64) < counts.total(),
            WorktreeStatus::Unavailable { .. } => false,
        }
    }
}

/// Where a change sits.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeAreaView {
    /// Prepared for the next commit (`HEAD` versus the index).
    Staged,
    /// Not prepared (index versus the working tree).
    Unstaged,
    /// Not tracked and not ignored.
    Untracked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeKindView {
    Added,
    Deleted,
    Modified,
    TypeChanged,
    Conflicted,
}

/// One changed path, relative to the worktree root with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileChangeView {
    pub path: Untrusted,
    pub area: ChangeAreaView,
    pub kind: ChangeKindView,
}

/// Data of a `worktree.state` event: the reconciled worktrees of one repo,
/// all of them (US-GRP-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorktreeStateData {
    pub repo_id: String,
    pub worktrees: Vec<WorktreeView>,
}

/// What happened in a `git.event` (US-GRP-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GitEventKind {
    Commit,
    Merge,
    Rebase,
    /// A branch moved without a commit, merge or rebase (reset, a ref
    /// written by a tool, a repo without reflogs).
    BranchUpdate,
    BranchCreate,
    BranchDelete,
    /// `HEAD` of the worktree moved to another branch.
    BranchSwitch,
    WorktreeCreate,
    WorktreeDelete,
    Push,
    /// A reconciliation found differences no Git event explains; linked to
    /// a gap (ADR-GRP-013 § 5). Not a Git command.
    Reconciled,
}

impl GitEventKind {
    /// Stable text, also the `kind` stored in the repo's history.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::BranchUpdate => "branch-update",
            Self::BranchCreate => "branch-create",
            Self::BranchDelete => "branch-delete",
            Self::BranchSwitch => "branch-switch",
            Self::WorktreeCreate => "worktree-create",
            Self::WorktreeDelete => "worktree-delete",
            Self::Push => "push",
            Self::Reconciled => "reconciled",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    pub const ALL: [Self; 11] = [
        Self::Commit,
        Self::Merge,
        Self::Rebase,
        Self::BranchUpdate,
        Self::BranchCreate,
        Self::BranchDelete,
        Self::BranchSwitch,
        Self::WorktreeCreate,
        Self::WorktreeDelete,
        Self::Push,
        Self::Reconciled,
    ];
}

/// Metadata of a Git event: refs and commit ids, never content (NFR-03).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitEventDetails {
    /// The branch the event is about (`feat-login`), or the remote-tracking
    /// one of a push (`origin/feat-login`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<UntrustedName>,
    /// Branch before a switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<UntrustedName>,
    /// Commit before and after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_commit: Option<String>,
    /// Git does not say which worktree ran the command: the engine placed
    /// the event by a fallback rule (US-GRP-002, D6) and says so.
    #[serde(default)]
    pub worktree_inferred: bool,
}

/// Data of a `git.event` event and one entry of `events.history`
/// (US-GRP-002, ADR-GRP-013 § 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitEventView {
    pub repo_id: String,
    /// Sequence in the repo's history (ADR-GRP-013 § 4), not the stream's.
    pub seq: i64,
    /// Root of the worktree it happened in.
    pub worktree: Untrusted,
    pub kind: GitEventKind,
    /// Without evidence of a session, "unattributed" (BR-CONS-003).
    pub actor: crate::Actor,
    /// When the engine observed it, UTC milliseconds, and the local offset.
    pub observed_utc_ms: i64,
    pub utc_offset_s: i32,
    pub details: GitEventDetails,
    /// The gap a reconciliation event belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
}

/// Most entries one `events.history` page returns.
pub const MAX_HISTORY_PAGE: u32 = 200;

/// `events.history` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsHistoryParams {
    pub repo_id: String,
    /// Only the events of this worktree root.
    #[serde(default)]
    pub worktree: Option<String>,
    /// Events with a greater sequence, oldest first. Without it, the most
    /// recent `limit` events, oldest first.
    #[serde(default)]
    pub after_seq: Option<i64>,
    /// At most this many (capped at [`MAX_HISTORY_PAGE`]).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsHistoryResult {
    pub events: Vec<GitEventView>,
}

/// State of an agent session (BR-WF-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStateView {
    /// Present, with activity in its worktree within the threshold.
    Active,
    /// Present, without activity for the threshold.
    Inactive,
    /// No longer present. Never reopened (Q41).
    Ended,
}

impl SessionStateView {
    /// Stable text, also the suffix of the `session-*` kinds stored in the
    /// repo's history.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Ended => "ended",
        }
    }
}

/// Why a session ended (ADR-GRP-013 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SessionEndCauseView {
    /// Its process disappeared (normal or forced close).
    ProcessGone,
    /// Its process disappeared while the engine was not observing; the end
    /// time is unknown.
    EndedDuringGap,
    /// Its explicit registration was withdrawn (US-GRP-009).
    RegistrationWithdrawn,
}

/// One agent session, as `session.state` and `sessions.list` show it
/// (US-GRP-007, ADR-GRP-013 § 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionView {
    pub repo_id: String,
    pub session_id: String,
    /// Root of the worktree the session is associated with.
    pub worktree: Untrusted,
    /// Effective attribution of the session: always an agent with its
    /// origin.
    pub actor: crate::Actor,
    pub state: SessionStateView,
    pub started_utc_ms: i64,
    /// Since when it is in `state`.
    pub state_since_utc_ms: i64,
    /// Local offset of the engine's machine, to show the times.
    pub utc_offset_s: i32,
    /// Absent while present, and when it ended during a gap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_utc_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_cause: Option<SessionEndCauseView>,
}

/// `sessions.list` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsListParams {
    /// Only this repo; every observed repo without it.
    #[serde(default)]
    pub repo_id: Option<String>,
    /// Also the ended sessions. Without it, only the present ones and the
    /// latest ended session of each worktree (the "latest session" of the
    /// Cockpit amendment of ADR-GRP-013).
    #[serde(default)]
    pub include_ended: bool,
    /// At most this many, the most recent (capped at
    /// [`MAX_SESSIONS_PAGE`]).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Most sessions one `sessions.list` answer returns.
pub const MAX_SESSIONS_PAGE: u32 = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsListResult {
    /// Whether this system can detect sessions at all. `false` (Windows,
    /// for now) means "unknown", not "no sessions".
    pub detection_available: bool,
    /// Oldest first.
    pub sessions: Vec<SessionView>,
}

/// Data of a `repo.observation` event: a repo started or stopped being
/// observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoObservationData {
    pub repo_id: String,
    pub observed: bool,
    pub state: RepoStateView,
    /// Canonical path of the Git common directory.
    pub path: Untrusted,
}

/// What `repo.add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RepoAddOutcome {
    /// First time this repo is added.
    New,
    /// It was already observed; nothing changed.
    AlreadyObserved,
    /// A retired repo is observed again with its previous id and data.
    Reactivated,
}

/// `repo.add` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoAddResult {
    pub outcome: RepoAddOutcome,
    pub repo: RepoView,
}

/// `repo.retire` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoRetireResult {
    /// `false` if it was not observed (already retired).
    pub retired: bool,
}

/// Why `repo.add` rejected a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RepoRejection {
    /// The path is not a Git repository or worktree.
    NotARepo,
    /// Git's `safe.directory` ownership rules do not trust it (SEC-11).
    Untrusted,
    /// It looks like a repository but could not be read.
    Unreadable,
    /// No longer exists (`repo.retire` of an unknown id).
    UnknownRepo,
    /// A repo, but not one the engine observes (Guardrails, Q-GRD-15).
    NotObserved,
}

/// `data` of a `REPO_REJECTED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoRejectedData {
    pub reason: RepoRejection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonView {
    pub pid: u32,
    pub protocol: u32,
    pub binary_version: String,
    pub started_wall_ms: i64,
}

/// `engine.snapshot` result for a full connection. It reflects every event
/// up to `seq` included: subscribe with `from_seq = seq + 1` and the same
/// `run_id` to continue without gaps or duplicates (DEP-CKP-6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Identifies this run of the daemon: `seq` starts again at every start.
    pub run_id: String,
    pub seq: u64,
    pub engine: EngineView,
    pub daemon: DaemonView,
    pub repos: Vec<RepoView>,
}

/// `engine.snapshot` result for an MCP connection. This type is the field
/// allowlist of SEC-12: no paths, no free text, and only the caller's repo
/// (the observed repo that contains the caller's working folder). F-001-05
/// extends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpSnapshot {
    pub run_id: String,
    pub seq: u64,
    pub engine_state: EngineStateView,
    pub caller_repo: Option<McpRepoView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpRepoView {
    pub repo_id: String,
    pub state: RepoStateView,
}

/// `events.subscribe` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscribeParams {
    /// First sequence wanted. `None`: only events published from now on.
    /// Older than the replay buffer: an immediate `events.resync`.
    #[serde(default)]
    pub from_seq: Option<u64>,
    /// `run_id` of the snapshot `from_seq` continues. A different run means
    /// the daemon restarted: an immediate `events.resync`.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscribeResult {
    pub subscription: u32,
    /// Sequence of the first event this subscription will deliver.
    pub from_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnsubscribeParams {
    pub subscription: u32,
}

/// Params of an `events.event` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventNotification {
    pub subscription: u32,
    pub event: Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ResyncReason {
    /// The client did not read fast enough; it is disconnected.
    SlowConsumer,
    /// The requested `from_seq` is no longer in the replay buffer.
    ReplayUnavailable,
    /// The daemon restarted since the client's snapshot.
    DaemonRestarted,
    /// The repo of a scoped subscription stopped being observed; the
    /// subscription ended (protocol 6).
    ScopeClosed,
}

impl ResyncReason {
    pub const ALL: [Self; 4] = [
        Self::SlowConsumer,
        Self::ReplayUnavailable,
        Self::DaemonRestarted,
        Self::ScopeClosed,
    ];
}

/// Params of an `events.resync` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResyncNotification {
    pub reason: ResyncReason,
}

/// Data of a `daemon.stopping` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoppingData {
    pub cause: StopCauseCode,
}

/// Why the daemon stops (N7: a code, never presentation text). Same text
/// as the cause stored in the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StopCauseCode {
    /// `daemon.stop` from the developer.
    StopCommand,
    /// A newer installed binary replaces it.
    Replace,
    /// A signal from the OS or the service manager.
    Signal,
}

impl StopCauseCode {
    pub const ALL: [Self; 3] = [Self::StopCommand, Self::Replace, Self::Signal];
}

/// Outcome of a reserved command attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuditOutcome {
    Accepted,
    Rejected,
    /// Authorized, but its story has not implemented it yet.
    NotImplemented,
}

/// Why the daemon refused a reserved command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RefusalReason {
    /// The caller or one of its ancestors is an agent process.
    AgentAncestry,
    /// The caller's session leader descends from an agent.
    SessionLeaderAgent,
    /// The caller descends from the daemon itself (a hook or `git` run by
    /// its operation executor): a confused deputy (DEP-MCP-3).
    DaemonDescendant,
    /// The caller has no controlling terminal.
    NoControllingTerminal,
    /// The caller's identity could not be read or changed during the check.
    IdentityUnverified,
    /// Reserved commands are not offered to `raptor-mcp` (SEC-14).
    NotAvailableToMcp,
    /// The platform cannot run the checks yet.
    Unsupported,
}

/// The client as the daemon saw it (never as the client declared it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientIdentity {
    pub pid: u32,
    /// Process start time in microseconds since the epoch: with `pid`, the
    /// identifier that is never reused.
    pub start_us: u64,
    /// Executable path.
    pub exe: Option<Untrusted>,
    pub agent_ancestor: bool,
    #[serde(default)]
    pub daemon_descendant: bool,
    pub controlling_terminal: bool,
    /// The ancestry walk stopped at a process of another user or one the
    /// daemon cannot read (`login`, `launchd`).
    pub chain_truncated: bool,
}

/// One entry of the append-only audit of reserved commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    pub id: i64,
    pub at_ms: i64,
    pub operation: String,
    pub repo_id: Option<String>,
    pub outcome: AuditOutcome,
    pub reason: Option<RefusalReason>,
    pub client: ClientIdentity,
}

/// `data` of a `RESERVED_REFUSED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefusedData {
    pub reason: RefusalReason,
}

/// `audit.list` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditListParams {
    /// Only entries with a greater id.
    #[serde(default)]
    pub after_id: Option<i64>,
    /// At most this many entries (capped by the daemon).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditListResult {
    pub entries: Vec<AuditEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UntrustedName;

    /// SEC-12: the MCP projection carries exactly the allowlisted fields.
    #[test]
    fn mcp_snapshot_is_the_field_allowlist() {
        let snap = McpSnapshot {
            run_id: "r".into(),
            seq: 3,
            engine_state: EngineStateView::Observing,
            caller_repo: Some(McpRepoView {
                repo_id: "abc".into(),
                state: RepoStateView::Observed,
            }),
        };
        let value = serde_json::to_value(&snap).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["caller_repo", "engine_state", "run_id", "seq"]);
        let repo = value["caller_repo"].as_object().unwrap();
        let mut keys: Vec<_> = repo.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["repo_id", "state"]);
    }

    /// US-GRP-002: the wire form of a Git event; the actor without a session
    /// is "unattributed" and every kind round-trips through its text.
    #[test]
    fn git_event_wire_form() {
        let view = GitEventView {
            repo_id: "r".into(),
            seq: 7,
            worktree: Untrusted::new("/w/feat-login"),
            kind: GitEventKind::BranchSwitch,
            actor: crate::Actor::Unattributed,
            observed_utc_ms: 1,
            utc_offset_s: -18000,
            details: GitEventDetails {
                branch: Some(UntrustedName::new("feat-login")),
                from: Some(UntrustedName::new("main")),
                ..GitEventDetails::default()
            },
            gap_id: None,
        };
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(value["kind"], "branch-switch");
        assert_eq!(value["actor"], serde_json::json!({"actor": "unattributed"}));
        assert_eq!(value["details"]["worktree_inferred"], false);
        assert!(value.get("gap_id").is_none());
        let back: GitEventView = serde_json::from_value(value).unwrap();
        assert_eq!(back, view);
        for kind in GitEventKind::ALL {
            assert_eq!(GitEventKind::parse(kind.as_str()), Some(kind));
            let text = serde_json::to_value(kind).unwrap();
            assert_eq!(text, kind.as_str());
        }
        assert!(serde_json::from_str::<EventsHistoryParams>(r#"{"repo_id":"r","x":1}"#).is_err());
    }

    /// N7: the stop cause is a code, with the same text as before.
    #[test]
    fn stop_cause_is_a_code() {
        let data = StoppingData {
            cause: StopCauseCode::StopCommand,
        };
        assert_eq!(
            serde_json::to_value(&data).unwrap(),
            serde_json::json!({"cause": "stop-command"})
        );
        assert!(serde_json::from_str::<StoppingData>(r#"{"cause":"whatever"}"#).is_err());
        for code in StopCauseCode::ALL {
            let text = serde_json::to_value(code).unwrap();
            assert_eq!(serde_json::from_value::<StopCauseCode>(text).unwrap(), code);
        }
    }

    #[test]
    fn params_reject_unknown_fields() {
        assert!(serde_json::from_str::<SubscribeParams>(r#"{"from_seq":1,"x":2}"#).is_err());
        assert!(serde_json::from_str::<NoParams>(r#"{"confirmed":true}"#).is_err());
        assert!(
            serde_json::from_str::<Hello>(
                r#"{"protocol":1,"client":"cli","client_version":"1","tty":true}"#
            )
            .is_err()
        );
    }
}
