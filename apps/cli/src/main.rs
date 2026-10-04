use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use gitraptor_api::messages::{ClientKind, RefusalReason, RefusedData, Snapshot};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::daemon::{self, DaemonConfig, DaemonError, EXIT_ALREADY_RUNNING};
use gitraptor_core::profile::ProfileDirs;

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
    /// Ask the running engine to stop in order (reserved to the developer).
    Stop {
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show the engine state, starting the engine if it is not running.
    Status,
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
            action: Some(DaemonAction::Stop { yes }),
        }) => stop_daemon(yes),
        Some(Command::Daemon {
            action: Some(DaemonAction::Status),
        }) => status(),
    }
}

/// User-facing text in the user's language (en/es).
fn tr(en: &'static str, es: &'static str) -> &'static str {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    if lang.starts_with("es") { es } else { en }
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

fn profile_dirs() -> Result<ProfileDirs, ExitCode> {
    ProfileDirs::resolve().map_err(|err| {
        eprintln!("raptor: {err}");
        ExitCode::FAILURE
    })
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
            println!("raptor daemon: {}", tr("not running", "no está en marcha"));
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    if !yes
        && !confirm(tr(
            "Stop the GitRaptor engine? Agent activity will not be observed until it starts again. [y/N] ",
            "¿Parar el motor de GitRaptor? La actividad de los agentes no se observará hasta que vuelva a arrancar. [s/N] ",
        ))
    {
        eprintln!("raptor daemon stop: {}", tr("cancelled", "cancelado"));
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
                println!("raptor daemon: {} (pid {pid})", tr("stopped", "parado"));
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "raptor daemon stop: pid {pid} {}",
                    tr("did not stop within 10 s", "no paró en 10 s")
                );
                ExitCode::FAILURE
            }
        }
        Err(ClientError::Rpc(err)) if err.code == code::RESERVED_REFUSED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<RefusedData>(d).ok())
                .map(|d| d.reason);
            eprintln!("raptor daemon stop: {}", refusal_text(reason));
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {}", sanitize(&err.to_string()));
            ExitCode::FAILURE
        }
    }
}

fn refusal_text(reason: Option<RefusalReason>) -> &'static str {
    match reason {
        Some(RefusalReason::AgentAncestry | RefusalReason::SessionLeaderAgent) => tr(
            "refused: only the developer can stop the engine, not an agent or a process started by one",
            "rechazado: solo el desarrollador puede parar el motor, no un agente ni un proceso lanzado por él",
        ),
        Some(RefusalReason::NoControllingTerminal) => tr(
            "refused: run it from your own terminal",
            "rechazado: ejecútalo desde tu propia terminal",
        ),
        _ => tr(
            "refused: the engine could not verify who is asking",
            "rechazado: el motor no pudo verificar quién lo pide",
        ),
    }
}

/// Asks on the terminal. Without a terminal there is nobody to ask: no.
fn confirm(question: &str) -> bool {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        eprintln!(
            "raptor: {}",
            tr(
                "confirmation needs a terminal (or pass --yes)",
                "la confirmación necesita una terminal (o usa --yes)"
            )
        );
        return false;
    }
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if stdin.lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "sí"
    )
}

/// `raptor daemon status`: diagnostic, read-only. Starts the engine on
/// demand and prints the snapshot, sanitized (SEC-12). Its presentation is
/// not a commitment (F-001-02 owns it).
fn status() -> ExitCode {
    let dirs = match profile_dirs() {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    let mut client = match ensure_daemon(&ClientOptions::new(dirs, ClientKind::Cli)) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("raptor daemon status: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    let snapshot: Snapshot = match client.call(methods::ENGINE_SNAPSHOT, serde_json::json!({})) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            eprintln!("raptor daemon status: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    let state = serde_json::to_value(snapshot.engine.state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    println!(
        "{} {} (pid {}, {} {})",
        tr("engine:", "motor:"),
        state,
        snapshot.daemon.pid,
        tr("protocol", "protocolo"),
        snapshot.daemon.protocol
    );
    println!(
        "git: {}",
        snapshot
            .engine
            .git_version
            .as_deref()
            .map(sanitize)
            .unwrap_or_else(|| tr("not found", "no encontrado").to_owned())
    );
    for repo in &snapshot.repos {
        let state = serde_json::to_value(repo.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        // Everything that comes from the daemon is printed sanitized: the
        // socket could be served by an impostor of the same user.
        println!(
            "repo {} {} {}",
            sanitize(&repo.repo_id),
            sanitize(&state),
            repo.path.sanitized()
        );
    }
    ExitCode::SUCCESS
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
