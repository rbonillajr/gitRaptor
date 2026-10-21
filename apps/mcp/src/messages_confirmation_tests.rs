//! The `confirmation-required` refusal of a write the team's policy answers "ask" (US-MCP-013).
//! Dedicated file (wired from `messages.rs` by one line): the criteria's tests must not live in
//! the production file.

use gitraptor_api::mcp_view::{MCP_REFUSAL_TOKENS, McpToolError, check_token_budget};

use super::{Lang, refusal};

#[test]
fn the_confirmation_refusal_says_what_the_story_says() {
    let en = refusal(McpToolError::ConfirmationRequired, Lang::En);
    assert_eq!(en["code"], "confirmation-required", "{en}");
    assert_eq!(
        en["message"], "This action requires the developer's confirmation.",
        "{en}"
    );
    assert_eq!(
        en["action"], "Tell the developer: the decision is theirs. Retrying gets the same answer.",
        "{en}"
    );

    let es = refusal(McpToolError::ConfirmationRequired, Lang::Es);
    assert_eq!(es["code"], "confirmation-required", "{es}");
    assert_eq!(
        es["message"], "Esta acción requiere confirmación del desarrollador.",
        "{es}"
    );
    assert_eq!(
        es["action"],
        "Avisa al desarrollador: la decisión es suya. Reintentar da la misma respuesta.",
        "{es}"
    );
}

#[test]
fn the_confirmation_refusal_has_no_params_and_fits_its_budget() {
    for lang in [Lang::En, Lang::Es] {
        let body = refusal(McpToolError::ConfirmationRequired, lang);
        assert!(body.get("params").is_none(), "{lang:?}: {body}");
        let what = format!("confirmation-required ({lang:?})");
        check_token_budget("RES-MCP-03", &what, &body.to_string(), MCP_REFUSAL_TOKENS).unwrap();
    }
}
