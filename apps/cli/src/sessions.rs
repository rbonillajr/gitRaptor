//! Agent sessions in the CLI output (US-GRP-007): the lines `raptor status`
//! shows under each worktree, and `raptor sessions`, text and JSON.
//!
//! Every text that comes from the engine is untrusted (SEC-12): the text
//! output prints it sanitized; the JSON output carries it as JSON strings.

use std::fmt::Write as _;

use gitraptor_api::messages::{SessionStateView, SessionView};
use gitraptor_api::{Actor, AgentOrigin};
use serde::Serialize;
use serde_json::{Value, json};

use crate::events::{actor_name, local_time};
use crate::i18n::t;
use crate::status::wire;

/// One session in one line: agent, state, origin and the time of its state
/// (its start or its end).
pub fn line(s: &SessionView) -> String {
    let state = t(&format!("session-state.{}", s.state.as_str()), &[]);
    let origin = match &s.actor {
        Actor::Agent { origin, .. } => origin_text(*origin),
        Actor::Unattributed => String::new(),
    };
    // The agent name last: an "other agent" declares it, and it must not
    // fill the other placeholders.
    let agent = actor_name(&s.actor);
    match (s.state, s.ended_utc_ms) {
        (SessionStateView::Ended, Some(ended)) => t(
            "session.ended",
            &[
                ("state", &state),
                ("origin", &origin),
                ("time", &local_time(ended, s.utc_offset_s)),
                ("agent", &agent),
            ],
        ),
        (SessionStateView::Ended, None) => t(
            "session.ended-unknown",
            &[("state", &state), ("origin", &origin), ("agent", &agent)],
        ),
        _ => t(
            "session.present",
            &[
                ("state", &state),
                ("origin", &origin),
                ("time", &local_time(s.state_since_utc_ms, s.utc_offset_s)),
                ("agent", &agent),
            ],
        ),
    }
}

pub fn origin_text(origin: AgentOrigin) -> String {
    t(&format!("origin.{}", wire(&origin)), &[])
}

/// `raptor sessions` as text, oldest first.
pub fn text(sessions: &[SessionView], detection_available: bool) -> String {
    let mut out = String::new();
    if !detection_available {
        let _ = writeln!(out, "{}", t("sessions.unavailable", &[]));
    }
    if sessions.is_empty() {
        let _ = writeln!(out, "{}", t("sessions.none", &[]));
    }
    for s in sessions {
        let _ = writeln!(
            out,
            "{}",
            t(
                "sessions.line",
                &[("session", &line(s)), ("worktree", &s.worktree.sanitized())],
            )
        );
    }
    out
}

/// One session in the JSON outputs: a flat shape with plain strings.
#[derive(Debug, Clone, Serialize)]
pub struct SessionJson {
    repo_id: String,
    session_id: String,
    worktree: String,
    /// `claude-code` or `other`.
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_name: Option<String>,
    /// `detected` or `registered`.
    origin: String,
    /// `active`, `inactive` or `ended`.
    state: String,
    started_utc_ms: i64,
    state_since_utc_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_utc_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_cause: Option<String>,
}

pub fn session_json(s: &SessionView) -> SessionJson {
    let (agent, agent_name, origin) = match &s.actor {
        Actor::Agent { kind, name, origin } => (
            wire(kind),
            name.as_ref().map(|n| n.raw().to_owned()),
            wire(origin),
        ),
        Actor::Unattributed => (String::new(), None, String::new()),
    };
    SessionJson {
        repo_id: s.repo_id.clone(),
        session_id: s.session_id.clone(),
        worktree: s.worktree.raw().to_owned(),
        agent,
        agent_name,
        origin,
        state: s.state.as_str().to_owned(),
        started_utc_ms: s.started_utc_ms,
        state_since_utc_ms: s.state_since_utc_ms,
        ended_utc_ms: s.ended_utc_ms,
        end_cause: s.end_cause.as_ref().map(wire),
    }
}

/// `raptor sessions --json`.
pub fn json(sessions: &[SessionView], detection_available: bool) -> Value {
    json!({
        "detection_available": detection_available,
        "sessions": sessions.iter().map(session_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::messages::SessionEndCauseView;
    use gitraptor_api::{AgentKind, Untrusted};

    fn session(state: SessionStateView, ended: Option<i64>) -> SessionView {
        SessionView {
            repo_id: "r".into(),
            session_id: "20:2000".into(),
            worktree: Untrusted::new("/w/feat-login\u{1b}[31m"),
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            state,
            started_utc_ms: 1_791_148_066_018,
            state_since_utc_ms: 1_791_148_066_018,
            utc_offset_s: 0,
            ended_utc_ms: ended,
            end_cause: (state == SessionStateView::Ended)
                .then_some(SessionEndCauseView::ProcessGone),
        }
    }

    #[test]
    fn lines_show_agent_state_and_origin() {
        let out = line(&session(SessionStateView::Active, None));
        assert!(out.starts_with("Claude Code · Active · detected"), "{out}");
        let out = line(&session(SessionStateView::Ended, Some(1_791_148_066_018)));
        assert!(out.contains("Ended"), "{out}");
        let out = line(&session(SessionStateView::Ended, None));
        assert!(out.contains("end unknown"), "{out}");
    }

    #[test]
    fn text_is_sanitized_and_says_when_detection_is_unavailable() {
        let out = text(&[session(SessionStateView::Inactive, None)], false);
        assert!(!out.contains('\u{1b}'), "{out}");
        assert!(out.contains("Inactive"), "{out}");
        assert!(out.contains("cannot be detected"), "{out}");
        assert!(text(&[], true).contains("no agent sessions"));
    }

    #[test]
    fn json_is_flat() {
        let value = json(&[session(SessionStateView::Ended, Some(5))], true);
        let s = &value["sessions"][0];
        assert_eq!(s["agent"], "claude-code");
        assert_eq!(s["origin"], "detected");
        assert_eq!(s["state"], "ended");
        assert_eq!(s["end_cause"], "process-gone");
        assert_eq!(s["worktree"], "/w/feat-login\u{1b}[31m");
    }
}
