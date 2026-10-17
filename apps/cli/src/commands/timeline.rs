//! `raptor timeline`: what changed in the repo, when and who did it
//! (US-TMC-006). The engine assembles the timeline; this file asks for it
//! and prints it. Everything that comes from the repo (paths, branch and
//! agent names) is untrusted and printed sanitized (SEC-12).

use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{TimelineParams, TimelineResult};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;

use super::Global;
use crate::i18n::{Tr, t};
use crate::{engine, error_text, offers, shown, undo};

const CMD: &str = "raptor timeline";

/// What changed in the repo, when and who did it: Time Machine operations and Git events.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// Only the entries of this worktree (a path).
    #[arg(long, value_name = "PATH")]
    worktree: Option<PathBuf>,
    /// Only the last period, such as 30m, 2h or 1d (at most 30 days).
    #[arg(long, value_name = "DURATION")]
    since: Option<String>,
    /// Only what an agent did (its name or kind, such as claude-code), or `unattributed`.
    #[arg(long, value_name = "AGENT")]
    agent: Option<String>,
    /// How many entries, the most recent ones (1 to 200, default 50).
    #[arg(long)]
    limit: Option<u32>,
    /// Show every entry, with the actor and its origin, the full level and id, and the files.
    #[arg(long)]
    verbose: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        let cwd = std::env::current_dir().and_then(gitraptor_cli::paths::canonicalize).ok();
        let Some(anchor) = cwd.as_deref().and_then(undo::worktree_root) else {
            eprintln!("{CMD}: {}", t("timeline.not-in-worktree", &[]));
            return ExitCode::FAILURE;
        };
        let mut client = match engine(CMD) {
            Ok(client) => client,
            Err(code) => return code,
        };
        undo::notices::show_pending(&mut client, &anchor, CMD);
        if !offers(&client, methods::TM_TIMELINE) {
            eprintln!("{CMD}: {}", t("timeline.restart-engine", &[]));
            return ExitCode::FAILURE;
        }
        let filtered = self.worktree.is_some() || self.since.is_some() || self.agent.is_some();
        // The 24 h window and the short ids are for text; `--json` asks as it always did.
        let window_applied = !self.json && self.since.is_none();
        let base = TimelineParams {
            short_ids: !self.json,
            worktree: Some(anchor.to_string_lossy().into_owned()),
            only_worktree: self
                .worktree
                .map(|w| crate::support::command_path(Some(w)).to_string_lossy().into_owned()),
            since: compact::default_since(self.json, self.since),
            agent: self.agent,
            limit: self.limit,
        };
        let answer: Result<serde_json::Value, ClientError> =
            client.call(methods::TM_TIMELINE, &base);
        let value = match answer {
            Ok(value) => value,
            Err(err) => {
                eprintln!("{CMD}: {}", failure_text(err, &anchor));
                return ExitCode::FAILURE;
            }
        };
        if self.json {
            println!("{value}");
            return ExitCode::SUCCESS;
        }
        let result = match serde_json::from_value::<TimelineResult>(value) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                return ExitCode::FAILURE;
            }
        };
        let tr = Tr::current();
        if result.entries.is_empty() && result.unavailable.is_empty() {
            // An empty default window is not "no activity": ask once whether anything is older.
            let outside = window_applied && {
                let wider = TimelineParams {
                    since: None,
                    limit: Some(1),
                    ..base
                };
                client
                    .call::<_, TimelineResult>(methods::TM_TIMELINE, &wider)
                    .is_ok_and(|older| !older.entries.is_empty())
            };
            let text = if outside {
                compact::empty_text(true, true, tr)
            } else if filtered {
                tr.t("timeline.no-match", &[])
            } else {
                compact::empty_text(window_applied, false, tr)
            };
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        // The offset of the newest entry is the one the engine saw the clock at.
        let offset = result.entries.last().map_or(0, |e| e.utc_offset_s);
        let opts = compact::CompactOpts {
            verbose: self.verbose,
            today: compact::LocalDay::of(now_ms(), offset),
            window_applied,
        };
        print!("{}", compact::render_compact(&result, &opts, tr));
        ExitCode::SUCCESS
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn failure_text(err: ClientError, anchor: &std::path::Path) -> String {
    match err {
        // An engine older than this raptor: it declares the method without serving it, or refuses
        // a field it does not know. The remedy is the same as for a method it does not offer.
        ClientError::Rpc(err) if is_old_engine(&err) => t("timeline.restart-engine", &[]),
        ClientError::Rpc(err) if err.code == code::SCOPE_REFUSED => {
            t("timeline.not-observed", &[("worktree", &shown(anchor))])
        }
        ClientError::Rpc(err) if err.code == code::INVALID_PARAMS => {
            t("timeline.invalid-params", &[])
        }
        other => error_text(other),
    }
}

fn is_old_engine(err: &gitraptor_api::rpc::ErrorObject) -> bool {
    err.code == code::NOT_IMPLEMENTED
        || (err.code == code::INVALID_PARAMS && err.message.contains("unknown field"))
}

#[path = "timeline_compact.rs"]
pub(super) mod compact;

#[cfg(test)]
#[path = "timeline_compact_tests.rs"]
mod compact_tests;

#[cfg(test)]
#[path = "timeline_inferred_tests.rs"]
mod inferred_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use compact::{CompactOpts, LocalDay, render_compact};
    use gitraptor_api::messages::GitEventKind;
    use gitraptor_api::timemachine::{
        Attribution, ChangedFiles, EntryOrigin, Protection, ProtectionLevel, TimelineChannel,
        TimelineEntry, TimelineSource,
    };
    use gitraptor_api::untrusted::Untrusted;
    use gitraptor_api::{Actor, AgentKind, AgentOrigin};

    fn untrusted(text: &str) -> Untrusted {
        serde_json::from_value(serde_json::json!({ "untrusted": text })).unwrap()
    }

    fn entry(actor: Actor, attribution: Attribution, files: ChangedFiles) -> TimelineEntry {
        TimelineEntry {
            short_id_len: None,
            id: "event:1".into(),
            origin: EntryOrigin::GitEvent {
                seq: 1,
                kind: GitEventKind::Commit,
                branch: None,
            },
            occurred_utc_ms: 0,
            utc_offset_s: 0,
            worktrees: vec![untrusted("/repo/wt-a")],
            actor,
            inferred: None,
            attribution,
            protection: Protection {
                level: ProtectionLevel::None,
                snapshot_id: None,
            },
            files,
        }
    }

    fn result(entries: Vec<TimelineEntry>, unavailable: Vec<TimelineSource>) -> TimelineResult {
        TimelineResult {
            repo_id: "r".into(),
            entries,
            truncated: false,
            unavailable,
            detection_available: true,
        }
    }

    fn files(paths: &[&str], total: u32) -> ChangedFiles {
        ChangedFiles::Available {
            paths: paths.iter().map(|p| untrusted(p)).collect(),
            total,
            first_parent: false,
        }
    }

    fn render(result: &TimelineResult, verbose: bool) -> String {
        let opts = CompactOpts {
            verbose,
            today: LocalDay(0),
            window_applied: false,
        };
        render_compact(result, &opts, Tr::current())
    }

    #[test]
    fn every_key_has_both_languages() {
        for key in [
            "timeline.empty",
            "timeline.no-match",
            "timeline.incomplete",
            "timeline.truncated",
            "timeline.actor-unavailable",
            "timeline.files",
            "timeline.files-more",
            "timeline.files-first-parent",
            "timeline.level.prior",
            "timeline.level.observation",
            "timeline.level.none",
            "timeline.level.manual",
            "timeline.level.short.prior",
            "timeline.level.short.observation",
            "timeline.level.short.manual",
            "timeline.level.short.none",
            "timeline.header.window",
            "timeline.header.window-summarized",
            "timeline.header.all",
            "timeline.header.all-summarized",
            "timeline.entries.one",
            "timeline.entries.many",
            "timeline.day.today",
            "timeline.day.yesterday",
            "timeline.action.restore",
            "timeline.action.none",
            "timeline.summary.branch",
            "timeline.summary.branches",
            "timeline.summary.resets",
            "timeline.actor.several",
            "timeline.actor.you-or-other",
            "timeline.legend.you-or-other",
            "timeline.hint.restore",
            "timeline.window-empty",
            "restore.ambiguous",
            "restore.candidate",
            "restore.candidates-more",
            "restore.too-short",
            "timeline.manual",
            "timeline.op.protected",
            "timeline.op.protected-unnamed",
            "timeline.op.undo",
            "timeline.op.redo",
            "timeline.op.restore",
            "timeline.state.applying",
            "timeline.state.interrupted",
            "timeline.acted-on",
            "timeline.acted.operation",
            "timeline.acted.event",
            "timeline.acted.snapshot",
            "timeline.source.operations",
            "timeline.source.events",
            "timeline.branch-unknown",
            "timeline.not-in-worktree",
            "timeline.not-observed",
            "timeline.invalid-params",
            "timeline.restart-engine",
        ] {
            assert!(crate::i18n::has_key(key), "missing {key}");
        }
    }

    #[test]
    fn plus_k_more_is_shown_with_the_real_total() {
        let shown = render(
            &result(
                vec![entry(Actor::Unattributed, Attribution::Current, files(&["a", "b"], 25))],
                vec![],
            ),
            true,
        );
        assert!(shown.contains(&t("timeline.files-more", &[("count", &23)])), "{shown}");
        assert!(shown.contains("25"), "{shown}");
    }

    #[test]
    fn an_unavailable_source_is_never_called_no_activity() {
        let shown = render(&result(vec![], vec![TimelineSource::Events]), false);
        assert!(!shown.contains(&t("timeline.empty", &[])), "{shown}");
        assert!(
            shown.contains(&t(compact_source(TimelineSource::Events), &[])),
            "{shown}"
        );
    }

    fn compact_source(source: TimelineSource) -> &'static str {
        match source {
            TimelineSource::Operations => "timeline.source.operations",
            TimelineSource::Events => "timeline.source.events",
        }
    }

    #[test]
    fn unattributed_reads_you_or_other_or_not_available_never_human() {
        let e = entry(Actor::Unattributed, Attribution::Current, files(&[], 0));
        let shown = render(&result(vec![e.clone()], vec![]), false);
        assert!(shown.contains(&t("timeline.actor.you-or-other", &[])), "{shown}");
        let lower = shown.to_lowercase();
        assert!(!lower.contains("human") && !lower.contains("humano"), "{shown}");
        let mut blind = result(vec![e], vec![]);
        blind.detection_available = false;
        let shown = render(&blind, true);
        assert!(shown.contains(&t("timeline.actor-unavailable", &[])), "{shown}");
    }

    #[test]
    fn an_undo_entry_shows_the_recorded_requester_not_the_current_actor() {
        let actor = Actor::Agent {
            kind: AgentKind::Other,
            name: Some(serde_json::from_value(serde_json::json!({ "untrusted": "Codex" })).unwrap()),
            origin: AgentOrigin::Registered,
        };
        let e = entry(actor, Attribution::Recorded, files(&[], 0));
        let shown = render(&result(vec![e], vec![]), false);
        assert!(shown.contains("Codex"), "{shown}");
        assert!(!shown.contains(&t("actor.other", &[])), "{shown}");
    }

    #[test]
    fn unavailable_files_are_not_shown_and_not_a_zero() {
        let e = entry(Actor::Unattributed, Attribution::Current, ChangedFiles::Unavailable);
        let shown = render(&result(vec![e], vec![]), true);
        assert!(!shown.contains("files"), "{shown}");
    }

    #[test]
    fn a_merge_says_its_paths_are_against_the_first_parent() {
        let merged = ChangedFiles::Available {
            paths: vec![untrusted("a")],
            total: 1,
            first_parent: true,
        };
        let e = entry(Actor::Unattributed, Attribution::Current, merged);
        let shown = render(&result(vec![e], vec![]), true);
        assert!(shown.contains(&t("timeline.files-first-parent", &[])), "{shown}");
    }

    #[test]
    fn paths_with_escape_sequences_are_printed_sanitized() {
        let e = entry(
            Actor::Unattributed,
            Attribution::Current,
            files(&["a\x1b[31mred\nb"], 1),
        );
        let shown = render(&result(vec![e], vec![]), true);
        assert!(!shown.contains('\x1b'), "{shown:?}");
        assert!(!shown.contains("red\nb"), "{shown:?}");
    }

    #[test]
    fn a_manual_snapshot_row_is_sanitized() {
        use gitraptor_api::untrusted::UntrustedName;
        let label: UntrustedName =
            serde_json::from_value(serde_json::json!({ "untrusted": "done\x1b[31m red\u{202e}\nx" }))
                .unwrap();
        let mut e = entry(Actor::Unattributed, Attribution::Recorded, files(&[], 0));
        e.origin = EntryOrigin::ManualSnapshot {
            snapshot_id: "s1".into(),
            label,
            channel: TimelineChannel::Mcp,
        };
        e.protection.level = ProtectionLevel::Manual;
        let shown = render(&result(vec![e], vec![]), false);
        assert!(!shown.contains('\x1b'), "{shown:?}");
        assert!(!shown.contains('\u{202e}'), "{shown:?}");
        assert!(!shown.contains("red\nx"), "{shown:?}");
        assert!(shown.contains("mcp"), "{shown:?}");
        assert!(shown.contains(&t("timeline.level.short.manual", &[])), "{shown:?}");
    }

    #[test]
    fn an_old_engine_gets_the_restart_hint() {
        use gitraptor_api::rpc::ErrorObject;
        let hint = t("timeline.restart-engine", &[]);
        let anchor = std::path::Path::new("/repo");
        let not_implemented = ClientError::Rpc(ErrorObject::new(code::NOT_IMPLEMENTED, "x"));
        assert_eq!(failure_text(not_implemented, anchor), hint);
        let unknown = ClientError::Rpc(ErrorObject::new(
            code::INVALID_PARAMS,
            "invalid params: unknown field `only_worktree`",
        ));
        assert_eq!(failure_text(unknown, anchor), hint);
        // A bad value on a current engine is still the filters' message.
        let bad = ClientError::Rpc(ErrorObject::new(code::INVALID_PARAMS, "since: invalid"));
        assert_eq!(
            failure_text(bad, anchor),
            t("timeline.invalid-params", &[])
        );
    }

    /// The levels read the same in both languages: verbose has the whole text, the compact view
    /// the short word.
    #[test]
    fn the_protection_levels_read_right_in_es_and_en() {
        use crate::i18n::text_in;
        let want = [
            ("timeline.level.prior", "prior snapshot", "snapshot previo"),
            ("timeline.level.observation", "captured by observation", "capturado por observación"),
            ("timeline.level.none", "unprotected", "sin protección"),
            ("timeline.level.manual", "manual point", "punto manual"),
            ("timeline.level.short.prior", "prior", "previo"),
            ("timeline.level.short.observation", "observed", "observado"),
            ("timeline.level.short.none", "none", "ninguno"),
            ("timeline.level.short.manual", "manual", "manual"),
        ];
        for (key, en, es) in want {
            assert_eq!(text_in(false, key), Some(en), "{key}");
            assert_eq!(text_in(true, key), Some(es), "{key}");
        }
    }

    #[test]
    fn an_entry_with_a_saved_state_shows_the_id_restore_takes() {
        let mut e = entry(Actor::Unattributed, Attribution::Current, files(&[], 0));
        let none = render(&result(vec![e.clone()], vec![]), false);
        assert!(!none.contains("raptor restore"), "{none}");
        e.protection.snapshot_id = Some("0a1b2c3d-0000-4000-8000-000000000000".into());
        let shown = render(&result(vec![e], vec![]), false);
        assert!(shown.contains("restore 0a1b2c3d\n") || shown.contains("restore 0a1b2c3d "), "{shown}");
        assert!(shown.contains("raptor restore <id>"), "{shown}");
    }
}
