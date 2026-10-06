//! Requests to the daemon loop (ADR-GRP-005 § 4).
//!
//! Every stop goes through a [`ShutdownHandle`]: termination signals, and
//! the stop command of the channel once the daemon authorized it as a
//! reserved command (TS-GRP-004). The channel also asks the loop, which owns
//! the profile, to append to and read the reserved-command audit.

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::Duration;

use gitraptor_api::guard::{GuardPlan, GuardStatus, InstallBlocker};
use gitraptor_api::messages::{
    EventsHistoryParams, GitEventView, RegistrationRegisterResult, RegistrationRejection,
    RegistrationWithdrawResult, RepoAddResult, RepoRetireResult,
};

use crate::observe::RepoRead;
use crate::profile::AuditRow;
use crate::watch::ObservedBatch;

/// How long a channel thread waits for the loop to persist or read the audit.
#[cfg_attr(not(unix), allow(dead_code))]
const AUDIT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a channel thread waits for the loop to add or retire a repo.
#[cfg_attr(not(unix), allow(dead_code))]
const REPO_TIMEOUT: Duration = Duration::from_secs(30);

/// Why the loop could not add or retire a repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoCommandError {
    /// `repo.retire` of an id the profile does not know.
    UnknownRepo,
    /// The profile could not be written (or the loop did not answer).
    Internal,
}

/// Why the loop refused a registration or its withdrawal (US-GRP-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    Rejected(RegistrationRejection),
    /// The profile could not be written (or the loop did not answer).
    Internal,
}

/// A registration the channel authorized: who asks, for which agent and
/// where (US-GRP-009, ADR-GRP-005 § 6.6).
#[derive(Debug, Clone)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct RegisterRequest {
    pub agent: crate::profile::Agent,
    /// The developer's named folder, or the agent's working folder.
    pub folder: std::path::PathBuf,
    /// For an agent, the worktree it named, if any: it must be the one of
    /// its working folder, compared without reading that path.
    pub named: Option<std::path::PathBuf>,
    pub author: crate::profile::Author,
    /// The detected session the caller descends from, if any: the one it
    /// confirms (Q39).
    pub caller_session: Option<String>,
}

/// A withdrawal the channel authorized as a reserved command.
#[derive(Debug, Clone)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct WithdrawRequest {
    pub agent: crate::profile::Agent,
    pub folder: std::path::PathBuf,
}

/// A Guardrails request to the loop, for a repo the channel already located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum GuardRequest {
    Plan,
    Status,
    Install,
    Decline,
}

/// What the loop answers to a [`GuardRequest`].
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum GuardReply {
    Plan(Box<GuardPlan>),
    Status(Box<GuardStatus>),
    /// The repo is not observed (Q-GRD-15).
    NotObserved,
    /// The install was refused before writing anything.
    Rejected(Vec<InstallBlocker>),
    /// A step failed and was reverted, or the profile is unavailable.
    Failed,
}

/// How long a channel thread waits for the loop to install the hook layer.
#[cfg_attr(not(unix), allow(dead_code))]
const GUARD_TIMEOUT: Duration = Duration::from_secs(60);

/// A repo the channel already located and read, for the loop to add.
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct RepoAddRequest {
    pub common_dir: std::path::PathBuf,
    pub read: RepoRead,
    /// Monotonic time the request arrived and its read finished
    /// (ADR-GRP-011 § 3).
    pub t_recv: u64,
    pub t_computed: u64,
}

/// Why the daemon stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopCause {
    /// A termination signal (`TERM`, `INT`, `HUP`). launchd and systemd stop
    /// the daemon at logout with `TERM`. A signal does not say who sent it,
    /// so it never counts as an attributed stop (SEC-13).
    Signal(&'static str),
    /// Stop command authorized by the daemon as a reserved command, with the
    /// client that asked for it (TS-GRP-004).
    StopCommand { requested_by: String },
    /// A newer installed binary replaces this daemon (SEC-13). Recorded with
    /// the client and this daemon's protocol: the next start only counts it
    /// as an attributed stop if it really speaks a newer protocol.
    Replace { requested_by: String, protocol: u32 },
}

impl StopCause {
    /// Stable cause text stored in the profile.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Signal(_) => "signal",
            Self::StopCommand { .. } => "stop-command",
            Self::Replace { .. } => "replace",
        }
    }

    /// The cause as a contract code (N7).
    pub fn code(&self) -> gitraptor_api::messages::StopCauseCode {
        use gitraptor_api::messages::StopCauseCode;
        match self {
            Self::Signal(_) => StopCauseCode::Signal,
            Self::StopCommand { .. } => StopCauseCode::StopCommand,
            Self::Replace { .. } => StopCauseCode::Replace,
        }
    }

    /// Cause text stored in the profile: [`Self::as_str`], plus the old
    /// protocol for a replacement (`replace:1`).
    pub fn stored(&self) -> String {
        match self {
            Self::Replace { protocol, .. } => format!("replace:{protocol}"),
            other => other.as_str().to_owned(),
        }
    }

    /// Who asked for the stop, as stored with the cause (SEC-13). For a
    /// signal, only which signal: it is a diagnostic, not an identity.
    pub fn requested_by(&self) -> String {
        match self {
            Self::Signal(name) => ["signal:", name].concat(),
            Self::StopCommand { requested_by } | Self::Replace { requested_by, .. } => {
                requested_by.clone()
            }
        }
    }
}

/// A request to the daemon loop.
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum Control {
    Stop(StopCause),
    /// Append to the audit; the reply carries the new entry id.
    Audit(AuditRow, SyncSender<Option<i64>>),
    /// Read the audit after an id.
    AuditList {
        after_id: i64,
        limit: u32,
        reply: SyncSender<Option<Vec<(i64, AuditRow)>>>,
    },
    RepoAdd(
        Box<RepoAddRequest>,
        SyncSender<Result<RepoAddResult, RepoCommandError>>,
    ),
    RepoRetire {
        repo_id: String,
        reply: SyncSender<Result<RepoRetireResult, RepoCommandError>>,
    },
    /// What the observer saw in one window (US-GRP-002): persist, then
    /// publish.
    Observed(Box<ObservedBatch>),
    /// What the session detector saw (US-GRP-007): persist, then publish.
    Sessions(Vec<crate::detect::SessionChange>),
    /// The sessions of the observed repos (US-GRP-007).
    #[cfg_attr(not(unix), allow(dead_code))]
    SessionsList {
        params: gitraptor_api::messages::SessionsListParams,
        reply: SyncSender<SessionsListReply>,
    },
    /// Registers an agent (US-GRP-009).
    #[cfg_attr(not(unix), allow(dead_code))]
    Register {
        request: RegisterRequest,
        reply: SyncSender<Result<RegistrationRegisterResult, RegistrationError>>,
    },
    /// Withdraws a registration (US-GRP-009), already authorized.
    #[cfg_attr(not(unix), allow(dead_code))]
    Withdraw {
        request: WithdrawRequest,
        reply: SyncSender<Result<RegistrationWithdrawResult, RegistrationError>>,
    },
    /// Guardrails (US-GRD-001): plan, status, install or decline.
    Guard {
        common_dir: std::path::PathBuf,
        request: GuardRequest,
        /// Past it the caller was already told it failed: the loop must not act any more.
        deadline: std::time::Instant,
        reply: SyncSender<GuardReply>,
    },
    /// One page of a repo's Git events (US-GRP-002).
    #[cfg_attr(not(unix), allow(dead_code))]
    EventHistory {
        params: EventsHistoryParams,
        reply: SyncSender<Result<Vec<GitEventView>, RepoCommandError>>,
    },
}

/// Answer of `sessions.list`: whether detection is available here, and the
/// sessions.
pub(crate) type SessionsListReply =
    Result<(bool, Vec<gitraptor_api::messages::SessionView>), RepoCommandError>;

/// Sends requests to the running daemon. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    tx: Sender<Control>,
}

impl ShutdownHandle {
    pub(crate) fn new() -> (Self, Receiver<Control>) {
        let (tx, rx) = channel();
        (Self { tx }, rx)
    }

    /// Asks the daemon to stop. Returns `false` if it already stopped.
    pub fn request(&self, cause: StopCause) -> bool {
        self.tx.send(Control::Stop(cause)).is_ok()
    }

    /// Persists one audit entry through the loop. `None` if it could not be
    /// written: the caller must then not run the command (fail-closed).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn audit(&self, row: AuditRow) -> Option<i64> {
        let (reply, rx) = sync_channel(1);
        self.tx.send(Control::Audit(row, reply)).ok()?;
        rx.recv_timeout(AUDIT_TIMEOUT).ok().flatten()
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn audit_list(&self, after_id: i64, limit: u32) -> Option<Vec<(i64, AuditRow)>> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::AuditList {
                after_id,
                limit,
                reply,
            })
            .ok()?;
        rx.recv_timeout(AUDIT_TIMEOUT).ok().flatten()
    }
}

impl ShutdownHandle {
    /// Adds a located and read repo through the loop, which owns the
    /// profile (US-GRP-001).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn repo_add(
        &self,
        request: RepoAddRequest,
    ) -> Result<RepoAddResult, RepoCommandError> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::RepoAdd(Box::new(request), reply))
            .map_err(|_| RepoCommandError::Internal)?;
        rx.recv_timeout(REPO_TIMEOUT)
            .map_err(|_| RepoCommandError::Internal)?
    }

    /// Stops observing a repo through the loop (US-GRP-001).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn repo_retire(
        &self,
        repo_id: String,
    ) -> Result<RepoRetireResult, RepoCommandError> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::RepoRetire { repo_id, reply })
            .map_err(|_| RepoCommandError::Internal)?;
        rx.recv_timeout(REPO_TIMEOUT)
            .map_err(|_| RepoCommandError::Internal)?
    }
}

impl ShutdownHandle {
    /// A Guardrails request through the loop, which owns the stores (US-GRD-001).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn guard(
        &self,
        common_dir: std::path::PathBuf,
        request: GuardRequest,
    ) -> GuardReply {
        let (reply, rx) = sync_channel(1);
        if self
            .tx
            .send(Control::Guard {
                common_dir,
                request,
                deadline: std::time::Instant::now() + GUARD_TIMEOUT - Duration::from_secs(5),
                reply,
            })
            .is_err()
        {
            return GuardReply::Failed;
        }
        rx.recv_timeout(GUARD_TIMEOUT).unwrap_or(GuardReply::Failed)
    }
}

impl ShutdownHandle {
    /// Hands an observed batch to the loop. `false` if it already stopped.
    pub(crate) fn observed(&self, batch: ObservedBatch) -> bool {
        self.tx.send(Control::Observed(Box::new(batch))).is_ok()
    }

    /// Hands session changes to the loop. `false` if it already stopped.
    pub(crate) fn sessions(&self, changes: Vec<crate::detect::SessionChange>) -> bool {
        self.tx.send(Control::Sessions(changes)).is_ok()
    }

    /// Reads the sessions through the loop, which owns the stores
    /// (US-GRP-007).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn sessions_list(
        &self,
        params: gitraptor_api::messages::SessionsListParams,
    ) -> SessionsListReply {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::SessionsList { params, reply })
            .map_err(|_| RepoCommandError::Internal)?;
        rx.recv_timeout(AUDIT_TIMEOUT)
            .map_err(|_| RepoCommandError::Internal)?
    }

    /// Registers an agent through the loop, which owns the stores
    /// (US-GRP-009).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn register(
        &self,
        request: RegisterRequest,
    ) -> Result<RegistrationRegisterResult, RegistrationError> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::Register { request, reply })
            .map_err(|_| RegistrationError::Internal)?;
        rx.recv_timeout(AUDIT_TIMEOUT)
            .map_err(|_| RegistrationError::Internal)?
    }

    /// Withdraws a registration through the loop (US-GRP-009).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn withdraw(
        &self,
        request: WithdrawRequest,
    ) -> Result<RegistrationWithdrawResult, RegistrationError> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::Withdraw { request, reply })
            .map_err(|_| RegistrationError::Internal)?;
        rx.recv_timeout(AUDIT_TIMEOUT)
            .map_err(|_| RegistrationError::Internal)?
    }

    /// Reads one page of a repo's Git events through the loop, which owns
    /// the stores (US-GRP-002).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn event_history(
        &self,
        params: EventsHistoryParams,
    ) -> Result<Vec<GitEventView>, RepoCommandError> {
        let (reply, rx) = sync_channel(1);
        self.tx
            .send(Control::EventHistory { params, reply })
            .map_err(|_| RepoCommandError::Internal)?;
        rx.recv_timeout(AUDIT_TIMEOUT)
            .map_err(|_| RepoCommandError::Internal)?
    }
}

/// Turns `SIGTERM`, `SIGINT` and `SIGHUP` into stop requests on `handle`.
/// Process-wide: only the `raptor daemon` entry point calls it.
#[cfg(unix)]
pub fn install_signal_handlers(handle: ShutdownHandle) -> std::io::Result<()> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP])?;
    std::thread::Builder::new()
        .name("raptor-signals".into())
        .spawn(move || {
            for signal in signals.forever() {
                let name = match signal {
                    SIGTERM => "TERM",
                    SIGINT => "INT",
                    _ => "HUP",
                };
                if !handle.request(StopCause::Signal(name)) {
                    break;
                }
            }
        })?;
    Ok(())
}

/// Windows has no signals. An orderly stop arrives through the channel
/// (TS-GRP-004); the logoff notification needs a hidden window and is
/// pending (known debt: until then a logoff is recorded as a crash).
#[cfg(not(unix))]
pub fn install_signal_handlers(_handle: ShutdownHandle) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_never_carry_an_identity() {
        let cause = StopCause::Signal("TERM");
        assert_eq!(cause.as_str(), "signal");
        assert_eq!(cause.requested_by(), "signal:TERM");
    }

    #[test]
    fn handle_reports_a_stopped_daemon() {
        let (handle, rx) = ShutdownHandle::new();
        assert!(handle.request(StopCause::Signal("INT")));
        assert!(matches!(
            rx.recv().unwrap(),
            Control::Stop(StopCause::Signal("INT"))
        ));
        drop(rx);
        assert!(!handle.request(StopCause::Signal("INT")));
        // Without a loop, the audit is never reported as written.
        assert_eq!(handle.audit_list(0, 1), None);
    }
}
