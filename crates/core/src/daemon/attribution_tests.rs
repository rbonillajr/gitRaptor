//! Tests of the effective attribution (US-GRP-010, T002, D1-D3, Q50). Child module of
//! `daemon::attribution`, attached there with
//! `#[cfg(test)] #[path = "attribution_tests.rs"] mod tests;`.

use std::path::PathBuf;

use super::*;
use crate::profile::{Agent, AgentKind, AttributionRecord, Author, Origin, RecordKind, Session};

fn claude() -> Agent {
    Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
    }
}

fn codex() -> Agent {
    Agent {
        kind: AgentKind::Other,
        name: Some("Codex".into()),
    }
}

fn aider() -> Agent {
    Agent {
        kind: AgentKind::Other,
        name: Some("Aider".into()),
    }
}

fn session(initial_origin: Origin) -> Session {
    Session {
        session_id: "100:1".into(),
        worktree: PathBuf::from("/wt/feat-login"),
        agent: claude(),
        initial_origin,
        detection_key: Some("100:1".into()),
        started_ms: 1,
        ended_ms: None,
        end_cause: None,
    }
}

fn record(seq: i64, kind: RecordKind, agent: Agent) -> AttributionRecord {
    AttributionRecord {
        effective_seq: seq,
        session_id: "100:1".into(),
        kind,
        agent,
        author: Author::Developer,
        recorded_ms: seq,
    }
}

/// GRP010-EFF-1
#[test]
fn effective_without_records_is_the_detected_one() {
    let e = effective(&session(Origin::Detected), &[]);
    assert_eq!(e.agent, claude(), "no records: the detected agent");
    assert_eq!(e.origin, Origin::Detected, "no records: origin detected");
    assert!(!e.corrected, "no records: nothing is corrected");
}

/// GRP010-EFF-2
#[test]
fn a_correction_replaces_the_agent_with_origin_registered() {
    let records = [record(1, RecordKind::Correct, codex())];
    let e = effective(&session(Origin::Detected), &records);
    assert_eq!(e.agent, codex(), "the correction fixes the agent");
    assert_eq!(e.origin, Origin::Registered, "a correction is registered");
    assert!(e.corrected);
}

/// GRP010-EFF-3: D3
#[test]
fn a_second_correction_replaces_the_first() {
    let records = [
        record(1, RecordKind::Correct, codex()),
        record(2, RecordKind::Correct, aider()),
    ];
    let e = effective(&session(Origin::Detected), &records);
    assert_eq!(e.agent, aider(), "the second correction wins");
    assert_eq!(e.origin, Origin::Registered);
    assert!(e.corrected);
}

/// GRP010-EFF-4: Q50, D3
#[test]
fn withdrawing_returns_to_the_detected_one_not_to_an_earlier_correction() {
    let records = [
        record(1, RecordKind::Correct, codex()),
        record(2, RecordKind::Correct, aider()),
        record(3, RecordKind::WithdrawCorrection, aider()),
    ];
    let e = effective(&session(Origin::Detected), &records);
    assert_eq!(e.agent, claude(), "withdrawing returns the detected agent");
    assert_eq!(e.origin, Origin::Detected);
    assert!(!e.corrected, "no correction is left");

    // A new correction after the withdrawal is active again.
    let mut again = records.to_vec();
    again.push(record(4, RecordKind::Correct, codex()));
    let e = effective(&session(Origin::Detected), &again);
    assert_eq!(e.agent, codex());
    assert!(e.corrected);
}

/// GRP010-EFF-5: D2, P16
#[test]
fn a_confirmed_session_withdrawn_returns_registered() {
    let records = [
        record(1, RecordKind::Confirm, claude()),
        record(2, RecordKind::Correct, codex()),
        record(3, RecordKind::WithdrawCorrection, codex()),
    ];
    let e = effective(&session(Origin::Detected), &records);
    assert_eq!(e.agent, claude());
    assert_eq!(
        e.origin,
        Origin::Registered,
        "a confirmed session goes back to registered, not detected"
    );
    assert!(!e.corrected);

    // Confirmed after the correction was withdrawn is the same.
    let late = [
        record(1, RecordKind::Correct, codex()),
        record(2, RecordKind::WithdrawCorrection, codex()),
        record(3, RecordKind::Confirm, claude()),
    ];
    let e = effective(&session(Origin::Detected), &late);
    assert_eq!(e.origin, Origin::Registered);
    // Registration records never change the actor.
    let reg = [
        record(1, RecordKind::Register, codex()),
        record(2, RecordKind::WithdrawRegistration, codex()),
    ];
    let e = effective(&session(Origin::Detected), &reg);
    assert_eq!(e.agent, claude());
    assert_eq!(e.origin, Origin::Detected);
}
