//! `raptor events` output, text and JSON (US-GRP-002): the latest Git
//! events of the observed repos, with their time and actor. A one-off query;
//! the live view belongs to the Cockpit (F-001-02).
//!
//! Every text that comes from the engine is untrusted (SEC-12): the text
//! output prints it sanitized; the JSON output carries it as JSON strings.

use std::fmt::Write as _;

use gitraptor_api::messages::{GitEventKind, GitEventView};
use gitraptor_api::{Actor, UntrustedName};
use serde_json::{Value, json};

use crate::i18n::t;
use crate::status::wire;

/// The text output, one line per event, oldest first.
pub fn text(events: &[GitEventView]) -> String {
    let mut out = String::new();
    if events.is_empty() {
        let _ = writeln!(out, "{}", t("events.none", &[]));
    }
    for e in events {
        let worktree = e.worktree.sanitized();
        let worktree = if e.details.worktree_inferred {
            t("events.inferred", &[("worktree", &worktree)])
        } else {
            worktree
        };
        let _ = writeln!(
            out,
            "{}",
            t(
                "events.line",
                &[
                    ("time", &local_time(e.observed_utc_ms, e.utc_offset_s)),
                    ("worktree", &worktree),
                    ("event", &describe(e)),
                    ("actor", &actor(&e.actor)),
                ],
            )
        );
    }
    out
}

fn describe(e: &GitEventView) -> String {
    let show =
        |u: &Option<UntrustedName>| u.as_ref().map(UntrustedName::sanitized).unwrap_or_default();
    let branch = show(&e.details.branch);
    match e.kind {
        GitEventKind::BranchSwitch if e.details.from.is_some() => t(
            "event.branch-switch",
            &[("from", &show(&e.details.from)), ("branch", &branch)],
        ),
        GitEventKind::BranchSwitch => t("event.branch-switch-to", &[("branch", &branch)]),
        kind => t(&format!("event.{}", kind.as_str()), &[("branch", &branch)]),
    }
}

fn actor(actor: &Actor) -> String {
    match actor {
        Actor::Unattributed => t("actor.unattributed", &[]),
        Actor::Agent { kind, name, .. } => match name {
            Some(name) => t("actor.other-agent", &[("name", &name.sanitized())]),
            None => t(&format!("actor.{}", wire(kind)), &[]),
        },
    }
}

/// The JSON output: a stable, flat shape with plain strings.
pub fn json(events: &[GitEventView]) -> Value {
    let plain = |u: &Option<UntrustedName>| u.as_ref().map(|u| u.raw().to_owned());
    Value::Array(
        events
            .iter()
            .map(|e| {
                json!({
                    "repo_id": e.repo_id,
                    "seq": e.seq,
                    "worktree": e.worktree.raw(),
                    "worktree_inferred": e.details.worktree_inferred,
                    "kind": e.kind.as_str(),
                    "actor": match &e.actor {
                        Actor::Unattributed => json!("unattributed"),
                        other => serde_json::to_value(other).unwrap_or(Value::Null),
                    },
                    "observed_utc_ms": e.observed_utc_ms,
                    "utc_offset_s": e.utc_offset_s,
                    "branch": plain(&e.details.branch),
                    "from": plain(&e.details.from),
                    "old_commit": e.details.old_commit,
                    "new_commit": e.details.new_commit,
                    "gap_id": e.gap_id,
                })
            })
            .collect(),
    )
}

/// `YYYY-MM-DD HH:MM:SS` at the event's own offset.
pub fn local_time(utc_ms: i64, offset_s: i32) -> String {
    let secs = utc_ms.div_euclid(1000) + i64::from(offset_s);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::messages::GitEventDetails;

    fn event(kind: GitEventKind, inferred: bool) -> GitEventView {
        GitEventView {
            repo_id: "r".into(),
            seq: 1,
            worktree: gitraptor_api::Untrusted::new("/w/feat-login\u{1b}[31m"),
            kind,
            actor: Actor::Unattributed,
            observed_utc_ms: 1_791_148_066_018,
            utc_offset_s: 7200,
            details: GitEventDetails {
                branch: Some(UntrustedName::new("feat-login")),
                worktree_inferred: inferred,
                ..GitEventDetails::default()
            },
            gap_id: None,
        }
    }

    #[test]
    fn local_time_uses_the_offset() {
        assert_eq!(local_time(0, 0), "1970-01-01 00:00:00");
        assert_eq!(local_time(1_791_148_066_018, 0), "2026-10-04 21:07:46");
        assert_eq!(
            local_time(1_791_148_066_018, -5 * 3600),
            "2026-10-04 16:07:46"
        );
        assert_eq!(local_time(951_782_400_000, 0), "2000-02-29 00:00:00");
    }

    #[test]
    fn text_is_sanitized_and_marks_the_inferred_worktree() {
        let out = text(&[event(GitEventKind::Commit, false)]);
        assert!(!out.contains('\u{1b}'), "{out}");
        assert!(out.contains("commit on feat-login"), "{out}");
        assert!(out.contains("unattributed"), "{out}");
        let out = text(&[event(GitEventKind::BranchDelete, true)]);
        assert!(out.contains("(inferred)"), "{out}");
    }

    #[test]
    fn every_kind_has_a_message() {
        for kind in GitEventKind::ALL {
            let shown = describe(&event(kind, false));
            assert!(!shown.starts_with("event."), "{kind:?}: {shown}");
        }
    }

    #[test]
    fn json_is_flat() {
        let value = json(&[event(GitEventKind::Push, false)]);
        assert_eq!(value[0]["kind"], "push");
        assert_eq!(value[0]["actor"], "unattributed");
        assert_eq!(value[0]["branch"], "feat-login");
        assert_eq!(value[0]["worktree_inferred"], false);
    }
}
