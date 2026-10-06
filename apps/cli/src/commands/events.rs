//! `raptor events`: the Git events of the observed repos (US-GRP-002).

use std::process::ExitCode;

use gitraptor_api::messages::{EventsHistoryParams, EventsHistoryResult, GitEventView};
use gitraptor_api::methods;
use gitraptor_api::untrusted::sanitize;

use super::Global;
use crate::i18n::t;
use crate::{engine, events, offers, snapshot};

/// Show the latest Git events of the observed repos, with their time and actor.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// How many events, the most recent ones (at most 200).
    #[arg(long, default_value_t = 50)]
    limit: u32,
    /// Every recorded event, page by page (ignores --limit).
    #[arg(long)]
    all: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        events_command(self.json, self.limit, self.all)
    }
}

/// `raptor events` (US-GRP-002): the latest Git events of every observed
/// repo, oldest first.
fn events_command(json: bool, limit: u32, all: bool) -> ExitCode {
    const CMD: &str = "raptor events";
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    // A daemon of the same protocol but older than this binary keeps
    // running after an upgrade and has no history to serve.
    if !offers(&client, methods::EVENTS_HISTORY) {
        eprintln!("{CMD}: {}", t("events.restart-engine", &[]));
        return ExitCode::FAILURE;
    }
    let snapshot = match snapshot(&mut client, CMD) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };
    let all_pages = all;
    let limit = if all_pages {
        gitraptor_api::messages::MAX_HISTORY_PAGE
    } else {
        limit.clamp(1, gitraptor_api::messages::MAX_HISTORY_PAGE)
    };
    let mut events: Vec<GitEventView> = Vec::new();
    for repo in &snapshot.repos {
        // With `--all`, every page from the first event (the dogfooding
        // review of SPIKE-GRP-001 exports whole days).
        let mut after_seq = all_pages.then_some(0);
        loop {
            let params = EventsHistoryParams {
                repo_id: repo.repo_id.clone(),
                limit: Some(limit),
                after_seq,
                ..EventsHistoryParams::default()
            };
            match client.call::<_, EventsHistoryResult>(methods::EVENTS_HISTORY, &params) {
                Ok(page) => {
                    let last = page.events.last().map(|e| e.seq);
                    let full = page.events.len() == limit as usize;
                    events.extend(page.events);
                    match (all_pages && full, last) {
                        (true, Some(seq)) => after_seq = Some(seq),
                        _ => break,
                    }
                }
                Err(err) => {
                    eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let mut all = events;
    all.sort_by_key(|e| (e.observed_utc_ms, e.seq));
    let skip = if all_pages {
        0
    } else {
        all.len().saturating_sub(limit as usize)
    };
    let all = &all[skip..];
    if json {
        match serde_json::to_string_pretty(&events::json(all)) {
            Ok(text) => println!("{text}"),
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        print!("{}", events::text(all));
    }
    ExitCode::SUCCESS
}
