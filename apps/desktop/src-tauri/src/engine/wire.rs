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
    RepoClosed {
        repo_id: String,
    },
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
    pub a: String,
    pub b: String,
    pub state: PairState,
    pub level: Option<PairLevel>,
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

/// The command that stops the daemon at `path`, safe to paste in a POSIX shell: every argument in
/// single quotes (a quote written `'\''`). `None` when `path` has a character that
/// [`gitraptor_api::display::must_escape`] flags: it could act on the terminal it is pasted in.
pub fn stop_command(path: &str) -> Option<String> {
    if path.chars().any(gitraptor_api::display::must_escape) {
        return None;
    }
    let quote = |arg: &str| format!("'{}'", arg.replace('\'', "'\\''"));
    Some(format!(
        "{} {} {}",
        quote(path),
        quote("daemon"),
        quote("stop")
    ))
}
