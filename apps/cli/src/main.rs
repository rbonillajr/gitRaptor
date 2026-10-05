mod events;
mod i18n;
mod status;

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use gitraptor_api::messages::{
    ClientKind, EventsHistoryParams, EventsHistoryResult, GitEventView, RefusalReason, RefusedData,
    RepoAddOutcome, RepoAddParams, RepoAddResult, RepoRejectedData, RepoRejection,
    RepoRetireParams, RepoRetireResult, Snapshot,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::daemon::{self, DaemonConfig, DaemonError, EXIT_ALREADY_RUNNING};
use gitraptor_core::profile::ProfileDirs;

use i18n::t;

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
    /// Add or retire the repos the engine observes (reserved to the developer).
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Show every observed repo and the state of its worktrees.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show the latest Git events of the observed repos, with their time and actor.
    Events {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// How many events, the most recent ones (at most 200).
        #[arg(long, default_value_t = 50)]
        limit: u32,
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

#[derive(Subcommand)]
enum RepoAction {
    /// Start observing a repo: the root of any of its worktrees, or its Git directory.
    Add {
        /// Defaults to the current folder.
        path: Option<PathBuf>,
    },
    /// Stop observing a repo. Its data in the profile is kept.
    Retire {
        /// The repo's Git directory or any of its worktrees; defaults to the current folder.
        path: Option<PathBuf>,
    },
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
        }) => daemon_status(),
        Some(Command::Repo {
            action: RepoAction::Add { path },
        }) => repo_add(path),
        Some(Command::Repo {
            action: RepoAction::Retire { path },
        }) => repo_retire(path),
        Some(Command::Status { json }) => status(json),
        Some(Command::Events { json, limit }) => events_command(json, limit),
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

fn profile_dirs() -> Result<ProfileDirs, ExitCode> {
    ProfileDirs::resolve().map_err(|err| {
        eprintln!("raptor: {err}");
        ExitCode::FAILURE
    })
}

/// A client of the running engine, starting it on demand.
fn engine(command: &str) -> Result<Client, ExitCode> {
    let dirs = profile_dirs()?;
    ensure_daemon(&ClientOptions::new(dirs, ClientKind::Cli)).map_err(|err| {
        eprintln!("{command}: {}", sanitize(&err.to_string()));
        ExitCode::FAILURE
    })
}

fn snapshot(client: &mut Client, command: &str) -> Result<Snapshot, ExitCode> {
    client
        .call(methods::ENGINE_SNAPSHOT, serde_json::json!({}))
        .map_err(|err| {
            eprintln!("{command}: {}", sanitize(&err.to_string()));
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
            println!("raptor daemon: {}", t("common.not-running", &[]));
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("raptor daemon stop: {}", sanitize(&err.to_string()));
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
            eprintln!("raptor daemon stop: {}", sanitize(&err.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// The message for a refused reserved command; `agent_key` names what an
/// agent may not do.
fn refusal_text(err: &ErrorObject, agent_key: &str) -> String {
    let reason = err
        .data
        .clone()
        .and_then(|d| serde_json::from_value::<RefusedData>(d).ok())
        .map(|d| d.reason);
    match reason {
        Some(RefusalReason::AgentAncestry | RefusalReason::SessionLeaderAgent) => t(agent_key, &[]),
        Some(RefusalReason::NoControllingTerminal) => t("common.refused-terminal", &[]),
        _ => t("common.refused-unverified", &[]),
    }
}

/// Asks on the terminal. Without a terminal there is nobody to ask: no.
fn confirm(question: &str) -> bool {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        eprintln!("raptor: {}", t("common.confirm-needs-terminal", &[]));
        return false;
    }
    eprint!("{question} ");
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

/// The folder a repo command names: the given one or the current one, made
/// absolute. Nothing is searched upwards: a folder inside a repo is not
/// that repo (US-GRP-001).
fn command_path(path: Option<PathBuf>) -> PathBuf {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    std::fs::canonicalize(&path)
        .or_else(|_| std::path::absolute(&path))
        .unwrap_or(path)
}

fn shown(path: &Path) -> String {
    sanitize(&path.display().to_string())
}

/// `raptor repo add`: a reserved command (US-GRP-001, BR-AUTH-001). The
/// engine decides who may run it and whether the path is a repo.
fn repo_add(path: Option<PathBuf>) -> ExitCode {
    const CMD: &str = "raptor repo add";
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = RepoAddParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, RepoAddResult>(methods::REPO_ADD, &params) {
        Ok(result) => {
            let key = match result.outcome {
                RepoAddOutcome::New => "repo.added",
                RepoAddOutcome::AlreadyObserved => "repo.already-observed",
                RepoAddOutcome::Reactivated => "repo.reactivated",
            };
            println!("{}", t(key, &[("path", &shown(&path))]));
            ExitCode::SUCCESS
        }
        Err(err) => repo_error(CMD, &path, err),
    }
}

/// `raptor repo retire`: a reserved command (US-GRP-001). The path names
/// the repo by its Git directory or any of its worktrees.
fn repo_retire(path: Option<PathBuf>) -> ExitCode {
    const CMD: &str = "raptor repo retire";
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    let Some(repo_id) = status::repo_at(&snapshot, &path) else {
        eprintln!(
            "{CMD}: {}",
            t("repo.not-observed", &[("path", &shown(&path))])
        );
        return ExitCode::FAILURE;
    };
    let params = RepoRetireParams { repo_id };
    match client.call::<_, RepoRetireResult>(methods::REPO_RETIRE, &params) {
        Ok(result) => {
            let key = if result.retired {
                "repo.retired"
            } else {
                "repo.not-observed"
            };
            println!("{}", t(key, &[("path", &shown(&path))]));
            ExitCode::SUCCESS
        }
        Err(err) => repo_error(CMD, &path, err),
    }
}

fn repo_error(command: &str, path: &Path, err: ClientError) -> ExitCode {
    let message = match err {
        ClientError::Rpc(err) if err.code == code::RESERVED_REFUSED => {
            refusal_text(&err, "repo.refused-agent")
        }
        ClientError::Rpc(err) if err.code == code::REPO_REJECTED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<RepoRejectedData>(d).ok())
                .map(|d| d.reason);
            let key = match reason {
                Some(RepoRejection::NotARepo) => "repo.not-a-repo",
                Some(RepoRejection::Untrusted) => "repo.untrusted",
                Some(RepoRejection::UnknownRepo) => "repo.unknown",
                Some(RepoRejection::Unreadable) | None => "repo.unreadable",
            };
            t(key, &[("path", &shown(path))])
        }
        other => sanitize(&other.to_string()),
    };
    eprintln!("{command}: {message}");
    ExitCode::FAILURE
}

/// `raptor status`: every observed repo and its worktrees (US-GRP-001).
/// Read-only; starts the engine on demand.
fn status(json: bool) -> ExitCode {
    const CMD: &str = "raptor status";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    if json {
        match serde_json::to_string_pretty(&status::json(&snapshot)) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!("{}", status::text(&snapshot));
    }
    ExitCode::SUCCESS
}

/// `raptor events` (US-GRP-002): the latest Git events of every observed
/// repo, oldest first.
fn events_command(json: bool, limit: u32) -> ExitCode {
    const CMD: &str = "raptor events";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    // A daemon of the same protocol but older than this binary keeps
    // running after an upgrade and has no history to serve.
    if !client
        .hello()
        .methods
        .iter()
        .any(|m| m == methods::EVENTS_HISTORY)
    {
        eprintln!("{CMD}: {}", t("events.restart-engine", &[]));
        return ExitCode::FAILURE;
    }
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    let limit = limit.clamp(1, gitraptor_api::messages::MAX_HISTORY_PAGE);
    let mut all: Vec<GitEventView> = Vec::new();
    for repo in &snapshot.repos {
        let params = EventsHistoryParams {
            repo_id: repo.repo_id.clone(),
            limit: Some(limit),
            ..EventsHistoryParams::default()
        };
        match client.call::<_, EventsHistoryResult>(methods::EVENTS_HISTORY, &params) {
            Ok(page) => all.extend(page.events),
            Err(err) => {
                eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                return ExitCode::FAILURE;
            }
        }
    }
    all.sort_by_key(|e| (e.observed_utc_ms, e.seq));
    let skip = all.len().saturating_sub(limit as usize);
    let all = &all[skip..];
    if json {
        match serde_json::to_string_pretty(&events::json(all)) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!("{}", events::text(all));
    }
    ExitCode::SUCCESS
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
