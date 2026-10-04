//! Requests to the daemon loop (ADR-GRP-005 § 4).
//!
//! Every stop goes through a [`ShutdownHandle`]: termination signals, and
//! the stop command of the channel once the daemon authorized it as a
//! reserved command (TS-GRP-004). The channel also asks the loop, which owns
//! the profile, to append to and read the reserved-command audit.

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::Duration;

use crate::profile::AuditRow;

/// How long a channel thread waits for the loop to persist or read the audit.
#[cfg_attr(not(unix), allow(dead_code))]
const AUDIT_TIMEOUT: Duration = Duration::from_secs(5);

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
}

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
