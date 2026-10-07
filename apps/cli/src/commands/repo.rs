//! `raptor repo`: the repos the engine observes (US-GRP-001).

use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::messages::{
    RepoAddOutcome, RepoAddParams, RepoAddResult, RepoRetireParams, RepoRetireResult,
};
use gitraptor_api::methods;

use super::Global;
use crate::i18n::t;
use crate::{command_path, engine, repo_error, shown, snapshot, status};

/// Add or retire the repos the engine observes (reserved to the developer).
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(subcommand)]
    action: RepoAction,
}

#[derive(clap::Subcommand)]
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

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            RepoAction::Add { path } => repo_add(path),
            RepoAction::Retire { path } => repo_retire(path),
        }
    }
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
    // The retirement takes the repo out of the MCP allowlist too (US-MCP-002).
    let was_enabled = super::mcp::is_enabled(&mut client, &repo_id) == Some(true);
    let params = RepoRetireParams { repo_id };
    match client.call::<_, RepoRetireResult>(methods::REPO_RETIRE, &params) {
        Ok(result) => {
            let key = if result.retired {
                "repo.retired"
            } else {
                "repo.not-observed"
            };
            println!("{}", t(key, &[("path", &shown(&path))]));
            if result.retired && was_enabled {
                println!("{}", t("mcp.retired-from-allowlist", &[("path", &shown(&path))]));
            }
            ExitCode::SUCCESS
        }
        Err(err) => repo_error(CMD, &path, err),
    }
}
