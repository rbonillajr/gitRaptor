//! US-WTL-005 (T009): the app's backend asks for `observation.worktree-origin` on its link and
//! hands the origin of a linked worktree to the webview in the `WorktreeDto` it already sends
//! (camelCase, `null` without origin), with no new IPC command (ADR-GRP-017 § 1). The views are
//! built from JSON on purpose: the field is read through the wire form.

use std::path::Path;

use gitraptor_api::messages::WorktreeView;
use gitraptor_desktop::engine::ingest;
use gitraptor_desktop::engine::link::REQUIRED_CAPABILITIES;
use serde_json::{Value, json};

fn view(origin: Option<Value>) -> WorktreeView {
    let mut w = json!({
        "path": {"untrusted": "/w/shop/feat"},
        "main": false,
        "admin_name": null,
        "status": {
            "state": "ready",
            "head": {"kind": "detached"},
            "counts": {"staged": 0, "unstaged": 0, "untracked": 0},
            "changes": [],
            "divergence": {"state": "no-commits"}
        }
    });
    if let Some(origin) = origin {
        w["origin"] = origin;
    }
    serde_json::from_value(w).expect("WorktreeView must accept origin")
}

fn dto(origin: Option<Value>) -> Value {
    serde_json::to_value(ingest::worktree(&view(origin))).unwrap()
}

#[test]
fn the_link_asks_for_the_capability() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&"observation.worktree-origin"),
        "D9: REQUIRED_CAPABILITIES must list observation.worktree-origin: {REQUIRED_CAPABILITIES:?}"
    );
}

#[test]
fn the_dto_translates_the_three_origins_to_camel_case() {
    let cases = [
        (
            json!({"tool": "codex", "evidence": "path-pattern", "hands_off": "detached"}),
            json!({"tool": "codex", "evidence": "path-pattern", "handsOff": "detached"}),
        ),
        (
            json!({"tool": "orca", "evidence": "admin-marker"}),
            json!({"tool": "orca", "evidence": "admin-marker", "handsOff": null}),
        ),
        (
            json!({"tool": "unknown", "evidence": "none"}),
            json!({"tool": "unknown", "evidence": "none", "handsOff": null}),
        ),
        (
            json!({"tool": "claude-code", "evidence": "path-pattern", "hands_off": "unreadable"}),
            json!({"tool": "claude-code", "evidence": "path-pattern", "handsOff": "unreadable"}),
        ),
    ];
    for (wire, expected) in cases {
        assert_eq!(
            dto(Some(wire.clone()))["origin"],
            expected,
            "WorktreeDto.origin of {wire}"
        );
    }
}

#[test]
fn the_dto_has_null_origin_without_origin() {
    let out = dto(None);
    assert!(
        out.as_object().unwrap().contains_key("origin"),
        "WorktreeDto must always carry the `origin` key: {out}"
    );
    assert_eq!(out["origin"], Value::Null);
    // Control: the same view with an origin does carry it, so the null is the rule's.
    assert_ne!(
        dto(Some(json!({"tool": "orca", "evidence": "admin-marker"})))["origin"],
        Value::Null
    );
}

#[test]
fn origin_adds_no_ipc_command() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("commands.json"))
        .unwrap();
    let commands: Vec<String> = serde_json::from_str(&text).unwrap();
    assert!(
        commands.iter().all(|c| !c.contains("origin")),
        "ADR-GRP-017 § 1: no IPC command for the origin: {commands:?}"
    );
    // The origin travels in the DTO the app already sends.
    assert_ne!(
        dto(Some(json!({"tool": "orca", "evidence": "admin-marker"})))["origin"],
        Value::Null,
        "the origin must reach the webview through WorktreeDto"
    );
}
