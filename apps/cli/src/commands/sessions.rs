//! `raptor sessions`: the agent sessions of the observed repos (US-GRP-007).

use std::process::ExitCode;

use gitraptor_api::messages::{SessionsListParams, SessionsListResult};
use gitraptor_api::methods;
use gitraptor_api::untrusted::sanitize;

use super::Global;
use crate::i18n::t;
use crate::{engine, offers, sessions};

/// Show the agent sessions of the observed repos: the present ones and
/// the latest ended one of each worktree.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// Also every ended session.
    #[arg(long)]
    all: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        sessions_command(self.json, self.all)
    }
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
