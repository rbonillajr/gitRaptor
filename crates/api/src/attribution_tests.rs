//! Tests of the contract of the correction of an attribution (US-GRP-010, T001). Child module of
//! `attribution`, attached there with `#[cfg(test)] #[path = "attribution_tests.rs"] mod tests;`.

use super::*;
use crate::messages::DeclaredAgent;
use crate::methods::{
    ATTRIBUTION_CORRECT, ATTRIBUTION_REJECTED, ATTRIBUTION_WITHDRAW, FIRST_ERROR_BLOCK, GROUPS,
    spec,
};
use serde_json::json;

/// GRP010-API-RESERVED: BR-AUTH-001 and SEC-14.
#[test]
fn attribution_methods_are_reserved_and_not_mcp() {
    for name in [ATTRIBUTION_CORRECT, ATTRIBUTION_WITHDRAW] {
        let m = spec(name).expect("the method is declared");
        assert!(m.reserved, "{name} must be a reserved command");
        assert!(!m.mcp, "{name} must not be offered to raptor-mcp");
        assert_eq!(
            m.implemented_by, None,
            "{name} is implemented now: it must stop being declared ahead of its story"
        );
    }
}

/// GRP010-API-BLOCK: ADR-GRP-016 § 3, the next free block is -33160.
#[test]
fn attribution_rejected_is_in_its_block() {
    assert_eq!(ATTRIBUTION_REJECTED.code, -33160);
    assert_eq!(ATTRIBUTION_REJECTED.name, "attribution-rejected");
    let group = GROUPS
        .iter()
        .find(|g| g.module == "attribution")
        .expect("the attribution module is a group");
    assert_eq!(
        group.error_block,
        Some(FIRST_ERROR_BLOCK - 8 * crate::methods::ERROR_BLOCK_LEN),
        "the attribution module must own the block -33160"
    );
    assert!(
        group.owns(ATTRIBUTION_REJECTED.code),
        "ATTRIBUTION_REJECTED must be inside the block of its module"
    );
    assert!(
        group.errors.contains(&ATTRIBUTION_REJECTED),
        "the group must list ATTRIBUTION_REJECTED in `errors`"
    );
}

/// GRP010-API-WIRE
#[test]
fn correct_params_wire_form() {
    let params: AttributionCorrectParams = serde_json::from_value(json!({
        "agent": {"kind": "other", "name": "Codex"},
        "worktree": "/work/feat-login"
    }))
    .expect("agent and worktree are enough");
    assert_eq!(
        params.agent,
        DeclaredAgent::Other {
            name: "Codex".into()
        }
    );
    assert_eq!(params.worktree, "/work/feat-login");
    assert_eq!(params.session_id, None, "session_id is optional");

    let with_session: AttributionCorrectParams = serde_json::from_value(json!({
        "agent": {"kind": "claude-code"},
        "worktree": "/w",
        "session_id": "reg:1:1"
    }))
    .unwrap();
    assert_eq!(with_session.session_id.as_deref(), Some("reg:1:1"));

    assert!(
        serde_json::from_value::<AttributionCorrectParams>(json!({
            "agent": {"kind": "claude-code"}, "worktree": "/w", "extra": 1
        }))
        .is_err(),
        "unknown fields must be refused (deny_unknown_fields)"
    );
    assert!(
        serde_json::from_value::<AttributionCorrectParams>(json!({
            "agent": {"kind": "claude-code"}
        }))
        .is_err(),
        "worktree is required (D8)"
    );
    assert!(
        serde_json::from_value::<AttributionWithdrawParams>(json!({
            "worktree": "/w", "agent": {"kind": "claude-code"}
        }))
        .is_err(),
        "the withdrawal takes no agent and refuses unknown fields"
    );
    let withdraw: AttributionWithdrawParams =
        serde_json::from_value(json!({"worktree": "/w"})).unwrap();
    assert_eq!(withdraw.session_id, None);

    // The rejection reasons are kebab-case.
    assert_eq!(
        serde_json::to_value(AttributionRejection::RegisteredSession).unwrap(),
        json!("registered-session")
    );
    assert_eq!(
        serde_json::to_value(AttributionRejection::SameAgent).unwrap(),
        json!("same-agent")
    );
    assert_eq!(
        serde_json::to_value(AttributionRejection::NoDetectedAttribution).unwrap(),
        json!("no-detected-attribution")
    );

    // `candidates` is omitted when empty and listed otherwise.
    let bare = AttributionRejectedData {
        reason: AttributionRejection::SameAgent,
        candidates: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(&bare).unwrap(),
        json!({"reason": "same-agent"}),
        "an empty candidates list must not be serialized"
    );
    let candidate: SessionCandidate = serde_json::from_value(json!({
        "session_id": "1:2",
        "actor": {"actor": "agent", "kind": "claude-code", "origin": "detected"},
        "started_utc_ms": 5,
        "present": true
    }))
    .unwrap();
    let listed = AttributionRejectedData {
        reason: AttributionRejection::AmbiguousSession,
        candidates: vec![candidate],
    };
    let value = serde_json::to_value(&listed).unwrap();
    assert_eq!(value["reason"], "ambiguous-session");
    assert_eq!(value["candidates"][0]["session_id"], "1:2");
    assert_eq!(MAX_SESSION_CANDIDATES, 10);
}
