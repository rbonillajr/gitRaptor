//! The messages the backend pushes to the webview over the `Channel`: the app's own DTOs, never
//! the engine's contract types. Every text copied from the file system or from an agent is
//! sanitized before it gets here.

use serde::Serialize;

/// One message of the engine stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EngineMessage {
    Conn {
        state: ConnState,
        stale_since_ms: Option<u64>,
        resync: Option<ResyncReason>,
    },
    Session {
        daemon_version: String,
        protocol: u32,
        restricted: bool,
        activity: bool,
        discovery: bool,
    },
    Global(GlobalDto),
    EngineState {
        engine: EngineDto,
    },
    RepoObserved {
        repo: RepoSummaryDto,
    },
    RepoRetired {
        repo_id: String,
    },
    RepoTier {
        repo_id: String,
        tier: Tier,
    },
    RepoAttention {
        repo_id: String,
        attention: AttentionDto,
    },
    Discovered {
        candidates: Vec<CandidateDto>,
        replace: bool,
    },
    EngineOnly {
        engine: EngineDto,
    },
    Repo {
        fleet: RepoFleetDto,
    },
    Worktrees {
        repo_id: String,
        worktrees: Vec<WorktreeDto>,
        fetched_utc_ms: Option<u64>,
        perf: FleetPerf,
    },
    Sessions {
        repo_id: String,
        detection_available: bool,
        sessions: Vec<SessionDto>,
    },
    RepoSession {
        repo_id: String,
        session: SessionDto,
        perf: FleetPerf,
    },
    Prediction {
        repo_id: String,
        pairs: Vec<PairDto>,
        removed: Vec<String>,
        perf: FleetPerf,
    },
    PairDetails {
        repo_id: String,
        details: Vec<PairDetailDto>,
        filled: bool,
        perf: FleetPerf,
    },
    RepoClosed {
        repo_id: String,
    },
    GuardBlocks {
        blocks: GuardBlocksDto,
    },
    GuardBlocksUnavailable {
        repo_id: String,
        cause: GuardUnavailableCause,
        protection: Option<ProtectionDto>,
    },
}

/// Why the guard log could not be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuardUnavailableCause {
    LogUnavailable,
    NotObserved,
    Refused,
    NotOpen,
}

/// The open repo's blocks, sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardBlocksDto {
    pub repo_id: String,
    pub protection: ProtectionDto,
    pub since_ms: i64,
    pub summary: GuardSummaryDto,
    pub entries: Vec<BlockEntryDto>,
    pub pending: Option<PendingExceptionDto>,
    pub unlogged: Vec<UnloggedDto>,
}

/// Counters of the log window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardSummaryDto {
    pub blocked: u64,
    pub exceptions: u64,
    pub exceptions_rejected: u64,
}

/// A period the log did not record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnloggedDto {
    pub from_ms: i64,
    pub to_ms: Option<i64>,
}

/// How protected the repo is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProtectionDto {
    pub state: ProtectionKind,
}

/// The protection levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectionKind {
    Unprotected,
    HooksOnly,
    McpOnly,
    Full,
}

/// One row of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockEntryDto {
    pub decision_id: String,
    pub kind: BlockKind,
    pub at_ms: i64,
    pub last_ms: i64,
    pub count: u64,
    pub actor: Option<BlockActorDto>,
    pub worktree: Option<BlockWorktreeDto>,
    pub branch: Option<String>,
    pub operation: OperationDto,
    pub rules: Vec<BlockRuleDto>,
    pub layer: BlockLayer,
    pub detail: BlockDetail,
    pub exception: Option<ExceptionCommandDto>,
}

/// What the entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    Denial,
    Exception,
    ExceptionCancelled,
    ExceptionRejected,
}

/// Who attempted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BlockActorDto {
    ClaudeCode,
    Other,
}

/// The worktree of an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockWorktreeDto {
    pub folder: String,
    pub path: String,
}

/// A rule that stopped the operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockRuleDto {
    pub rule: String,
    pub level: String,
    pub cause: Option<String>,
}

/// The layer that caught it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockLayer {
    Hooks,
    Mcp,
    Cockpit,
}

/// How much the entry kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockDetail {
    Full,
    RateLimited,
}

/// The attempted operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OperationDto {
    Push {
        remote: Option<String>,
        refs: Vec<RefDto>,
    },
    RefTransaction {
        refs: Vec<RefDto>,
    },
    Rebase,
    Commit,
}

/// A ref an operation touches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefDto {
    pub name: String,
    pub change: RefChange,
}

/// What the operation does to a ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefChange {
    Create,
    Update,
    Force,
    Delete,
}

/// The copyable exception command, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ExceptionCommandDto {
    Offered { command: String },
    NotOffered { reason: NotOffered },
}

pub use super::exception_command::NotOffered;

/// An announced exception that has not applied yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingExceptionDto {
    pub applies_at_ms: i64,
    pub window_ms: u64,
    pub operation: OperationDto,
    pub rules: Vec<String>,
    pub cancel_command: Option<String>,
}

/// Time the backend spent turning an engine event into a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetPerf {
    pub backend_ns: u64,
}

/// The open repo: its worktrees and predicted pairs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFleetDto {
    pub repo_id: String,
    pub name: String,
    pub base: BaseDto,
    pub fetched_utc_ms: Option<u64>,
    pub tier: Option<Tier>,
    pub worktrees: Vec<WorktreeDto>,
    pub pairs: Vec<PairDto>,
    pub pairs_truncated: bool,
    pub limits: Vec<LimitCode>,
    pub confirm_command: ConfirmCommandDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaseDto {
    pub name: Option<String>,
    pub status: BaseStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BaseStatus {
    Confirmed,
    Unconfirmed,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDto {
    pub key: String,
    pub folder: String,
    pub path: String,
    pub main: bool,
    pub status: WorktreeStatusDto,
    pub last_activity_utc_ms: Option<u64>,
    pub last_activity_in_gap: bool,
    pub detached_at: Option<String>,
    pub operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WorktreeStatusDto {
    Ready(ReadyDto),
    Unavailable { reason: UnavailableReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    Missing,
    Untrusted,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyDto {
    pub head: HeadDto,
    pub counts: CountsDto,
    pub divergence: DivergenceDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HeadDto {
    Branch { name: String },
    Unborn { name: String },
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CountsDto {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
}

/// A commit count and whether it is exact or a lower bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoundedCountDto {
    pub count: u32,
    pub exact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DivergenceDto {
    Counted {
        ahead: BoundedCountDto,
        behind: BoundedCountDto,
    },
    BaseMissing,
    NoBase,
    NoCommits,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub session_id: String,
    pub worktree_key: String,
    pub agent: AgentDto,
    pub origin: SessionOrigin,
    pub state: SessionState,
    pub started_utc_ms: u64,
    pub state_since_utc_ms: u64,
    pub ended_utc_ms: Option<u64>,
    pub end_cause: Option<EndCause>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AgentDto {
    ClaudeCode,
    Other { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionOrigin {
    Detected,
    Registered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    Active,
    Inactive,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndCause {
    ProcessGone,
    EndedDuringGap,
    RegistrationWithdrawn,
    WorktreeRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairDto {
    pub pair_id: String,
    pub rev: u64,
    pub a: String,
    pub b: String,
    pub state: PairState,
    pub reason: Option<PairReason>,
    pub level: Option<PairLevel>,
    pub computed_at_utc_ms: Option<u64>,
    pub overlap_files: u32,
    pub overlap_partial: bool,
    pub conflict_files: u32,
    pub conflict_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairReason {
    BaseUnconfirmed,
    OperationInProgress,
    PowerSaving,
    Dormant,
    BaseMissing,
    MissingObject,
    Shallow,
    LimitExceeded,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitCode {
    OverlapIncludesUncommitted,
    ConflictCommittedOnly,
    NoMergeDriversOrAttributes,
    RenameLimit,
    InsertionsMayMerge,
    JoinedHunks,
    PairIgnoresBaseMoves,
    LocalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    Content,
    AddAdd,
    ModifyDelete,
    Rename,
    DirectoryFile,
    Binary,
    Submodule,
}

/// Lines `start..start + len` of one side, 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LineRangeDto {
    pub start: u32,
    pub len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFileDto {
    pub path: String,
    pub kind: ConflictKind,
    pub ours: Vec<LineRangeDto>,
    pub theirs: Vec<LineRangeDto>,
    pub hunks_truncated: bool,
    pub has_merge_rule: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictFilesDto {
    pub files: Vec<ConflictFileDto>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairDetailDto {
    pub pair: PairDto,
    pub overlap: Vec<String>,
    pub conflict: Option<ConflictFilesDto>,
}

/// The `raptor` command that confirms the base, when the app can name one safely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ConfirmCommandDto {
    Offered { command: String, with_path: bool },
    NotOffered { reason: NotOfferedReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotOfferedReason {
    RaptorUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairState {
    Calculating,
    Current,
    Recalculating,
    Pending,
    NotComputable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairLevel {
    None,
    Overlap,
    Conflict,
}

/// State of the connection to the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ConnState {
    Connecting,
    Starting,
    Syncing,
    Live,
    Reconnecting {
        attempt: u32,
        retry_in_ms: u64,
    },
    Stopped {
        reason: StoppedReason,
    },
    Incompatible {
        which: IncompatibleWhich,
        daemon_version: Option<String>,
        daemon_path: Option<String>,
        stop_command: Option<String>,
        action: IncompatibleAction,
    },
    Rejected,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoppedReason {
    NotRunning,
    LaunchFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncompatibleWhich {
    DaemonOlder,
    AppOlder,
    MissingCapabilities,
}

/// What the user can do about an incompatible engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncompatibleAction {
    Start,
    CopyStop,
    UpdateApp,
    None,
}

/// Why the view is being rebuilt from a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResyncReason {
    SlowConsumer,
    ReplayUnavailable,
    DaemonRestarted,
    ScopeClosed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GlobalDto {
    pub engine: EngineDto,
    pub daemon: DaemonDto,
    pub autostart: Autostart,
    pub repos: Vec<RepoSummaryDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DaemonDto {
    pub version: String,
    pub protocol: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDto {
    pub state: EngineStateDto,
    pub git_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineStateDto {
    WaitingForGit,
    NoRepos,
    Observing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSummaryDto {
    pub repo_id: String,
    pub name: String,
    pub path: String,
    pub state: RepoStateDto,
    pub tier: Option<Tier>,
    pub attention: AttentionDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepoStateDto {
    Observed,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttentionDto {
    pub conflicts: CountDto,
    pub denials: CountDto,
}

/// A count, or that there is none: never a zero that was not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CountDto {
    Counted { count: u32 },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateDto {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Active,
    Waking,
    Dormant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Autostart {
    Registered,
    NotRegistered,
    Unknown,
}

/// The command that stops the daemon at `path`, safe to paste in a POSIX shell (see
/// [`super::shell_command::posix`]); `None` when `path` could act on the terminal it is pasted in.
pub fn stop_command(path: &str) -> Option<String> {
    super::shell_command::posix(&[path, "daemon", "stop"])
}
