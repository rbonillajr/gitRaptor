//! `raptor predict-worker`: the conflict predictor's worker process, launched only by the daemon
//! with a fixed argv and an environment of its own. Not a user surface: hidden from `--help`.
//! It reads jobs on stdin until EOF and never touches the profile.

use std::process::ExitCode;

use super::Global;

/// The predictor's worker (hidden: the daemon launches it).
#[derive(clap::Args)]
pub(crate) struct Cmd {}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stdout().lock();
        match gitraptor_core::predict::worker::serve::serve(&mut input, &mut output) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }
}
