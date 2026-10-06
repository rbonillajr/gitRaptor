//! `raptor guard`: the Guardrails hook layer (US-GRD-001).

use std::path::PathBuf;
use std::process::ExitCode;

use super::Global;
use crate::guard;

/// Protect a repo with GitRaptor hooks (Guardrails).
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(subcommand)]
    action: GuardAction,
}

#[derive(clap::Subcommand)]
enum GuardAction {
    /// Protect a repo: explains what is installed and asks for your permission (reserved to the
    /// developer). Denies force-push and deleting the base branch, even with plain Git.
    Install {
        /// The repo's Git directory or any of its worktrees; defaults to the current folder.
        path: Option<PathBuf>,
        /// Grant the permission without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Show the protection status of a repo.
    Status {
        /// The repo's Git directory or any of its worktrees; defaults to the current folder.
        path: Option<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            GuardAction::Install { path, yes } => guard::install(path, yes),
            GuardAction::Status { path, json } => guard::status(path, json),
        }
    }
}
