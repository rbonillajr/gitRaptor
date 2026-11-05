//! TMC010-WIRE (T001 of DS-US-TMC-010): the wire form of the `since` undo result.
//! Mounted at the crate root like `timemachine_overlap_tests`.

use crate::actor::Actor;
use crate::timemachine::*;
use serde_json::{Value, json};

fn result(ids: Vec<&str>) -> UndoResult {
    UndoResult {
        operation_id: "op-undo".into(),
        prior_snapshot_id: "snap-prior".into(),
        undone_operation_id: ids.first().copied().unwrap_or("op-1").into(),
        undone_subtype: None,
        target_snapshot_id: "snap-target".into(),
        requester: RequesterView {
            actor: Actor::Unattributed,
            channel: RequestChannel::Tui,
            via: ResolvedVia::None,
            confirmable: false,
        },
        written: 1,
        removed: 0,
        not_restored: Vec::new(),
        warnings: Vec::new(),
        kept: Vec::new(),
        kept_total: 0,
        undone_operation_ids: ids.into_iter().map(String::from).collect(),
    }
}

#[test]
fn undo_result_without_since_is_todays_form() {
    let value = serde_json::to_value(result(vec![])).unwrap();
    let object = value.as_object().unwrap();
    assert!(
        !object.contains_key("undone_operation_ids"),
        "TMC010-WIRE: an undo without since must not serialize undone_operation_ids"
    );
    assert_eq!(object["undone_operation_id"], json!("op-1"));
    // A response written before the field existed still reads.
    let old: UndoResult = serde_json::from_value(value).unwrap();
    assert!(
        old.undone_operation_ids.is_empty(),
        "TMC010-WIRE: a response without the field reads as an empty list"
    );
}

#[test]
fn undo_result_lists_every_undone_operation() {
    let value = serde_json::to_value(result(vec!["op-b", "op-c"])).unwrap();
    assert_eq!(
        value["undone_operation_ids"],
        json!(["op-b", "op-c"]),
        "TMC010-WIRE: undone_operation_ids lists every undone operation, oldest first"
    );
    assert_eq!(
        value["undone_operation_id"],
        json!("op-b"),
        "TMC010-WIRE: undone_operation_id is the oldest of the set"
    );
    let back: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(back.undone_operation_ids, vec!["op-b", "op-c"]);
}

#[test]
fn mcp_undo_result_counts_the_undone_operations_and_omits_zero() {
    let mcp = result(vec!["op-b", "op-c", "op-d"]).for_mcp();
    assert_eq!(
        mcp.undone_count, 3,
        "TMC010-WIRE: undone_count is the number of undone operations"
    );
    let text = serde_json::to_string(&mcp).unwrap();
    assert!(text.contains("\"undone_count\":3"), "{text}");
    for leak in ["op-c", "op-d", "undone_operation_ids", "snap-target"] {
        assert!(
            !text.contains(leak),
            "TMC010-WIRE: the MCP projection must not list ids or paths: {text}"
        );
    }
    let none: Value = serde_json::to_value(result(vec![]).for_mcp()).unwrap();
    assert!(
        none.get("undone_count").is_none(),
        "TMC010-WIRE: undone_count is omitted when 0: {none}"
    );
}
