//! `raptor sessions`: the agent sessions of the observed repos (US-GRP-007).

use std::process::ExitCode;

use gitraptor_api::messages::{SessionsListParams, SessionsListResult};
use gitraptor_api::methods;
use gitraptor_api::untrusted::sanitize;

use super::Global;
use crate::i18n::t;
use crate::{json_out, offers, sessions};

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
    let mut client = match json_out::engine(CMD, json) {
        Ok(client) => client,
        Err(code) => return code,
    };
    if json && let Err(code) = json_out::require(&client, methods::SESSIONS_LIST) {
        return code;
    }
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
        Err(err) if json => return json_out::fail_client(&client, &err),
        Err(err) => {
            eprintln!("{CMD}: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    if json {
        return json_out::ok_raw(&sessions::json(&list.sessions, list.detection_available));
    } else {
        print!(
            "{}",
            sessions::text(&list.sessions, list.detection_available)
        );
    }
    ExitCode::SUCCESS
}
