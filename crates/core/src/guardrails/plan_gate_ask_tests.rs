//! MCP013-GATE-VIEW (D7): what the executor sees of a decision. Child module of
//! `guardrails::plan_gate`, attached there with `#[cfg(test)] #[path = "plan_gate_ask_tests.rs"] mod ask_tests;`.

use super::*;
use crate::executor::GateDecision;
use gitraptor_api::guard::{Decision, Effect, ExceptionState, Level, Reason, Rule};

fn decision(effect: Effect, applied_effect: Effect) -> Decision {
    Decision {
        decision_id: "d".into(),
        effect,
        applied_effect,
        reasons: vec![Reason {
            rule: Rule::ProtectedBranch,
            level: Level::Minimum,
            cause: None,
            params: vec![],
        }],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    }
}

#[test]
fn a_decision_that_asks_and_is_applied_as_deny_is_viewed_as_ask() {
    let view = |effect, applied| view_of(&decision(effect, applied));
    assert_eq!(view(Effect::Ask, Effect::Deny), GateDecision::Ask);
    assert_eq!(view(Effect::Allow, Effect::Allow), GateDecision::Allow);
    assert_eq!(view(Effect::Deny, Effect::Deny), GateDecision::Deny);
    for (effect, applied) in [
        (Effect::Ask, Effect::Ask),
        (Effect::Ask, Effect::Allow),
        (Effect::Allow, Effect::Deny),
        (Effect::Allow, Effect::Ask),
        (Effect::Deny, Effect::Allow),
        (Effect::Deny, Effect::Ask),
    ] {
        assert_eq!(
            view(effect, applied),
            GateDecision::Deny,
            "{effect:?}/{applied:?}"
        );
    }
}
