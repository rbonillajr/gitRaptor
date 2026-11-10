//! The app's real [`Connector`] (ADR-GRP-014 Enmienda DEP-DSK-5 § D1, D3, D4, D5, D9): it runs
//! the `raptor` of its own bundle (`raptor daemon ensure --json`), reads its [`EnsureReport`]
//! and connects to the runtime folder the report names, checking that the socket is served by
//! an installed `raptor` before the handshake (B-3).
//!
//! The app never reads the environment (M-4): the `raptor` is the sibling of the app's own
//! canonical executable, never one found in `PATH`; it runs with a fixed argv, an empty
//! environment plus a fixed `PATH`, no shell, and a bounded output and time.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::client::ensure::{
    EnsureOutcome, EnsureReport, IncompatibleWhich, UnavailableReason,
};
use gitraptor_api::client::{Client, installed, peer};
use gitraptor_api::messages::{ClientKind, HelloResult};
use gitraptor_api::rpc::Notification;
use serde_json::Value;

use super::link::{
    ClientLink, ConnectFailure, Connector, Incompatibility, IncompatibilityKind, Link, LinkError,
    REQUIRED_CAPABILITIES, map_client_error,
};

/// The only `PATH` the bundled `raptor` gets.
pub(crate) const FIXED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Most bytes of the report read from `raptor`.
pub(crate) const OUTPUT_LIMIT: usize = 64 * 1024;

/// How long `raptor daemon ensure` may run before it is killed.
pub(crate) const ENSURE_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the process is polled while it runs.
pub(crate) const POLL: Duration = Duration::from_millis(20);

/// Connects through the bundle's `raptor`. One per process; it keeps the runtime folder of the
/// last report, so a connection without starting the engine goes straight to the channel (D9).
pub struct BundleConnector {
    /// `None`: resolved on every connection with [`bundled_raptor`].
    raptor: Option<PathBuf>,
    /// Whether the pid serving the socket is an installed `raptor` (B-3).
    verify_peer: fn(u32) -> bool,
    /// The runtime folder of the last report, always absolute.
    runtime: Option<PathBuf>,
    /// The executable that serves the daemon, from the last `ready` report.
    daemon_exe: Option<PathBuf>,
}

/// A [`ClientLink`] that knows the executable that serves its daemon.
struct BundleLink {
    inner: ClientLink,
    daemon_exe: Option<PathBuf>,
}

impl Link for BundleLink {
    fn hello(&self) -> &HelloResult {
        self.inner.hello()
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.inner.call(method, params)
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError> {
        self.inner.next(timeout)
    }

    fn daemon_exe(&self) -> Option<&Path> {
        self.daemon_exe.as_deref()
    }
}

impl BundleConnector {
    /// The connector of the installed app. Never fails: a missing or unfit `raptor` is a
    /// [`ConnectFailure::LaunchFailed`] of each connection.
    pub fn new() -> Self {
        Self {
            raptor: None,
            verify_peer: peer::installed_raptor_peer,
            runtime: None,
            daemon_exe: None,
        }
    }

    /// Test seam: `raptor` instead of the bundled one and `verify_peer` instead of the check of
    /// an installed `raptor` peer. Only tests call it.
    #[doc(hidden)]
    pub fn for_tests(raptor: PathBuf, verify_peer: fn(u32) -> bool) -> Self {
        Self {
            raptor: Some(raptor),
            verify_peer,
            runtime: None,
            daemon_exe: None,
        }
    }

    /// Connects to the channel in `runtime`, refusing a server that `verify_peer` refuses.
    fn open(&self, runtime: &Path) -> Result<Box<dyn Link>, ConnectFailure> {
        Client::connect_runtime(runtime, ClientKind::Cli, PROTOCOL_VERSION, self.verify_peer)
            .map(|client| {
                Box::new(BundleLink {
                    inner: ClientLink(client),
                    daemon_exe: self.daemon_exe.clone(),
                }) as Box<dyn Link>
            })
            .map_err(map_client_error)
    }
}

impl Default for BundleConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for BundleConnector {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        if !start && let Some(runtime) = &self.runtime {
            return self.open(runtime);
        }
        let raptor = match &self.raptor {
            Some(raptor) => raptor.clone(),
            None => bundled_raptor().ok_or(ConnectFailure::LaunchFailed)?,
        };
        let report = run_ensure(&raptor, start)?;
        // A relative runtime is never used: it would resolve against the app's own folder.
        let runtime = report
            .runtime
            .as_deref()
            .map(PathBuf::from)
            .filter(|runtime| runtime.is_absolute());
        // Only a runtime the report called ready is remembered: after an incompatible or
        // rejected report the next attempt must ask `ensure` again, which says why.
        outcome(&report)?;
        let runtime = runtime.ok_or(ConnectFailure::LaunchFailed)?;
        self.runtime = Some(runtime.clone());
        // Only an absolute path names an executable; a relative one would resolve against the
        // app's own folder.
        self.daemon_exe = report
            .daemon_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        self.open(&runtime)
    }
}

/// The failure a report stands for; `Ok` when it is `ready`. A report whose details do not match
/// its outcome is not understood: [`ConnectFailure::LaunchFailed`].
fn outcome(report: &EnsureReport) -> Result<(), ConnectFailure> {
    match report.outcome {
        EnsureOutcome::Ready => Ok(()),
        EnsureOutcome::NotRunning => Err(ConnectFailure::NotRunning),
        EnsureOutcome::Unavailable => Err(match report.unavailable.as_ref().map(|u| u.reason) {
            Some(UnavailableReason::ChannelRejected) => ConnectFailure::Rejected,
            Some(UnavailableReason::Unsupported) => ConnectFailure::Unsupported,
            _ => ConnectFailure::LaunchFailed,
        }),
        EnsureOutcome::Incompatible => {
            let Some(details) = &report.incompatible else {
                return Err(ConnectFailure::LaunchFailed);
            };
            Err(ConnectFailure::Incompatible(Incompatibility {
                which: match details.which {
                    IncompatibleWhich::DaemonOlder => IncompatibilityKind::DaemonOlder,
                    IncompatibleWhich::AppOlder => IncompatibilityKind::AppOlder,
                    IncompatibleWhich::MissingCapabilities => {
                        IncompatibilityKind::MissingCapabilities
                    }
                },
                daemon_version: report.daemon_version.clone(),
                daemon_path: report.daemon_path.clone(),
                missing: details.missing.clone(),
            }))
        }
    }
}

/// The `raptor` next to the app's own canonical executable, canonical, in that same folder and
/// an installed `raptor` ([`installed::is_installed_raptor`]); `None` otherwise. Never `PATH`.
pub(crate) fn bundled_raptor() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    sibling_raptor(&exe)
}

fn sibling_raptor(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let raptor = dir.join("raptor").canonicalize().ok()?;
    if raptor.parent()? != dir {
        return None;
    }
    installed::is_installed_raptor(&raptor).then_some(raptor)
}

/// Runs `raptor daemon ensure --json` and reads its report: the first line of its output, read
/// up to [`OUTPUT_LIMIT`] bytes. A `raptor` that cannot run, outlives [`ENSURE_TIMEOUT`] (it is
/// killed) or prints anything but a report is [`ConnectFailure::LaunchFailed`].
pub(crate) fn run_ensure(raptor: &Path, start: bool) -> Result<EnsureReport, ConnectFailure> {
    let mut command = Command::new(raptor);
    command.args(["daemon", "ensure", "--json"]);
    if !start {
        command.arg("--no-start");
    }
    for capability in REQUIRED_CAPABILITIES {
        command.arg("--require").arg(capability);
    }
    command
        .env_clear()
        .env("PATH", FIXED_PATH)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| ConnectFailure::LaunchFailed)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ConnectFailure::LaunchFailed);
    };
    // The reader sends the first line as soon as it has it: a process that keeps the pipe open
    // after `raptor` exits never holds the report back.
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(first_line(stdout));
    });
    let deadline = Instant::now() + ENSURE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ConnectFailure::LaunchFailed);
            }
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let line = received
        .recv_timeout(remaining)
        .map_err(|_| ConnectFailure::LaunchFailed)?;
    serde_json::from_slice(&line).map_err(|_| ConnectFailure::LaunchFailed)
}

/// The bytes before the first newline of `source`, reading at most [`OUTPUT_LIMIT`] bytes.
fn first_line(source: impl Read) -> Vec<u8> {
    let mut limited = source.take(OUTPUT_LIMIT as u64);
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match limited.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let read = &chunk[..n];
                if let Some(end) = read.iter().position(|b| *b == b'\n') {
                    out.extend_from_slice(&read[..end]);
                    break;
                }
                out.extend_from_slice(read);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_stops_at_the_newline_and_at_the_limit() {
        assert_eq!(first_line(&b"{\"a\":1}\nrest"[..]), b"{\"a\":1}");
        assert_eq!(first_line(&b"no newline"[..]), b"no newline");
        let long = vec![b'x'; OUTPUT_LIMIT * 2];
        assert_eq!(first_line(&long[..]).len(), OUTPUT_LIMIT);
    }

    #[test]
    fn an_executable_without_a_raptor_beside_it_has_no_bundled_raptor() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert_eq!(sibling_raptor(&dir.join("lib.rs")), None);
    }
}
