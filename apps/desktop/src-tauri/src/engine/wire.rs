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

/// The command that stops the daemon at `path`, safe to paste in a POSIX shell.
// STUB: the real quoting and the `must_escape` refusal land with the supervisor.
pub fn stop_command(_path: &str) -> Option<String> {
    None
}
