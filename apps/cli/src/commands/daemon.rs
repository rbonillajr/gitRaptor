//! `raptor daemon`: the engine in the foreground and its control (US-GRP-003,
//! US-GRP-004).

use std::process::ExitCode;
use std::time::Duration;

use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::autostart::start_failure_exit;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{self, DaemonConfig, DaemonError};

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
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            None => run_daemon(self.autostart),
            Some(DaemonAction::Stop { yes }) => stop_daemon(yes),
            Some(DaemonAction::Status) => daemon_status(),
            Some(DaemonAction::Enable) => autostart::enable(),
            Some(DaemonAction::Disable) => autostart::disable(),
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
