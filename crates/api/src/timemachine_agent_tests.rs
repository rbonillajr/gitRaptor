//! TMC011-WIRE (T001 of DS-US-TMC-011): `agent` combines with `since`, `operation_id` excludes
//! both, `unattributed` is not an agent for the undo, and the three new rejection reasons keep
//! their wire form at the end of `TmRejectReason`. Mounted at the crate root like
//! `timemachine_since_tests`.

use crate::timemachine::*;
use serde_json::json;

fn undo(json: &str) -> Result<(), crate::timemachine::Invalid> {
    serde_json::from_str::<UndoParams>(json).unwrap().validate()
}

#[test]
fn agent_combines_with_since_but_not_with_operation_id() {
    assert!(
        undo(r#"{"agent":"claude-1","since":"20m"}"#).is_ok(),
        "TMC011-WIRE: agent and since combine"
    );
    assert!(
        undo(r#"{"agent":"claude-code"}"#).is_ok(),
        "TMC011-WIRE: agent alone stays valid"
    );
    let op = "00000000-0000-4000-8000-000000000001";
    for body in [
        json!({"operation_id": op, "agent": "claude-1"}),
        json!({"operation_id": op, "since": "20m"}),
        json!({"operation_id": op, "agent": "claude-1", "since": "20m"}),
    ] {
        assert!(
            undo(&body.to_string()).is_err(),
            "TMC011-WIRE: operation_id excludes agent and since: {body}"
        );
    }
}

#[test]
fn unattributed_is_not_an_agent_for_undo() {
    let error = undo(r#"{"agent":"unattributed"}"#)
        .expect_err("TMC011-WIRE: agent=unattributed must be INVALID_PARAMS");
    assert_eq!(error.field, "agent");
    let combined = undo(r#"{"agent":"unattributed","since":"20m"}"#)
        .expect_err("TMC011-WIRE: agent=unattributed is invalid with since too");
    assert_eq!(combined.field, "agent");
}

#[test]
fn agent_reasons_wire_form() {
    for (wire, reason) in [
        ("not-selectable", "NotSelectable"),
        ("selective-undo-below", "SelectiveUndoBelow"),
        ("attribution-unavailable", "AttributionUnavailable"),
    ] {
        let parsed: TmRejectReason = serde_json::from_value(json!(wire)).unwrap_or_else(|e| {
            panic!("TMC011-WIRE: {wire} ({reason}) must be a TmRejectReason: {e}")
        });
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            json!(wire),
            "TMC011-WIRE: {reason} serializes as {wire}"
        );
        assert_eq!(format!("{parsed:?}"), reason);
    }
}
