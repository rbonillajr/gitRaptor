//! Parameters and results of each method, and the payload of the engine's
//! own events. All of them reject unknown fields (SEC-02).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Untrusted;
use crate::event::Event;

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
    pub name: Option<Untrusted>,
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
    Branch { name: Untrusted },
    /// A branch without commits yet.
    Unborn { name: Untrusted },
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
    pub admin_name: Option<Untrusted>,
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
    /// Stop cause as stored in the profile (`stop-command`, `replace`,
    /// `signal`).
    pub cause: String,
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
