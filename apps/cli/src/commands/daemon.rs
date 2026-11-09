//! `raptor daemon`: the engine in the foreground and its control (US-GRP-003,
//! US-GRP-004).

use std::process::ExitCode;
use std::time::Duration;

use gitraptor_api::client::ensure::{
    self, AutostartReport, EnsureOutcome, EnsureReport, IncompatibleReport, IncompatibleWhich,
    UnavailableReason, UnavailableReport,
};
use gitraptor_api::client::{
    Client as ApiClient, Ensured, ReplacePolicy,
    ensure_daemon_policy, peer,
};
use gitraptor_api::messages::{ClientKind, IncompatibleData};
use gitraptor_api::rpc::code;
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::autostart::{Autostart, start_failure_exit};
use gitraptor_core::client::{Client, ClientError, ClientOptions};
use gitraptor_core::daemon::{self, DaemonConfig, DaemonError};
use gitraptor_core::profile::ProfileDirs;

use super::Global;
use crate::i18n::t;
use crate::{autostart, codes, confirm, engine, error_text, profile_dirs, refusal_text, snapshot, status};

/// Run the GitRaptor engine in the foreground (one per user).
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Started by the login autostart: a start that fails exits with 0,
    /// so the service manager does not relaunch it in a loop.
    #[arg(long, hide = true)]
    autostart: bool,
    #[command(subcommand)]
    action: Option<DaemonAction>,
}

#[derive(clap::Subcommand)]
enum DaemonAction {
    /// Ask the running engine to stop in order (reserved to the developer).
    Stop {
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show the engine state, starting the engine if it is not running.
    Status,
    /// Start the engine at login (launchd, systemd --user or HKCU Run).
    Enable,
    /// Stop starting the engine at login. The running engine keeps running.
    Disable,
    /// Connect to the engine, starting it if needed, and print one JSON report line (the
    /// desktop app's way to the engine, ADR-GRP-014 Enmienda DEP-DSK-5 § D2). Never replaces
    /// a daemon run from another binary.
    #[command(hide = true)]
    Ensure {
        /// Print the report as one JSON line (required: the only output).
        #[arg(long, required = true)]
        json: bool,
        /// Do not start the engine if it is not running.
        #[arg(long)]
        no_start: bool,
        /// A capability the engine must serve (repeatable).
        #[arg(long = "require", value_name = "NAME")]
        require: Vec<String>,
    },
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            None => run_daemon(self.autostart),
            Some(DaemonAction::Stop { yes }) => stop_daemon(yes),
            Some(DaemonAction::Status) => daemon_status(),
            Some(DaemonAction::Enable) => autostart::enable(),
            Some(DaemonAction::Disable) => autostart::disable(),
            Some(DaemonAction::Ensure {
                json: _,
                no_start,
                require,
            }) => daemon_ensure(no_start, &require),
        }
    }
}

/// `raptor daemon`. Started by the login autostart (`--autostart`), a
/// start that fails exits with 0 (US-GRP-004, ADR-GRP-015).
fn run_daemon(autostart: bool) -> ExitCode {
    let result = DaemonConfig::for_current_user().and_then(daemon::run_process);
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("raptor daemon: {err}");
            let already = matches!(err, DaemonError::AlreadyRunning { .. });
            ExitCode::from(start_failure_exit(autostart, already))
        }
    }
}

/// `raptor daemon stop`: a reserved command (ADR-GRP-005 § 6). The
/// confirmation is a UX step only; the daemon decides on its own whether
/// the caller is the developer.
fn stop_daemon(yes: bool) -> ExitCode {
    let dirs = match profile_dirs() {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    let mut client = match Client::connect(&dirs, ClientKind::Cli, gitraptor_api::PROTOCOL_VERSION)
    {
        Ok(client) => client,
        Err(ClientError::NotRunning) => {
            println!("raptor daemon: {}", t("common.not-running", &[]));
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {}", error_text(err));
            return ExitCode::FAILURE;
        }
    };
    if !yes && !confirm(&t("daemon.stop.confirm", &[])) {
        eprintln!("raptor daemon stop: {}", t("common.cancelled", &[]));
        return ExitCode::FAILURE;
    }
    let pid = client.hello().daemon_pid;
    match client.stop_daemon() {
        Ok(_) => {
            client.wait_closed(Duration::from_secs(10));
            let state = dirs.state.clone();
            let released =
                daemon::wait_until_released(&state, Duration::from_secs(10)).unwrap_or(false);
            if released {
                println!("raptor daemon: {}", t("daemon.stopped", &[("pid", &pid)]));
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "raptor daemon stop: {}",
                    t("daemon.stop.timeout", &[("pid", &pid)])
                );
                ExitCode::FAILURE
            }
        }
        Err(ClientError::Rpc(err)) if err.code == code::RESERVED_REFUSED => {
            eprintln!(
                "raptor daemon stop: {}",
                refusal_text(&err, "daemon.stop.refused-agent")
            );
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {}", error_text(err));
            ExitCode::FAILURE
        }
    }
}

/// `raptor daemon status`: diagnostic, read-only. Starts the engine on
/// demand and prints the snapshot, sanitized (SEC-12). Its presentation is
/// not a commitment (F-001-02 owns it).
fn daemon_status() -> ExitCode {
    const CMD: &str = "raptor daemon status";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    println!(
        "{}",
        t(
            "daemon.status.engine",
            &[
                ("state", &status::wire(&snapshot.engine.state)),
                ("pid", &snapshot.daemon.pid),
                ("protocol", &snapshot.daemon.protocol),
            ],
        )
    );
    match snapshot.engine.git_version.as_deref() {
        Some(version) => println!(
            "{}",
            t("daemon.status.git", &[("version", &sanitize(version))])
        ),
        None => println!("{}", t("daemon.status.git-missing", &[])),
    }
    if let Some(requester) = &client.hello().requester {
        println!("{}", codes::requester_text(requester));
    }
    if let Ok(gitraptor_api::scope::ScopeSnapshot::Global(global)) = client.call(
        gitraptor_api::methods::SCOPE_SNAPSHOT,
        gitraptor_api::scope::ScopeSnapshotParams {
            scope: gitraptor_api::scope::Scope::Global,
        },
    ) {
        println!("{}", codes::autostart_text(global.autostart));
    }
    for repo in &snapshot.repos {
        // Everything that comes from the daemon is printed sanitized: the
        // socket could be served by an impostor of the same user.
        println!(
            "repo {} {} {}",
            sanitize(&repo.repo_id),
            sanitize(&status::wire(&repo.state)),
            repo.path.sanitized()
        );
    }
    ExitCode::SUCCESS
}

/// `raptor daemon ensure --json`: one [`EnsureReport`] line on stdout and its exit code
/// (DS-TS-DSK-002 § Informe de `ensure`). Nothing on stderr: the caller reads only the report.
/// With `--no-start` it only connects; otherwise it starts the engine on demand and asks a
/// running daemon to step down only if it runs this very binary (SEC-13, D2).
fn daemon_ensure(no_start: bool, require: &[String]) -> ExitCode {
    let report = ensure_report(no_start, require);
    let Ok(line) = serde_json::to_string(&report) else {
        return ExitCode::from(EnsureOutcome::Unavailable.exit_code());
    };
    println!("{line}");
    ExitCode::from(report.outcome.exit_code())
}

fn ensure_report(no_start: bool, require: &[String]) -> EnsureReport {
    let mut report = EnsureReport {
        schema: ensure::SCHEMA,
        outcome: EnsureOutcome::Unavailable,
        runtime: None,
        daemon_pid: None,
        daemon_path: None,
        daemon_version: None,
        daemon_protocol: None,
        started: false,
        replaced: false,
        incompatible: None,
        unavailable: None,
        autostart: autostart_report(),
    };
    let Ok(dirs) = ProfileDirs::resolve() else {
        report.unavailable = Some(UnavailableReport {
            reason: UnavailableReason::Profile,
        });
        return report;
    };
    let mut options = ClientOptions::new(dirs, ClientKind::Cli);
    options.capabilities = require.iter().map(|name| capability_name(name)).collect();
    let Ok(connect) = options.connect() else {
        report.unavailable = Some(UnavailableReport {
            reason: UnavailableReason::Profile,
        });
        return report;
    };
    report.runtime = Some(connect.runtime.to_string_lossy().into_owned());
    // The pid of the last daemon the replace policy was asked about: the only way to name the
    // daemon of an incompatibility, whose connection `ensure_daemon_policy` does not return.
    let mut asked: Option<u32> = None;
    let result = if no_start {
        ApiClient::connect(&connect.runtime, connect.kind, connect.protocol).map(|client| {
            Ensured {
                client,
                started: false,
                replaced: false,
            }
        })
    } else {
        let mut only_same_binary = |pid: u32| {
            asked = Some(pid);
            same_binary(pid)
        };
        ensure_daemon_policy(
            &connect,
            &mut options.launcher(),
            &mut || {},
            ReplacePolicy::OnlyIf(&mut only_same_binary),
        )
    };
    match result {
        Ok(ensured) => {
            let client = ensured.client;
            report.started = ensured.started;
            report.replaced = ensured.replaced;
            report.daemon_pid = client.peer_pid();
            report.daemon_path = report.daemon_pid.and_then(executable_text);
            report.daemon_version = Some(client.hello().binary_version.clone());
            report.daemon_protocol = Some(client.hello().protocol);
            let missing = client.missing_capabilities(&connect.capabilities);
            if missing.is_empty() {
                report.outcome = EnsureOutcome::Ready;
            } else {
                report.outcome = EnsureOutcome::Incompatible;
                report.incompatible = Some(IncompatibleReport {
                    which: IncompatibleWhich::MissingCapabilities,
                    missing: missing.into_iter().map(str::to_owned).collect(),
                });
            }
        }
        Err(ClientError::NotRunning) => report.outcome = EnsureOutcome::NotRunning,
        Err(ClientError::Incompatible(data)) => {
            incompatible(&mut report, IncompatibleWhich::DaemonOlder, &data, asked);
        }
        Err(ClientError::ClientTooOld(data)) => {
            incompatible(&mut report, IncompatibleWhich::AppOlder, &data, asked);
        }
        Err(err) => {
            report.unavailable = Some(UnavailableReport {
                reason: unavailable_reason(&err),
            });
        }
    }
    report
}

fn incompatible(
    report: &mut EnsureReport,
    which: IncompatibleWhich,
    data: &IncompatibleData,
    pid: Option<u32>,
) {
    report.outcome = EnsureOutcome::Incompatible;
    report.daemon_pid = pid;
    report.daemon_path = pid.and_then(executable_text);
    report.daemon_version = Some(data.binary_version.clone());
    report.daemon_protocol = Some(data.daemon_protocol);
    report.incompatible = Some(IncompatibleReport {
        which,
        missing: Vec::new(),
    });
}

/// Why the engine is unavailable. An I/O or protocol failure on the channel has no reason of
/// its own in the report: the engine could not be brought up, `launch-failed`.
fn unavailable_reason(err: &ClientError) -> UnavailableReason {
    match err {
        ClientError::StartTimeout => UnavailableReason::StartTimeout,
        ClientError::ChannelRejected | ClientError::NotAuthentic => {
            UnavailableReason::ChannelRejected
        }
        ClientError::TransportUnsupported | ClientError::Unsupported(_) => {
            UnavailableReason::Unsupported
        }
        _ => UnavailableReason::LaunchFailed,
    }
}

/// The capability name as `ClientOptions` keeps it (`&'static str`): the contract's own name
/// when it is known. An unknown name is leaked: `raptor daemon ensure` is a one-shot process
/// that runs this once, for the few names of its command line, so the memory is bounded and
/// returned when the process exits.
fn capability_name(name: &str) -> &'static str {
    gitraptor_api::capability::all()
        .map(|c| c.name)
        .find(|known| *known == name)
        .unwrap_or_else(|| String::leak(name.to_owned()))
}

/// `true` if `pid` runs this very binary: both executables, canonical, are the same file
/// path. Anything unreadable is `false`, so the daemon is never asked to step down.
fn same_binary(pid: u32) -> bool {
    let Ok(own) = std::env::current_exe().and_then(|exe| exe.canonicalize()) else {
        return false;
    };
    peer::executable_of(pid)
        .and_then(|exe| exe.canonicalize().ok())
        .is_some_and(|exe| exe == own)
}

fn executable_text(pid: u32) -> Option<String> {
    peer::executable_of(pid).map(|exe| exe.to_string_lossy().into_owned())
}

/// The login autostart of the user, as far as it can be read.
fn autostart_report() -> AutostartReport {
    match Autostart::for_current_user() {
        Some(autostart) => AutostartReport {
            registered: autostart.is_registered(),
            binary: autostart
                .registered_exe()
                .map(|exe| exe.to_string_lossy().into_owned()),
        },
        None => AutostartReport {
            registered: false,
            binary: None,
        },
    }
}
