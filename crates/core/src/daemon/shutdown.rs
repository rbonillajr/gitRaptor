//! Requests to stop the daemon in order (ADR-GRP-005 § 4).
//!
//! Every stop goes through a [`ShutdownHandle`]: termination signals today,
//! the authenticated stop command of the channel tomorrow (TS-GRP-004). The
//! daemon loop owns the receiving end.

use std::sync::mpsc::{Receiver, Sender, channel};

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
}

impl StopCause {
    /// Stable cause text stored in the profile.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Signal(_) => "signal",
            Self::StopCommand { .. } => "stop-command",
        }
    }

    /// Who asked for the stop, as stored with the cause (SEC-13). For a
    /// signal, only which signal: it is a diagnostic, not an identity.
    pub fn requested_by(&self) -> String {
        match self {
            Self::Signal(name) => ["signal:", name].concat(),
            Self::StopCommand { requested_by } => requested_by.clone(),
        }
    }
}

/// Sends stop requests to the running daemon. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    tx: Sender<StopCause>,
}

impl ShutdownHandle {
    pub(crate) fn new() -> (Self, Receiver<StopCause>) {
        let (tx, rx) = channel();
        (Self { tx }, rx)
    }

    /// Asks the daemon to stop. Returns `false` if it already stopped.
    pub fn request(&self, cause: StopCause) -> bool {
        self.tx.send(cause).is_ok()
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

/// Sends `SIGTERM` to the daemon holding the lock of `state_dir`. The lock
/// is checked right before signalling: a PID read from a released lock is
/// never signalled. Returns the PID signalled, or `None` if no daemon runs.
#[cfg(unix)]
pub fn signal_running_daemon(
    state_dir: &std::path::Path,
) -> Result<Option<u32>, super::DaemonError> {
    let Some(pid) = super::lock::running_pid(state_dir)? else {
        return Ok(None);
    };
    let raw = i32::try_from(pid).map_err(|_| std::io::Error::other("PID out of range"))?;
    let pid_t =
        rustix::process::Pid::from_raw(raw).ok_or_else(|| std::io::Error::other("invalid PID"))?;
    rustix::process::kill_process(pid_t, rustix::process::Signal::TERM)
        .map_err(std::io::Error::from)?;
    Ok(Some(pid))
}

/// Not available before the channel exists on Windows (TS-GRP-004).
#[cfg(not(unix))]
pub fn signal_running_daemon(
    _state_dir: &std::path::Path,
) -> Result<Option<u32>, super::DaemonError> {
    Err(super::DaemonError::Unsupported(
        "`raptor daemon stop` needs the local channel on Windows (TS-GRP-004)",
    ))
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
        assert_eq!(rx.recv().unwrap(), StopCause::Signal("INT"));
        drop(rx);
        assert!(!handle.request(StopCause::Signal("INT")));
    }
}
