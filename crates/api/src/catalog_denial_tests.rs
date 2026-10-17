//! D10 (the PO's adjustment): the MCP projection of a denied plan carries no message, content,
//! `configStatus`, `configRef` nor exception field, and its parameters are only of kind branch,
//! path or base (the operation of the agent itself). Child module of `catalog`, attached there
//! with `#[cfg(test)] #[path = "catalog_denial_tests.rs"] mod denial_tests;`.

use super::*;
use crate::Actor;
use crate::guard::{Effect, Level, Param, ParamKind, Reason, Rule};
use crate::timemachine::{RequestChannel, ResolvedVia};

const SECRET_TRAILER: &str = "Co-Authored-By: Someone Secret <secret@example.com>";
const SECRET_PATTERN: &str = "internal-secrets/**";
const REMOTE: &str = "https://example.com/private/repo.git";

fn denied_plan() -> PrepareResult {
    let reason = |rule, params| Reason {
        rule,
        level: Level::Minimum,
        cause: None,
        params,
    };
    PrepareResult {
        plan_id: "plan-1".into(),
        catalog_version: CATALOG_VERSION,
        operation: OperationId::Commit,
        layer: Layer::Mcp,
        requester: RequesterView {
            actor: Actor::Unattributed,
            channel: RequestChannel::Tui,
            via: ResolvedVia::None,
            confirmable: false,
        },
        fingerprint: "f".repeat(64),
        warnings: Vec::new(),
        decision: DecisionView::Deny,
        challenge: None,
        expires_in_ms: PLAN_TTL_MS,
        diagnostics: Vec::new(),
        guardrails: Some(GuardrailsDenial {
            decision_id: "d-1".into(),
            effect: Effect::Deny,
            applied_effect: Effect::Deny,
            reasons: vec![
                reason(
                    Rule::ProtectedBranch,
                    vec![
                        Param::new(ParamKind::Branch, "main"),
                        Param::new(ParamKind::Base, "main"),
                        Param::new(ParamKind::Pattern, SECRET_PATTERN),
                    ],
                ),
                reason(
                    Rule::ForbiddenPath,
                    vec![
                        Param::new(ParamKind::Path, "internal-secrets/key.pem"),
                        Param::new(ParamKind::Pattern, SECRET_PATTERN),
                        Param::new(ParamKind::Example, SECRET_TRAILER),
                        Param::new(ParamKind::Remote, REMOTE),
                        Param::new(ParamKind::Oid, "a".repeat(40)),
                    ],
                ),
            ],
        }),
    }
}

/// Every key at any depth of `v`.
fn keys(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                keys(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| keys(i, out)),
        _ => {}
    }
}

#[test]
fn the_mcp_projection_carries_no_message_content_or_config_text() {
    let mcp = denied_plan().for_mcp();
    let shown = serde_json::to_value(&mcp).unwrap();
    let denial = &shown["guardrails"];
    assert_eq!(denial["decisionId"], "d-1", "the denial travels: {shown}");
    assert_eq!(denial["reasons"][0]["rule"], "policy.protected-branch");

    // No message, content, configuration text or exception field, at any depth.
    let mut found = Vec::new();
    keys(&shown, &mut found);
    for forbidden in [
        "message",
        "content",
        "configStatus",
        "configRef",
        "exception",
        "exceptionState",
        "challenge",
    ] {
        assert!(
            !found.iter().any(|k| k == forbidden),
            "{forbidden}: {shown}"
        );
    }
    let text = shown.to_string();
    for secret in [SECRET_TRAILER, SECRET_PATTERN, REMOTE, "raptor guard exec"] {
        assert!(!text.contains(secret), "{secret:?} leaked: {text}");
    }

    // The parameters describe the agent's own operation: a branch, a path, a base.
    let mut kinds = 0;
    for reason in denial["reasons"].as_array().unwrap() {
        for param in reason["params"].as_array().into_iter().flatten() {
            let kind = param["kind"].as_str().unwrap();
            assert!(
                ["branch", "base", "path"].contains(&kind),
                "param of kind {kind}: {param}"
            );
            kinds += 1;
        }
    }
    assert!(
        kinds >= 3,
        "the branch, the base and the path are kept: {shown}"
    );
}
