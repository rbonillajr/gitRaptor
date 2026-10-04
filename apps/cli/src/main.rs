use std::process::ExitCode;

use clap::{Parser, Subcommand};
use std::time::Duration;

use gitraptor_core::daemon::{
    self, DaemonConfig, DaemonError, EXIT_ALREADY_RUNNING, signal_running_daemon,
    wait_until_released,
};

/// The Git copilot for teams that code with AI agents.
#[derive(Parser)]
#[command(name = "raptor", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the GitRaptor engine in the foreground (one per user).
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Ask the running engine to stop in order.
    Stop,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        None => {
            println!(
                "raptor {} (api {})",
                gitraptor_core::version(),
                gitraptor_core::API_VERSION
            );
            ExitCode::SUCCESS
        }
        Some(Command::Daemon { action: None }) => run_daemon(),
        Some(Command::Daemon {
            action: Some(DaemonAction::Stop),
        }) => stop_daemon(),
    }
}

fn run_daemon() -> ExitCode {
    let result = DaemonConfig::for_current_user().and_then(daemon::run_process);
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err @ DaemonError::AlreadyRunning { .. }) => {
            eprintln!("raptor daemon: {err}");
            ExitCode::from(EXIT_ALREADY_RUNNING as u8)
        }
        Err(err) => {
            eprintln!("raptor daemon: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Provisional stop until the channel exists (TS-GRP-004): signals the
/// daemon that holds the instance lock, like `kill` would. The stop is
/// recorded as "by signal", never as an attributed stop (SEC-13).
fn stop_daemon() -> ExitCode {
    let result = DaemonConfig::for_current_user().and_then(|config| {
        let state = config.dirs.state;
        let pid = signal_running_daemon(&state)?;
        let stopped = match pid {
            Some(_) => wait_until_released(&state, Duration::from_secs(10))?,
            None => true,
        };
        Ok((pid, stopped))
    });
    match result {
        Ok((Some(pid), true)) => {
            println!("raptor daemon: stopped (pid {pid})");
            ExitCode::SUCCESS
        }
        Ok((Some(pid), false)) => {
            eprintln!("raptor daemon stop: pid {pid} did not stop within 10 s");
            ExitCode::FAILURE
        }
        Ok((None, _)) => {
            println!("raptor daemon: not running");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
