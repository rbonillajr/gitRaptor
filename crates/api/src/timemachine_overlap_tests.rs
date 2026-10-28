//! TMC012-WIRE (T001 of DS-US-TMC-012): the wire form of the overlap codes and detail.
//! Mounted at the crate root like `timemachine_inferred_tests`.

use crate::Untrusted;
use crate::actor::Actor;
use crate::timemachine::*;
use serde_json::json;

fn conflict(kind_path: bool) -> OverlapConflict {
    OverlapConflict {
        subject: if kind_path {
            OverlapSubject::Path {
                worktree: Untrusted::new("feat-login"),
                path: Untrusted::new("api.rs"),
            }
        } else {
            OverlapSubject::Ref {
                name: Untrusted::new("feat-login"),
            }
        },
        actor: Actor::Unattributed,
        seen_utc_ms: 1_700_000_300_000,
    }
}

fn overlap() -> TmOverlap {
    TmOverlap {
        undone: OverlapUndone {
            id: "op-1".into(),
            subtype: Some(Untrusted::new("reset-hard")),
            actor: Actor::Unattributed,
            at_utc_ms: Some(1_700_000_000_000),
        },
        conflicts: vec![conflict(true), conflict(false)],
        total: 2,
    }
}

#[test]
fn tmc012_wire_the_new_reasons_are_kebab_codes_at_the_end_of_the_enum() {
    assert_eq!(
        serde_json::to_value(TmRejectReason::Overlap).unwrap(),
        json!("overlap")
    );
    assert_eq!(
        serde_json::to_value(TmRejectReason::OverlapUnverified).unwrap(),
        json!("overlap-unverified")
    );
    let back: TmRejectReason = serde_json::from_value(json!("overlap-unverified")).unwrap();
    assert_eq!(back, TmRejectReason::OverlapUnverified);
    // Appended after the last existing variant: a stable order for any client.
    assert_eq!(
        serde_json::to_value(TmRejectReason::ConfirmationUnavailable).unwrap(),
        json!("confirmation-unavailable")
    );
}

#[test]
fn tmc012_wire_the_overlap_detail_has_the_declared_shape() {
    let value = serde_json::to_value(overlap()).unwrap();
    assert_eq!(value["total"], json!(2));
    assert_eq!(value["undone"]["id"], json!("op-1"));
    assert_eq!(value["undone"]["actor"]["actor"], json!("unattributed"));
    assert_eq!(value["undone"]["at_utc_ms"], json!(1_700_000_000_000_i64));
    let conflicts = value["conflicts"].as_array().expect("conflicts list");
    assert_eq!(conflicts.len(), 2);
    assert_eq!(conflicts[0]["subject"]["kind"], json!("path"));
    assert_eq!(
        conflicts[0]["subject"]["worktree"],
        json!({"untrusted": "feat-login"})
    );
    assert_eq!(
        conflicts[0]["subject"]["path"],
        json!({"untrusted": "api.rs"})
    );
    assert_eq!(conflicts[0]["actor"]["actor"], json!("unattributed"));
    assert_eq!(conflicts[0]["seen_utc_ms"], json!(1_700_000_300_000_i64));
    assert_eq!(conflicts[1]["subject"]["kind"], json!("ref"));
    assert_eq!(
        conflicts[1]["subject"]["name"],
        json!({"untrusted": "feat-login"})
    );
    let back: TmOverlap = serde_json::from_value(value).unwrap();
    assert_eq!(back, overlap());
}

#[test]
fn tmc012_wire_unknown_fields_in_the_detail_are_refused() {
    let mut value = serde_json::to_value(overlap()).unwrap();
    value["extra"] = json!(1);
    assert!(serde_json::from_value::<TmOverlap>(value).is_err());
}

#[test]
fn tmc012_wire_confirm_data_omits_overlap_when_none() {
    let plain = TmConfirmData::rejected(TmRejectReason::Overlap, Some("op-2".into()));
    let value = serde_json::to_value(&plain).unwrap();
    assert!(value.get("overlap").is_none(), "{value}");
    assert_eq!(
        value,
        serde_json::to_value(TmRejectedData {
            reason: TmRejectReason::Overlap,
            operation_id: Some("op-2".into()),
        })
        .unwrap(),
        "without detail it serializes exactly like TmRejectedData"
    );

    let mut full = plain;
    full.overlap = Some(overlap());
    let value = serde_json::to_value(&full).unwrap();
    assert_eq!(value["overlap"]["total"], json!(2));
    let back: TmConfirmData = serde_json::from_value(value).unwrap();
    assert_eq!(back, full);
}

#[test]
fn tmc012_wire_undo_result_omits_an_empty_kept_and_lists_a_filled_one() {
    let base = json!({
        "operation_id": "op-3",
        "prior_snapshot_id": "s-1",
        "undone_operation_id": "op-1",
        "target_snapshot_id": "s-0",
        "requester": {
            "actor": { "actor": "unattributed" },
            "channel": "cli",
            "via": "none",
            "confirmable": false
        },
        "written": 1,
        "removed": 0,
        "not_restored": [],
        "warnings": []
    });
    let done: UndoResult = serde_json::from_value(base).expect("a result without kept parses");
    assert!(done.kept.is_empty());
    let value = serde_json::to_value(&done).unwrap();
    assert!(
        value.get("kept").is_none(),
        "an empty kept is omitted: {value}"
    );

    let mut with = done.clone();
    with.kept = vec![KeptPath {
        worktree: Untrusted::new("feat-login"),
        path: Untrusted::new("b.rs"),
    }];
    let value = serde_json::to_value(&with).unwrap();
    assert_eq!(
        value["kept"][0]["worktree"],
        json!({"untrusted": "feat-login"})
    );
    assert_eq!(value["kept"][0]["path"], json!({"untrusted": "b.rs"}));
    let back: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(back, with);
}

#[test]
fn tmc012_wire_mcp_result_counts_kept_and_defaults_to_zero() {
    let old: McpUndoResult = serde_json::from_value(json!({
        "operation_id": "op-3",
        "prior_snapshot_id": "s-1",
        "undone_operation_id": "op-1",
        "actor": { "actor": "unattributed" },
        "not_restored": 0
    }))
    .expect("kept has a default");
    assert_eq!(old.kept, 0);
    assert_eq!(serde_json::to_value(&old).unwrap()["kept"], json!(0));
}

#[test]
fn tmc012_wire_the_capability_name_and_the_caps() {
    assert_eq!(crate::methods::CAP_TM_OVERLAP.name, "timemachine.overlap");
    assert!(
        crate::capability::all().any(|c| c.name == "timemachine.overlap"),
        "the timemachine group serves the capability"
    );
    assert_eq!(MAX_OVERLAP_CONFLICTS, 50);
    assert_eq!(MAX_KEPT_PATHS, 200);
}

/// SEC-12: over MCP the kept paths are a number and no path or branch name travels.
#[test]
fn mcp_gets_the_reason_without_paths() {
    let done: UndoResult = serde_json::from_value(json!({
        "operation_id": "op-3",
        "prior_snapshot_id": "s-1",
        "undone_operation_id": "op-1",
        "target_snapshot_id": "s-0",
        "requester": {
            "actor": { "actor": "unattributed" },
            "channel": "mcp",
            "via": "none",
            "confirmable": false
        },
        "written": 1,
        "removed": 0,
        "not_restored": [],
        "warnings": [],
        "kept": [
            { "worktree": { "untrusted": "wt-secret-worktree" }, "path": { "untrusted": "secret/path/one.rs" } },
            { "worktree": { "untrusted": "wt-secret-worktree" }, "path": { "untrusted": "secret/path/two.rs" } }
        ]
    }))
    .expect("a result with kept parses");

    let mcp = done.for_mcp();

    assert_eq!(mcp.kept, 2, "kept travels as a count");
    let text = serde_json::to_string(&mcp).unwrap();
    for secret in ["secret/path", "one.rs", "two.rs", "wt-secret-worktree"] {
        assert!(
            !text.contains(secret),
            "{secret} reached the MCP result: {text}"
        );
    }

    let rejected = TmRejectedData {
        reason: TmRejectReason::Overlap,
        operation_id: Some("op-2".into()),
    };
    let value = serde_json::to_value(&rejected).unwrap();
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["operation_id", "reason"], "only the reason: {value}");
    assert_eq!(value["reason"], json!("overlap"));
}
