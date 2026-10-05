mod events;
mod i18n;
mod mcp;
mod resources;
mod sessions;
mod status;

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use gitraptor_api::messages::{
    ClientKind, EventsHistoryParams, EventsHistoryResult, GitEventView, RefusalReason, RefusedData,
    RepoAddOutcome, RepoAddParams, RepoAddResult, RepoRejectedData, RepoRejection,
    RepoRetireParams, RepoRetireResult, SessionsListParams, SessionsListResult, Snapshot,
};
use gitraptor_api::methods;
use gitraptor_api::resources::ResourcesResult;
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_api::timemachine::{PriorFailedData, PriorFailure};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::daemon::{self, DaemonConfig, DaemonError, EXIT_ALREADY_RUNNING};
use gitraptor_core::profile::{INDEX_FILE, ProfileDirs, read_only_repos};

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
        /// Show what GitRaptor consumes instead: CPU, memory, watches and disk.
        /// Does not start the engine.
        #[arg(long)]
        resources: bool,
    },
    /// Show the latest Git events of the observed repos, with their time and actor.
    Events {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// How many events, the most recent ones (at most 200).
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Every recorded event, page by page (ignores --limit).
        #[arg(long)]
        all: bool,
    },
    /// Show the agent sessions of the observed repos: the present ones and
    /// the latest ended one of each worktree.
    Sessions {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Also every ended session.
        #[arg(long)]
        all: bool,
    },
    /// Register or remove the GitRaptor MCP server in your coding agent.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Register the gitraptor MCP server in Claude Code, for every project (user scope).
    Install {
        /// The coding agent. Only claude-code is supported for now.
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },
    /// Remove the gitraptor MCP server from Claude Code, only if it is this GitRaptor's.
    Uninstall {
        /// The coding agent. Only claude-code is supported for now.
        #[arg(long, default_value = "claude-code")]
        agent: String,
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
        Some(Command::Status {
            json,
            resources: false,
        }) => status(json),
        Some(Command::Status {
            json,
            resources: true,
        }) => status_resources(json),
        Some(Command::Events { json, limit, all }) => events_command(json, limit, all),
        Some(Command::Sessions { json, all }) => sessions_command(json, all),
        Some(Command::Mcp {
            action: McpAction::Install { agent },
        }) => mcp::install(&agent),
        Some(Command::Mcp {
            action: McpAction::Uninstall { agent },
        }) => mcp::uninstall(&agent),
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

/// The message for any other failed call. A failed prior snapshot says, in
/// the user's language, that the operation did not run and why (US-TMC-001).
fn error_text(err: ClientError) -> String {
    match err {
        ClientError::Rpc(err) if err.code == code::PRIOR_SNAPSHOT_FAILED => t(
            prior_failure_key(
                err.data
                    .and_then(|d| serde_json::from_value::<PriorFailedData>(d).ok())
                    .map(|d| d.reason),
            ),
            &[],
        ),
        other => sanitize(&other.to_string()),
    }
}

fn prior_failure_key(reason: Option<PriorFailure>) -> &'static str {
    match reason {
        Some(PriorFailure::NoSpace) => "prior.no-space",
        Some(PriorFailure::StoreUnavailable) => "prior.store-unavailable",
        Some(PriorFailure::Timeout) => "prior.timeout",
        Some(PriorFailure::DaemonStopping) => "prior.daemon-stopping",
        Some(PriorFailure::CaptureFailed) | None => "prior.capture-failed",
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
        other => error_text(other),
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
    // Sessions (US-GRP-007), when the engine offers them.
    let mut sessions = status::SessionsInfo::default();
    if offers(&client, methods::SESSIONS_LIST) {
        match client
            .call::<_, SessionsListResult>(methods::SESSIONS_LIST, &SessionsListParams::default())
        {
            Ok(list) => {
                sessions.available = Some(list.detection_available);
                sessions.sessions = list.sessions;
            }
            Err(err) => {
                eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                return ExitCode::FAILURE;
            }
        }
    }
    if json {
        match serde_json::to_string_pretty(&status::json(&snapshot, &sessions)) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!("{}", status::text(&snapshot, &sessions));
    }
    ExitCode::SUCCESS
}

/// Whether the running engine offers `method`: a daemon of the same
/// protocol but older than this binary keeps running after an upgrade.
fn offers(client: &Client, method: &str) -> bool {
    client.hello().methods.iter().any(|m| m == method)
}

/// `raptor sessions` (US-GRP-007): the agent sessions of every observed
/// repo, oldest first.
fn sessions_command(json: bool, all: bool) -> ExitCode {
    const CMD: &str = "raptor sessions";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    if !offers(&client, methods::SESSIONS_LIST) {
        eprintln!("{CMD}: {}", t("sessions.restart-engine", &[]));
        return ExitCode::FAILURE;
    }
    let params = SessionsListParams {
        include_ended: all,
        ..SessionsListParams::default()
    };
    let list = match client.call::<_, SessionsListResult>(methods::SESSIONS_LIST, &params) {
        Ok(list) => list,
        Err(err) => {
            eprintln!("{CMD}: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    if json {
        match serde_json::to_string_pretty(&sessions::json(
            &list.sessions,
            list.detection_available,
        )) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!(
            "{}",
            sessions::text(&list.sessions, list.detection_available)
        );
    }
    ExitCode::SUCCESS
}

/// `raptor status --resources` (US-GRP-017): what GitRaptor consumes,
/// each value against its target. Never starts the engine: when it is not
/// running, only the disk is shown, read from the profile. Being over a
/// target does not change the exit code.
fn status_resources(json: bool) -> ExitCode {
    const CMD: &str = "raptor status --resources";
    let dirs = match profile_dirs() {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    let view = match Client::connect(&dirs, ClientKind::Cli, gitraptor_api::PROTOCOL_VERSION) {
        Ok(mut client) => {
            // A daemon of the same protocol but older than this binary.
            if !client
                .hello()
                .methods
                .iter()
                .any(|m| m == methods::ENGINE_RESOURCES)
            {
                eprintln!("{CMD}: {}", t("res.restart-engine", &[]));
                return ExitCode::FAILURE;
            }
            let pid = client.hello().daemon_pid;
            let result: ResourcesResult =
                match client.call(methods::ENGINE_RESOURCES, serde_json::json!({})) {
                    Ok(result) => result,
                    Err(err) => {
                        eprintln!("{CMD}: {}", error_text(err));
                        return ExitCode::FAILURE;
                    }
                };
            let snapshot = match snapshot(&mut client, CMD) {
                Ok(snapshot) => snapshot,
                Err(code) => return code,
            };
            let repos = snapshot
                .repos
                .iter()
                .map(|r| (r.repo_id.clone(), r.path.raw().to_owned()))
                .collect();
            resources::View::running(pid, result, repos)
        }
        Err(ClientError::NotRunning) => {
            let disk = gitraptor_core::resources::disk::measure(
                &dirs,
                gitraptor_core::resources::DiskLimits::default(),
            );
            let repos = read_only_repos(&dirs.data.join(INDEX_FILE))
                .unwrap_or_default()
                .into_iter()
                .map(|(id, path)| (id, path.to_string_lossy().into_owned()))
                .collect();
            resources::View::stopped(
                disk,
                gitraptor_core::resources::ResourceConfig::default().targets,
                repos,
            )
        }
        Err(err) => {
            eprintln!("{CMD}: {}", error_text(err));
            return ExitCode::FAILURE;
        }
    };
    if json {
        match serde_json::to_string_pretty(&resources::json(&view)) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!("{}", resources::text(&view));
    }
    ExitCode::SUCCESS
}

/// `raptor events` (US-GRP-002): the latest Git events of every observed
/// repo, oldest first.
fn events_command(json: bool, limit: u32, all: bool) -> ExitCode {
    const CMD: &str = "raptor events";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    // A daemon of the same protocol but older than this binary keeps
    // running after an upgrade and has no history to serve.
    if !offers(&client, methods::EVENTS_HISTORY) {
        eprintln!("{CMD}: {}", t("events.restart-engine", &[]));
        return ExitCode::FAILURE;
    }
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    let all_pages = all;
    let limit = if all_pages {
        gitraptor_api::messages::MAX_HISTORY_PAGE
    } else {
        limit.clamp(1, gitraptor_api::messages::MAX_HISTORY_PAGE)
    };
    let mut events: Vec<GitEventView> = Vec::new();
    for repo in &snapshot.repos {
        // With `--all`, every page from the first event (the dogfooding
        // review of SPIKE-GRP-001 exports whole days).
        let mut after_seq = all_pages.then_some(0);
        loop {
            let params = EventsHistoryParams {
                repo_id: repo.repo_id.clone(),
                limit: Some(limit),
                after_seq,
                ..EventsHistoryParams::default()
            };
            match client.call::<_, EventsHistoryResult>(methods::EVENTS_HISTORY, &params) {
                Ok(page) => {
                    let last = page.events.last().map(|e| e.seq);
                    let full = page.events.len() == limit as usize;
                    events.extend(page.events);
                    match (all_pages && full, last) {
                        (true, Some(seq)) => after_seq = Some(seq),
                        _ => break,
                    }
                }
                Err(err) => {
                    eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let mut all = events;
    all.sort_by_key(|e| (e.observed_utc_ms, e.seq));
    let skip = if all_pages {
        0
    } else {
        all.len().saturating_sub(limit as usize)
    };
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

    /// Every reason of a failed prior snapshot has its message (en/es).
    #[test]
    fn every_prior_failure_has_a_message() {
        let reasons = [
            PriorFailure::NoSpace,
            PriorFailure::StoreUnavailable,
            PriorFailure::Timeout,
            PriorFailure::DaemonStopping,
            PriorFailure::CaptureFailed,
        ];
        for reason in reasons.into_iter().map(Some).chain([None]) {
            let key = prior_failure_key(reason);
            assert!(i18n::has_key(key), "{key}");
        }
        let err = ClientError::Rpc(
            ErrorObject::new(code::PRIOR_SNAPSHOT_FAILED, "no space").with_data(PriorFailedData {
                reason: PriorFailure::NoSpace,
                operation_id: None,
            }),
        );
        assert_ne!(error_text(err), "prior.no-space");
    }
}
