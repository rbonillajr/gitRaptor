//! US-MCP-013 T001: the guardrails "ask" rejection has its own name on the wire, distinct from
//! the Cockpit challenge's `confirmation-required`, and the tool code exists in kebab-case.

use gitraptor_api::catalog::RejectReason;
use gitraptor_api::mcp_view::McpToolError;
use serde_json::json;

#[test]
fn guardrails_confirmation_required_is_its_own_wire_reason() {
    let ask = serde_json::to_value(RejectReason::GuardrailsConfirmationRequired).unwrap();
    let challenge = serde_json::to_value(RejectReason::ConfirmationRequired).unwrap();
    assert_eq!(ask, json!("guardrails-confirmation-required"));
    assert_eq!(challenge, json!("confirmation-required"));
    assert_ne!(ask, challenge, "a client must tell the two outcomes apart");
    let back: RejectReason = serde_json::from_value(ask).unwrap();
    assert_eq!(back, RejectReason::GuardrailsConfirmationRequired);

    assert_eq!(
        McpToolError::ConfirmationRequired.as_str(),
        "confirmation-required"
    );
    assert!(McpToolError::ALL.contains(&McpToolError::ConfirmationRequired));
}
