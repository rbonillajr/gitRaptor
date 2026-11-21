//! US-WTL-005 (T001): `WorktreeView::origin`, its closed JSON form and its
//! capability `observation.worktree-origin`. The views are built from JSON on
//! purpose: the field is read and written through the wire form.

use gitraptor_api::capability;
use gitraptor_api::messages::WorktreeView;
use gitraptor_api::methods::GROUPS;
use serde_json::{Value, json};

const CAPABILITY: &str = "observation.worktree-origin";

fn worktree_json(origin: Option<Value>) -> Value {
    let mut w = json!({
        "path": {"untrusted": "/w/feat"},
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
    w
}

fn parse(origin: Value) -> Result<WorktreeView, serde_json::Error> {
    serde_json::from_value(worktree_json(Some(origin)))
}

#[test]
fn worktree_origin_is_a_capability_of_observation() {
    let observation = GROUPS
        .iter()
        .find(|g| g.module == "observation")
        .expect("observation group");
    assert!(
        observation
            .capabilities
            .iter()
            .any(|c| c.name == CAPABILITY),
        "{CAPABILITY} is not listed in the observation group"
    );
    assert!(
        capability::all().any(|c| c.name == CAPABILITY),
        "{CAPABILITY} is not in the capability catalog"
    );
}

#[test]
fn origin_wire_form_round_trips() {
    for origin in [
        json!({"tool": "codex", "evidence": "path-pattern", "hands_off": "detached"}),
        json!({"tool": "orca", "evidence": "admin-marker"}),
        json!({"tool": "unknown", "evidence": "none"}),
        json!({"tool": "claude-code", "evidence": "path-pattern", "hands_off": "locked"}),
        json!({"tool": "worktrunk", "evidence": "path-pattern", "hands_off": "unreadable"}),
        json!({"tool": "gitraptor", "evidence": "registered"}),
        json!({"tool": "gitkraken", "evidence": "admin-marker"}),
    ] {
        let view = parse(origin.clone())
            .unwrap_or_else(|e| panic!("WorktreeView must accept origin {origin}: {e}"));
        let out = serde_json::to_value(&view).unwrap();
        assert_eq!(
            out["origin"], origin,
            "origin must serialize as {{tool, evidence, hands_off?}}: {out}"
        );
    }
}

#[test]
fn a_worktree_without_origin_keeps_the_json_of_before() {
    let view: WorktreeView = serde_json::from_value(worktree_json(None)).unwrap();
    let out = serde_json::to_value(&view).unwrap();
    assert!(
        out.get("origin").is_none(),
        "no origin must mean no `origin` key (not null): {out}"
    );
    // The field exists: a worktree with one reads it back.
    parse(json!({"tool": "orca", "evidence": "admin-marker"}))
        .expect("WorktreeView must accept an origin");
}

#[test]
fn unknown_fields_of_origin_are_rejected() {
    // Control: the same origin without the extra field is accepted.
    parse(json!({"tool": "orca", "evidence": "admin-marker"}))
        .expect("WorktreeView must accept an origin");
    assert!(
        parse(json!({"tool": "orca", "evidence": "admin-marker", "since": 1})).is_err(),
        "an origin with an unknown field must be rejected"
    );
    assert!(
        parse(json!({"tool": "emacs", "evidence": "admin-marker"})).is_err(),
        "an unknown tool name must be rejected"
    );
    assert!(
        parse(json!({"tool": "orca", "evidence": "admin-marker", "hands_off": "sleepy"})).is_err(),
        "an unknown hands_off reason must be rejected"
    );
}

#[test]
fn inconsistent_origins_are_rejected() {
    // Control: a coherent origin is accepted, so the rejections below are the rule's.
    parse(json!({"tool": "codex", "evidence": "path-pattern"}))
        .expect("WorktreeView must accept an origin");
    for (name, origin) in [
        (
            "unknown with evidence",
            json!({"tool": "unknown", "evidence": "path-pattern"}),
        ),
        (
            "unknown with admin-marker",
            json!({"tool": "unknown", "evidence": "admin-marker"}),
        ),
        (
            "a tool without evidence",
            json!({"tool": "codex", "evidence": "none"}),
        ),
        (
            "gitraptor without registered",
            json!({"tool": "gitraptor", "evidence": "path-pattern"}),
        ),
        (
            "gitraptor with hands_off",
            json!({"tool": "gitraptor", "evidence": "registered", "hands_off": "detached"}),
        ),
    ] {
        assert!(
            parse(origin.clone()).is_err(),
            "{name} must be rejected on deserialize: {origin}"
        );
    }
}
