//! How the engine's refusals of the full `status` become tool codes: a repo or worktree that cannot
//! be read says why and nothing else, a reason this binary does not know is a repo that cannot
//! be read, and a cursor the engine does not know is an invalid one. Dedicated file (wired from
//! `engine.rs` by one line): the criteria's tests must not live in the production file.

use gitraptor_api::mcp_view::McpToolError;
use gitraptor_api::methods::{MCP_UNAVAILABLE, McpUnavailable, McpUnavailableData};
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_core::client::ClientError;
use serde_json::json;

use super::refusal;

fn unavailable(data: impl serde::Serialize) -> ClientError {
    ClientError::Rpc(ErrorObject::new(MCP_UNAVAILABLE.code, "mcp-unavailable").with_data(data))
}

/// D7: each reason of the channel is a code of the tool; the one that is only about the repo
/// not being readable is the code the tool already had.
#[test]
fn unavailable_reasons_map_to_their_tool_codes() {
    for (reason, expected) in [
        (
            McpUnavailable::WorktreeMissing,
            McpToolError::WorktreeMissing,
        ),
        (McpUnavailable::OtherOwner, McpToolError::RepoOtherOwner),
        (
            McpUnavailable::WorktreeUntrusted,
            McpToolError::WorktreeUntrusted,
        ),
        (
            McpUnavailable::RepoUnreadable,
            McpToolError::RepoUnavailable,
        ),
    ] {
        assert_eq!(
            refusal(&unavailable(McpUnavailableData { reason })),
            expected,
            "{reason:?}"
        );
    }
    // As the daemon writes them on the wire.
    for (text, expected) in [
        ("worktree-missing", McpToolError::WorktreeMissing),
        ("other-owner", McpToolError::RepoOtherOwner),
        ("worktree-untrusted", McpToolError::WorktreeUntrusted),
        ("repo-unreadable", McpToolError::RepoUnavailable),
    ] {
        assert_eq!(refusal(&unavailable(json!({"reason": text}))), expected);
    }
}

/// D7, fail closed: a reason this binary cannot read, or no reason at all, is a repo that
/// cannot be read, never the generic internal error and never a guess.
#[test]
fn an_unknown_unavailable_reason_fails_closed() {
    for data in [
        json!({"reason": "something-a-newer-daemon-says"}),
        json!({"reason": 5}),
        json!({"reason": null}),
        json!({}),
        json!("worktree-missing"),
        json!(null),
    ] {
        assert_eq!(
            refusal(&unavailable(data.clone())),
            McpToolError::RepoUnavailable,
            "{data}"
        );
    }
    let without_data = ClientError::Rpc(ErrorObject::new(MCP_UNAVAILABLE.code, "mcp-unavailable"));
    assert_eq!(refusal(&without_data), McpToolError::RepoUnavailable);
}

/// D7: a cursor the daemon does not know (another connection's, another repo's, or none that was
/// ever given) and one it finds malformed are the same for the model: it is not valid for this
/// session, and it asks for `status` again.
#[test]
fn an_unknown_cursor_is_invalid_cursor() {
    for c in [code::NOT_FOUND, code::INVALID_PARAMS] {
        let err = ClientError::Rpc(ErrorObject::new(c, "x"));
        assert_eq!(refusal(&err), McpToolError::InvalidCursor, "{c}");
    }
    // What is not about the cursor keeps its own code.
    let limited = ClientError::Rpc(ErrorObject::new(code::RATE_LIMITED, "rate limited"));
    assert_eq!(refusal(&limited), McpToolError::RateLimited);
}
