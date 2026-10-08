//! `raptor timeline`: what changed in the repo, when and who did it
//! (US-TMC-006). The engine assembles the timeline; this file asks for it
//! and prints it. Everything that comes from the repo (paths, branch and
//! agent names) is untrusted and printed sanitized (SEC-12).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::messages::GitEventKind;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    ActedOn, ChangedFiles, EntryOrigin, ProtectionLevel, TimelineChannel, TimelineEntry,
    TimelineOperationKind, TimelineOperationState, TimelineParams, TimelineResult, TimelineSource,
};
use gitraptor_api::untrusted::sanitize;
use gitraptor_api::{Actor, AgentKind, AgentOrigin};
use gitraptor_core::client::ClientError;

use super::Global;
use crate::i18n::t;
use crate::{engine, error_text, events, offers, shown, undo};

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
        if !offers(&client, methods::TM_TIMELINE) {
            eprintln!("{CMD}: {}", t("timeline.restart-engine", &[]));
            return ExitCode::FAILURE;
        }
        let filtered = self.worktree.is_some() || self.since.is_some() || self.agent.is_some();
        let params = TimelineParams {
            worktree: Some(anchor.to_string_lossy().into_owned()),
            only_worktree: self
                .worktree
                .map(|w| crate::support::command_path(Some(w)).to_string_lossy().into_owned()),
            since: self.since,
            agent: self.agent,
            limit: self.limit,
        };
        let answer: Result<serde_json::Value, ClientError> =
            client.call(methods::TM_TIMELINE, &params);
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
        match serde_json::from_value::<TimelineResult>(value) {
            Ok(result) => {
                print!("{}", render(&result, filtered));
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                ExitCode::FAILURE
            }
        }
    }
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

/// The text output: one block per entry, oldest first, then the notices.
fn render(result: &TimelineResult, filtered: bool) -> String {
    let mut out = String::new();
    // A source that could not be read is never "no activity".
    let degraded = !result.unavailable.is_empty();
    if result.entries.is_empty() && !degraded {
        let key = if filtered {
            "timeline.no-match"
        } else {
            "timeline.empty"
        };
        let _ = writeln!(out, "{}", t(key, &[]));
    }
    if result.truncated {
        let _ = writeln!(out, "{}", t("timeline.truncated", &[]));
    }
    for entry in &result.entries {
        entry_text(&mut out, entry, result.detection_available);
    }
    if degraded {
        let sources: Vec<String> = result
            .unavailable
            .iter()
            .map(|s| t(source_key(*s), &[]))
            .collect();
        let _ = writeln!(
            out,
            "{}",
            t("timeline.incomplete", &[("sources", &sources.join(", "))])
        );
    }
    out
}

fn source_key(source: TimelineSource) -> &'static str {
    match source {
        TimelineSource::Operations => "timeline.source.operations",
        TimelineSource::Events => "timeline.source.events",
    }
}

fn entry_text(out: &mut String, entry: &TimelineEntry, detection_available: bool) {
    let worktrees: Vec<String> = entry
        .worktrees
        .iter()
        .map(|w| worktree_name(&w.sanitized()))
        .collect();
    let _ = writeln!(
        out,
        "{}",
        t(
            "timeline.line",
            &[
                ("time", &events::local_time(entry.occurred_utc_ms, entry.utc_offset_s)),
                ("worktree", &worktrees.join(", ")),
                ("what", &what(&entry.origin)),
                ("actor", &actor_text(entry, detection_available)),
                ("protection", &t(protection_key(entry.protection.level), &[])),
            ],
        )
    );
    // The id `raptor restore` takes: only an entry that saved a state has one.
    if let Some(id) = &entry.protection.snapshot_id {
        let _ = writeln!(out, "    {}", t("timeline.point", &[("id", &sanitize(id))]));
    }
    if let EntryOrigin::Operation { acted_on, .. } = &entry.origin
        && !acted_on.is_empty()
    {
        let targets: Vec<String> = acted_on.iter().map(acted_text).collect();
        let _ = writeln!(
            out,
            "    {}",
            t("timeline.acted-on", &[("targets", &targets.join(", "))])
        );
    }
    let _ = writeln!(out, "    {}", files_text(&entry.files));
}

/// The last component of a worktree root; the whole text when it has none.
fn worktree_name(root: &str) -> String {
    let name = root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next();
    name.filter(|n| !n.is_empty()).unwrap_or(root).to_owned()
}

fn what(origin: &EntryOrigin) -> String {
    match origin {
        EntryOrigin::ManualSnapshot { label, channel, .. } => t(
            "timeline.manual",
            &[
                ("label", &label.sanitized()),
                ("channel", &channel_text(*channel)),
            ],
        ),
        EntryOrigin::Operation {
            kind,
            subtype,
            state,
            ..
        } => {
            let mut text = match (kind, subtype) {
                (TimelineOperationKind::Protected, Some(subtype)) => t(
                    "timeline.op.protected",
                    &[("subtype", &subtype.sanitized())],
                ),
                (TimelineOperationKind::Protected, None) => t("timeline.op.protected-unnamed", &[]),
                (TimelineOperationKind::Undo, _) => t("timeline.op.undo", &[]),
                (TimelineOperationKind::Redo, _) => t("timeline.op.redo", &[]),
                (TimelineOperationKind::Restore, _) => t("timeline.op.restore", &[]),
            };
            match state {
                TimelineOperationState::Finished => {}
                TimelineOperationState::Applying => {
                    text.push_str(&format!(" ({})", t("timeline.state.applying", &[])));
                }
                TimelineOperationState::Interrupted => {
                    text.push_str(&format!(" ({})", t("timeline.state.interrupted", &[])));
                }
            }
            text
        }
        EntryOrigin::GitEvent { kind, branch, .. } => {
            let branch = branch
                .as_ref()
                .map_or_else(|| t("timeline.branch-unknown", &[]), |b| b.sanitized());
            match kind {
                GitEventKind::BranchSwitch => t("event.branch-switch-to", &[("branch", &branch)]),
                GitEventKind::Reset if !branch_known(origin) => t("event.reset-detached", &[]),
                kind => t(&format!("event.{}", kind.as_str()), &[("branch", &branch)]),
            }
        }
    }
}

/// The wire name of the surface; it is a fixed set, so it is not translated.
fn channel_text(channel: TimelineChannel) -> &'static str {
    match channel {
        TimelineChannel::Cli => "cli",
        TimelineChannel::Tui => "tui",
        TimelineChannel::Mcp => "mcp",
        TimelineChannel::Hook => "hook",
    }
}

fn branch_known(origin: &EntryOrigin) -> bool {
    matches!(origin, EntryOrigin::GitEvent { branch: Some(_), .. })
}

/// Who: the live attribution as `raptor events` says it, or the requester
/// as it was recorded (name and origin, without "another agent"). Without
/// session information the engine cannot tell "no agent" from "unknown".
fn actor_text(entry: &TimelineEntry, detection_available: bool) -> String {
    use gitraptor_api::timemachine::Attribution;
    match (&entry.attribution, &entry.actor) {
        (Attribution::Current, Actor::Unattributed) if !detection_available => {
            t("timeline.actor-unavailable", &[])
        }
        (
            Attribution::Recorded,
            Actor::Agent {
                kind: AgentKind::Other,
                name: Some(name),
                origin,
            },
        ) => recorded_agent(&name.sanitized(), *origin),
        (_, actor) => events::actor(actor),
    }
}

fn recorded_agent(name: &str, origin: AgentOrigin) -> String {
    t(
        "actor.with-origin",
        &[
            ("origin", &crate::sessions::origin_text(origin)),
            ("agent", &name),
        ],
    )
}

fn protection_key(level: ProtectionLevel) -> &'static str {
    match level {
        ProtectionLevel::GuaranteedPrior | ProtectionLevel::HookPrior => "timeline.level.prior",
        ProtectionLevel::Observation => "timeline.level.observation",
        ProtectionLevel::Manual => "timeline.level.manual",
        ProtectionLevel::None => "timeline.level.none",
    }
}

fn acted_text(target: &ActedOn) -> String {
    match target {
        ActedOn::Operation(id) => t("timeline.acted.operation", &[("id", &sanitize(id))]),
        ActedOn::GitEvent(seq) => t("timeline.acted.event", &[("seq", seq)]),
        ActedOn::Snapshot(id) => t("timeline.acted.snapshot", &[("id", &sanitize(id))]),
    }
}

/// The listed paths, "+K more" with the real total, and what the list is against.
fn files_text(files: &ChangedFiles) -> String {
    match files {
        ChangedFiles::Unavailable => t("timeline.files-unavailable", &[]),
        ChangedFiles::Available {
            paths,
            total,
            first_parent,
        } => {
            let mut items: Vec<String> = paths.iter().map(|p| p.sanitized()).collect();
            let hidden = (*total as usize).saturating_sub(paths.len());
            if hidden > 0 {
                items.push(t("timeline.files-more", &[("count", &hidden)]));
            }
            let mut text = if items.is_empty() {
                t("timeline.files-none", &[])
            } else {
                t(
                    "timeline.files",
                    &[("total", total), ("paths", &items.join(", "))],
                )
            };
            if *first_parent {
                text.push_str(&format!(" ({})", t("timeline.files-first-parent", &[])));
            }
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::timemachine::Attribution;
    use gitraptor_api::untrusted::Untrusted;

    fn untrusted(text: &str) -> Untrusted {
        serde_json::from_value(serde_json::json!({ "untrusted": text })).unwrap()
    }

    fn entry(actor: Actor, attribution: Attribution, files: ChangedFiles) -> TimelineEntry {
        TimelineEntry {
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
            attribution,
            protection: gitraptor_api::timemachine::Protection {
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

    #[test]
    fn every_key_has_both_languages() {
        for key in [
            "timeline.line",
            "timeline.point",
            "timeline.empty",
            "timeline.no-match",
            "timeline.incomplete",
            "timeline.truncated",
            "timeline.actor-unavailable",
            "timeline.files",
            "timeline.files-more",
            "timeline.files-none",
            "timeline.files-unavailable",
            "timeline.files-first-parent",
            "timeline.level.prior",
            "timeline.level.observation",
            "timeline.level.none",
            "timeline.level.manual",
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
        let text = files_text(&files(&["a", "b"], 25));
        assert!(text.contains(&t("timeline.files-more", &[("count", &23)])), "{text}");
        assert!(text.contains("25"), "{text}");
    }

    #[test]
    fn an_unavailable_source_is_never_called_no_activity() {
        let shown = render(&result(vec![], vec![TimelineSource::Events]), false);
        assert!(!shown.contains(&t("timeline.empty", &[])), "{shown}");
        assert!(shown.contains(&t(source_key(TimelineSource::Events), &[])), "{shown}");
        let quiet = render(&result(vec![], vec![]), false);
        assert!(quiet.contains(&t("timeline.empty", &[])), "{quiet}");
    }

    #[test]
    fn unattributed_reads_no_agent_or_not_available_never_human() {
        let e = entry(Actor::Unattributed, Attribution::Current, files(&[], 0));
        assert_eq!(actor_text(&e, true), t("events.no_agent", &[]));
        assert_eq!(actor_text(&e, false), t("timeline.actor-unavailable", &[]));
        let shown = render(&result(vec![e], vec![]), false).to_lowercase();
        assert!(!shown.contains("human") && !shown.contains("humano"), "{shown}");
    }

    #[test]
    fn an_undo_entry_shows_the_recorded_requester_not_the_current_actor() {
        let actor = Actor::Agent {
            kind: AgentKind::Other,
            name: Some(serde_json::from_value(serde_json::json!({ "untrusted": "Codex" })).unwrap()),
            origin: AgentOrigin::Registered,
        };
        let e = entry(actor, Attribution::Recorded, files(&[], 0));
        let text = actor_text(&e, true);
        assert!(text.contains("Codex"), "{text}");
        assert!(!text.contains(&t("actor.other", &[])), "{text}");
    }

    #[test]
    fn unavailable_files_are_not_a_zero() {
        let text = files_text(&ChangedFiles::Unavailable);
        assert_eq!(text, t("timeline.files-unavailable", &[]));
    }

    #[test]
    fn a_merge_says_its_paths_are_against_the_first_parent() {
        let merged = ChangedFiles::Available {
            paths: vec![untrusted("a")],
            total: 1,
            first_parent: true,
        };
        assert!(files_text(&merged).contains(&t("timeline.files-first-parent", &[])));
    }

    #[test]
    fn paths_with_escape_sequences_are_printed_sanitized() {
        let e = entry(
            Actor::Unattributed,
            Attribution::Current,
            files(&["a\x1b[31mred\nb"], 1),
        );
        let shown = render(&result(vec![e], vec![]), false);
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
        assert!(shown.contains(&format!("[{}]", t("timeline.level.manual", &[]))), "{shown:?}");
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

    /// The three levels of protection read the same in both languages: a prior snapshot (of
    /// GitRaptor's own operation or a hook), a capture by observation, and none.
    #[test]
    fn the_three_protection_levels_read_right_in_es_and_en() {
        use crate::i18n::text_in;
        let want = [
            (ProtectionLevel::GuaranteedPrior, "prior snapshot", "snapshot previo"),
            (ProtectionLevel::HookPrior, "prior snapshot", "snapshot previo"),
            (
                ProtectionLevel::Observation,
                "captured by observation",
                "capturado por observación",
            ),
            (ProtectionLevel::None, "unprotected", "sin protección"),
            (ProtectionLevel::Manual, "manual point", "punto manual"),
        ];
        for (level, en, es) in want {
            let key = protection_key(level);
            assert_eq!(text_in(false, key), Some(en), "{level:?}");
            assert_eq!(text_in(true, key), Some(es), "{level:?}");
            // And it is what the entry's line carries, in the language of the process.
            let mut e = entry(Actor::Unattributed, Attribution::Current, files(&[], 0));
            e.protection.level = level;
            let shown = render(&result(vec![e], vec![]), false);
            assert!(shown.contains(&format!("[{}]", t(key, &[]))), "{shown}");
        }
    }

    #[test]
    fn an_entry_with_a_saved_state_shows_the_id_restore_takes() {
        let mut e = entry(Actor::Unattributed, Attribution::Current, files(&[], 0));
        let none = render(&result(vec![e.clone()], vec![]), false);
        assert!(!none.contains("raptor restore"), "{none}");
        e.protection.snapshot_id = Some("0a1b2c3d-0000-4000-8000-000000000000".into());
        let shown = render(&result(vec![e], vec![]), false);
        assert!(
            shown.contains("raptor restore 0a1b2c3d-0000-4000-8000-000000000000"),
            "{shown}"
        );
    }

    #[test]
    fn a_worktree_shows_its_last_component() {
        assert_eq!(worktree_name("/a/b/wt-c/"), "wt-c");
        assert_eq!(worktree_name(r"C:\a\wt-c"), "wt-c");
        assert_eq!(worktree_name("/"), "/");
    }
}
