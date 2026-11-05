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
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        undo::run(self.json, self.since.as_deref())
    }
}
