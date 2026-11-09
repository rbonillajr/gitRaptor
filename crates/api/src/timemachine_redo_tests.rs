//! TMC003-WIRE (T001 of DS-US-TMC-003): the wire form of the redo: two new rejection reasons at
//! the end of `TmRejectReason`, an optional `confirmation` in `RedoParams` and `RedoResult`.
//! Mounted at the crate root like `timemachine_agent_tests`.

use crate::Untrusted;
use crate::actor::Actor;
use crate::timemachine::*;
use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789ABCDEF";

fn redo(body: Value) -> RedoParams {
    serde_json::from_value(body).expect("TMC003-WIRE: RedoParams must parse")
}

fn result() -> RedoResult {
    RedoResult {
        operation_id: "op-redo".into(),
        prior_snapshot_id: "snap-prior".into(),
        redone_undo_id: "op-undo".into(),
        target_snapshot_id: "snap-target".into(),
        requester: RequesterView {
            actor: Actor::Unattributed,
            channel: RequestChannel::Tui,
            via: ResolvedVia::None,
            confirmable: false,
        },
        written: 1,
        removed: 2,
        not_restored: Vec::new(),
        kept: Vec::new(),
        kept_total: 0,
        warnings: Vec::new(),
    }
}

#[test]
fn redo_reasons_wire_form() {
    for (wire, reason) in [
        ("nothing-to-redo", "NothingToRedo"),
        ("redo-invalidated", "RedoInvalidated"),
    ] {
        let parsed: TmRejectReason = serde_json::from_value(json!(wire)).unwrap_or_else(|e| {
            panic!("TMC003-WIRE: {wire} ({reason}) must be a TmRejectReason: {e}")
        });
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            json!(wire),
            "TMC003-WIRE: {reason} serializes as {wire}"
        );
        assert_eq!(format!("{parsed:?}"), reason);
    }
    // They go at the end: the schema lists them after the last reason of US-TMC-011.
    let schema = serde_json::to_value(schemars::schema_for!(TmRejectReason)).unwrap();
    let text = schema.to_string();
    let last = text
        .find("attribution-unavailable")
        .expect("existing reason");
    let first = text
        .find("nothing-to-redo")
        .expect("TMC003-WIRE: nothing-to-redo in the schema");
    let second = text
        .find("redo-invalidated")
        .expect("TMC003-WIRE: redo-invalidated in the schema");
    assert!(
        last < first && first < second,
        "TMC003-WIRE: nothing-to-redo and redo-invalidated come after attribution-unavailable"
    );
}

#[test]
fn redo_params_confirmation_is_optional_and_omitted() {
    let plain = redo(json!({}));
    assert!(plain.confirmation.is_none());
    assert!(plain.validate().is_ok());
    let value = serde_json::to_value(&plain).unwrap();
    assert!(
        !value.as_object().unwrap().contains_key("confirmation"),
        "TMC003-WIRE: a redo without confirmation must not serialize the key"
    );
    let with = redo(json!({ "confirmation": TOKEN }));
    assert!(
        with.validate().is_ok(),
        "TMC003-WIRE: a well-formed token is accepted"
    );
    assert_eq!(
        serde_json::to_value(&with).unwrap()["confirmation"],
        json!(TOKEN),
        "TMC003-WIRE: the token travels back as sent"
    );
}

#[test]
fn redo_params_reject_a_bad_confirmation() {
    for bad in [
        "short",
        "zz23456789abcdef0123456789ABCDEF",
        "0123456789abcdef0123456789ABCDEF00",
    ] {
        let error = redo(json!({ "confirmation": bad }))
            .validate()
            .expect_err("TMC003-WIRE: a malformed confirmation token must be INVALID_PARAMS");
        assert_eq!(error.field, "confirmation", "TMC003-WIRE: field of {bad}");
    }
}

#[test]
fn redo_result_serializes_like_the_undo_result() {
    let value = serde_json::to_value(result()).unwrap();
    let object = value.as_object().unwrap();
    let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "not_restored",
            "operation_id",
            "prior_snapshot_id",
            "redone_undo_id",
            "removed",
            "requester",
            "target_snapshot_id",
            "warnings",
            "written",
        ],
        "TMC003-WIRE: RedoResult has the undo's fields with redone_undo_id, and omits empty kept"
    );
    assert!(!object.contains_key("undone_operation_id"));
    assert!(!object.contains_key("undone_subtype"));
    let back: RedoResult = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(back).unwrap(), value);
}

#[test]
fn redo_result_lists_kept_paths_and_their_total() {
    let mut cut = result();
    cut.kept = vec![KeptPath {
        worktree: Untrusted::new("feat-login"),
        path: Untrusted::new("b.rs"),
    }];
    cut.kept_total = 120;
    let value = serde_json::to_value(&cut).unwrap();
    assert_eq!(
        value["kept"],
        json!([{ "worktree": { "untrusted": "feat-login" }, "path": { "untrusted": "b.rs" } }]),
        "TMC003-WIRE: kept travels when not empty"
    );
    assert_eq!(
        value["kept_total"],
        json!(120),
        "TMC003-WIRE: kept_total travels when not zero"
    );
}
