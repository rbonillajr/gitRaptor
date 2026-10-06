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
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        undo::run(self.json)
    }
}
