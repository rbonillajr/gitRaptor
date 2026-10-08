//! `raptor restore`: return the worktree to a point of the timeline (US-TMC-009).

use std::process::ExitCode;

use super::Global;

/// Return the worktree you are in to a point of the timeline; the state right before is saved.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// The id of the point, as `raptor timeline` shows it.
    snapshot_id: String,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        let _ = (&self.snapshot_id, self.json);
        eprintln!("raptor restore is not implemented yet");
        ExitCode::FAILURE
    }
}
