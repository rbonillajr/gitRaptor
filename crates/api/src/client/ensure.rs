//! The report of `raptor daemon ensure` (ADR-GRP-014 Enmienda DEP-DSK-5 § D4): one JSON line
//! the CLI writes and the desktop app reads, so both sides share one schema. Every key is always
//! present (`null` when it does not apply), and an unknown key is refused: a report of another
//! schema is never half-read.

use serde::{Deserialize, Serialize};

/// The schema version written in [`EnsureReport::schema`].
pub const SCHEMA: u32 = 1;

/// What `raptor daemon ensure` found or did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct EnsureReport {
    /// [`SCHEMA`].
    pub schema: u32,
    pub outcome: EnsureOutcome,
    /// The channel's runtime folder; `None` only when the profile could not be resolved.
    pub runtime: Option<String>,
    /// The pid of the socket's peer.
    pub daemon_pid: Option<u32>,
    /// The executable of the daemon as the kernel reports it, not canonicalized.
    pub daemon_path: Option<String>,
    /// The daemon's binary version (from the handshake or from its incompatibility).
    pub daemon_version: Option<String>,
    pub daemon_protocol: Option<u32>,
    /// `ensure` launched a daemon.
    pub started: bool,
    /// A daemon of the same path accepted to step down.
    pub replaced: bool,
    /// Only with [`EnsureOutcome::Incompatible`].
    pub incompatible: Option<IncompatibleReport>,
    /// Only with [`EnsureOutcome::Unavailable`].
    pub unavailable: Option<UnavailableReport>,
    pub autostart: AutostartReport,
}

/// The outcome of `ensure`, which also fixes its exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnsureOutcome {
    /// A greeted daemon serves every capability asked for.
    Ready,
    /// No daemon runs and `ensure` was told not to start one.
    NotRunning,
    /// A daemon runs that this binary cannot use and did not replace.
    Incompatible,
    /// The engine cannot be reached or started.
    Unavailable,
}

impl EnsureOutcome {
    /// The exit code of `raptor daemon ensure` with this outcome (2 is a usage error).
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Incompatible => 3,
            Self::Unavailable => 4,
            Self::NotRunning => 5,
        }
    }
}

/// Why the running daemon cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct IncompatibleReport {
    pub which: IncompatibleWhich,
    /// The capabilities asked for that the daemon does not serve.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncompatibleWhich {
    /// The daemon speaks an older protocol.
    DaemonOlder,
    /// The daemon speaks a newer protocol: the caller must be updated.
    AppOlder,
    /// Same protocol, but a capability asked for is not served.
    MissingCapabilities,
}

/// Why the engine is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct UnavailableReport {
    pub reason: UnavailableReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    /// Starting the daemon failed.
    LaunchFailed,
    /// The daemon did not complete the handshake in time.
    StartTimeout,
    /// The channel's folder or its server is not this user's (L-06).
    ChannelRejected,
    /// No channel transport on this platform.
    Unsupported,
    /// The profile could not be resolved.
    Profile,
}

/// The login autostart of the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AutostartReport {
    pub registered: bool,
    /// The binary the autostart runs, when registered and readable.
    pub binary: Option<String>,
}
