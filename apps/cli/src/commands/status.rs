//! `raptor status`: the observed repos (US-GRP-001) or what GitRaptor
//! consumes (US-GRP-017).

use std::process::ExitCode;

use gitraptor_api::discovery::CandidatesResult;
use gitraptor_api::messages::{ClientKind, SessionsListParams, SessionsListResult};
use gitraptor_api::methods;
use gitraptor_api::resources::ResourcesResult;
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::profile::{INDEX_FILE, read_only_repos};

use super::Global;
use crate::i18n::t;
use crate::{engine, error_text, offers, profile_dirs, resources, snapshot, status};

/// Show every observed repo and the state of its worktrees.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// Show what GitRaptor consumes instead: CPU, memory, watches and disk.
    /// Does not start the engine.
    #[arg(long)]
    resources: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        if self.resources {
            status_resources(self.json)
        } else {
            status(self.json)
        }
    }
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
            // US-GRP-020: discovered repos are not observed, so they cost
            // nothing; only their count is shown, when it can be read.
            let discovered = if client
                .hello()
                .methods
                .iter()
                .any(|m| m == methods::DISCOVERY_CANDIDATES)
            {
                match client.call::<_, CandidatesResult>(
                    methods::DISCOVERY_CANDIDATES,
                    serde_json::json!({}),
                ) {
                    Ok(found) => Some(found.candidates.len() as u64),
                    // Informational: the readings that worked still show.
                    Err(_) => None,
                }
            } else {
                None
            };
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
            resources::View::running(pid, result, repos).with_discovered(discovered)
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
