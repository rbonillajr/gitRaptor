//! How the channel's rejection reasons of a write become tool codes for the guardrails "ask"
//! (US-MCP-013). Dedicated file (wired from `engine.rs` by one line): the criteria's tests must
//! not live in the production file.

use gitraptor_api::mcp_view::McpToolError;
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_core::client::ClientError;
use serde_json::{Value, json};

use super::snapshot_refusal;

fn rejected(data: Value) -> ClientError {
    ClientError::Rpc(
        ErrorObject::new(code::OPERATION_REJECTED, "operation rejected").with_data(data),
    )
}

#[test]
fn a_guardrails_ask_is_confirmation_required_without_params() {
    let r = snapshot_refusal(&rejected(
        json!({"reason": "guardrails-confirmation-required"}),
    ));
    assert_eq!(r.code, McpToolError::ConfirmationRequired);
    assert!(r.params.is_none(), "{:?}", r.params);
}

#[test]
fn a_guardrails_deny_is_never_confirmation_required() {
    let r = snapshot_refusal(&rejected(json!({"reason": "guardrails-denied"})));
    assert_eq!(r.code, McpToolError::Internal);
    assert!(r.params.is_none(), "{:?}", r.params);
}

#[test]
fn an_undecodable_reason_stays_a_generic_refusal() {
    for data in [
        json!({"reason": "something-a-newer-daemon-says"}),
        json!({"reason": 5}),
        json!({}),
    ] {
        let r = snapshot_refusal(&rejected(data.clone()));
        assert_eq!(r.code, McpToolError::Internal, "{data}");
        assert!(r.params.is_none(), "{data}: {:?}", r.params);
    }
}
