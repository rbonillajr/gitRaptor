//! `raptor redo`: the Time Machine's redo (US-TMC-003).

use std::process::ExitCode;

use super::Global;
use crate::undo;

/// Redo the last undo of the worktree you are in; the state right before the redo is saved.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        undo::redo::run(self.json)
    }
}
