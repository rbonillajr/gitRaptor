//! `raptor undo`: the Time Machine's undo (US-TMC-002).

use std::process::ExitCode;

use super::Global;
use crate::undo;

/// Undo the last operation of the worktree you are in; the state right before the undo is saved.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// Undo everything done in this worktree in the last DURATION (e.g. 20m, 2h, 1d; up to 30
    /// days), as one undo.
    #[arg(long, value_name = "DURATION")]
    since: Option<String>,
    /// Undo only the work of one agent in this worktree: a session id from
    /// `raptor sessions --json` or an agent name such as claude-code. With --since, only that
    /// period; without it, the last 30 days.
    #[arg(long, value_name = "ID")]
    agent: Option<String>,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        undo::run(self.json, self.since.as_deref(), self.agent.as_deref())
    }
}
