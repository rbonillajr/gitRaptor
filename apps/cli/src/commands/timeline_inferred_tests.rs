//! Contract tests of the inferred hint in `raptor timeline`: the same text as
//! `raptor events`, in English and Spanish.

use gitraptor_api::messages::{InferredAgent, TrailerCheck};
use gitraptor_api::timemachine::{Attribution, Protection};
use gitraptor_api::untrusted::Untrusted;

use super::*;

fn untrusted(text: &str) -> Untrusted {
    serde_json::from_value(serde_json::json!({ "untrusted": text })).unwrap()
}

fn hint(trailer: Option<TrailerCheck>) -> InferredAgent {
    InferredAgent {
        kind: AgentKind::ClaudeCode,
        session_id: "20:2000".into(),
        trailer,
    }
}

fn entry(seq: i64, actor: Actor, inferred: Option<InferredAgent>) -> TimelineEntry {
    TimelineEntry {
        id: format!("event:{seq}"),
        origin: EntryOrigin::GitEvent {
            seq,
            kind: GitEventKind::Commit,
            branch: None,
        },
        occurred_utc_ms: 0,
        utc_offset_s: 0,
        worktrees: vec![untrusted("/repo/wt-a")],
        actor,
        inferred,
        attribution: Attribution::Current,
        protection: Protection {
            level: ProtectionLevel::None,
            snapshot_id: None,
        },
        files: ChangedFiles::Unavailable,
    }
}

fn result(entries: Vec<TimelineEntry>, detection_available: bool) -> TimelineResult {
    TimelineResult {
        repo_id: "r".into(),
        entries,
        truncated: false,
        unavailable: Vec::new(),
        detection_available,
    }
}

/// The text `raptor events` writes for a hint, built from the catalog.
fn expected(trailer_key: Option<&str>) -> String {
    let agent = t("actor.claude-code", &[]);
    let agent = match trailer_key {
        Some(key) => format!("{agent} ({})", t(key, &[])),
        None => agent,
    };
    t("events.no_agent_inferred", &[("agent", &agent)])
}

#[test]
fn an_inferred_entry_reads_like_raptor_events_in_en_and_es() {
    use crate::i18n::text_in;
    // The reused keys say what the content guide says, in both languages.
    for (key, en, es) in [
        (
            "events.no_agent_inferred",
            "no agent; inferred: {agent}",
            "sin agente; inferido: {agent}",
        ),
        (
            "events.inferred_confirmed",
            "confirmed by the trailer",
            "confirmado por el trailer",
        ),
        (
            "events.inferred_unconfirmed",
            "not confirmed by the trailer",
            "no confirmado por el trailer",
        ),
    ] {
        assert_eq!(text_in(false, key), Some(en), "{key} en");
        assert_eq!(text_in(true, key), Some(es), "{key} es");
    }

    for (trailer, key) in [
        (Some(TrailerCheck::Confirmed), Some("events.inferred_confirmed")),
        (
            Some(TrailerCheck::Unconfirmed),
            Some("events.inferred_unconfirmed"),
        ),
        (None, None),
    ] {
        let hint = hint(trailer);
        let want = expected(key);
        // The very text of `raptor events`.
        assert_eq!(events::inferred_actor(&hint), want);
        let shown = render(
            &result(vec![entry(1, Actor::Unattributed, Some(hint.clone()))], true),
            false,
        );
        assert!(shown.contains(&format!("({want})")), "{shown}");
        // A stored hint shows even when the engine cannot detect agents now.
        let shown = render(
            &result(vec![entry(1, Actor::Unattributed, Some(hint))], false),
            false,
        );
        assert!(shown.contains(&format!("({want})")), "{shown}");
        assert!(
            !shown.contains(&t("timeline.actor-unavailable", &[])),
            "{shown}"
        );
    }
    // The session id is never printed as text.
    let shown = render(
        &result(
            vec![entry(
                1,
                Actor::Unattributed,
                Some(hint(Some(TrailerCheck::Confirmed))),
            )],
            true,
        ),
        false,
    );
    assert!(!shown.contains("20:2000"), "{shown}");
}

/// A contradicted hint, or one of a `human-author` repo, is never stored:
/// that entry reads "no agent", next to one that has its hint.
#[test]
fn an_entry_without_hint_reads_no_agent() {
    let no_agent = format!("({})", t("events.no_agent", &[]));
    let shown = render(
        &result(
            vec![
                entry(1, Actor::Unattributed, None),
                entry(2, Actor::Unattributed, Some(hint(Some(TrailerCheck::Confirmed)))),
            ],
            true,
        ),
        false,
    );
    assert_eq!(
        shown.matches(&no_agent).count(),
        1,
        "only the entry without hint reads no agent: {shown}"
    );
    assert_eq!(
        shown
            .matches(&format!("({})", expected(Some("events.inferred_confirmed"))))
            .count(),
        1,
        "{shown}"
    );
    let first = shown.lines().next().unwrap_or_default();
    assert!(first.contains(&no_agent), "{shown}");
}
