use clap::Parser;
use gitraptor_api::cli_contract::{CLI_SCHEMA_VERSION, CliErrorCode, CliOnlyCode};
use gitraptor_api::mcp_view::McpToolError;
use gitraptor_api::rpc::{ErrorObject, ScopeRefusal, ScopeRefusedData, code};
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_core::client::ClientError;
use serde_json::{Value, json};

use super::*;
use crate::i18n;

fn rpc(code: i64, data: Option<Value>) -> ClientError {
    let mut err = ErrorObject::new(code, "daemon text that must never be forwarded");
    err.data = data;
    ClientError::Rpc(err)
}

#[test]
fn every_cli_code_has_message_and_action_in_both_languages() {
    for code in CliErrorCode::ALL {
        for part in ["message", "action"] {
            let key = format!("jsonerr.{}.{part}", code.as_str());
            for spanish in [false, true] {
                let text = i18n::text_in(spanish, &key);
                assert!(
                    text.is_some_and(|t| !t.trim().is_empty()),
                    "{key} is missing in {}",
                    if spanish { "es" } else { "en" }
                );
            }
        }
    }
}

#[test]
fn client_errors_map_to_stable_codes() {
    let tool = CliErrorCode::Tool;
    let cli = CliErrorCode::Cli;
    let refused = |reason| json!({ "reason": reason });
    let cases: Vec<(ClientError, bool, CliErrorCode, Option<Value>)> = vec![
        (
            ClientError::NotRunning,
            false,
            tool(McpToolError::EngineUnavailable),
            None,
        ),
        (
            ClientError::StartTimeout,
            true,
            tool(McpToolError::EngineUnavailable),
            None,
        ),
        (
            ClientError::Protocol("bad"),
            false,
            tool(McpToolError::Internal),
            None,
        ),
        (
            rpc(
                code::SCOPE_REFUSED,
                serde_json::to_value(ScopeRefusedData {
                    reason: ScopeRefusal::NotAllowlisted,
                })
                .ok(),
            ),
            true,
            tool(McpToolError::RepoNotEnabled),
            None,
        ),
        (
            rpc(
                code::SCOPE_REFUSED,
                serde_json::to_value(ScopeRefusedData {
                    reason: ScopeRefusal::NotObserved,
                })
                .ok(),
            ),
            true,
            tool(McpToolError::NotInObservedWorktree),
            None,
        ),
        (
            rpc(code::RATE_LIMITED, None),
            true,
            tool(McpToolError::RateLimited),
            None,
        ),
        (
            rpc(methods::MCP_UNAVAILABLE.code, None),
            true,
            tool(McpToolError::RepoUnavailable),
            None,
        ),
        (
            rpc(
                code::RESERVED_REFUSED,
                Some(refused("not-available-to-mcp")),
            ),
            true,
            cli(CliOnlyCode::ReservedCommand),
            Some(json!({"reason": "not-available-to-mcp"})),
        ),
        (
            rpc(code::RESERVED_REFUSED, Some(refused("identity-unverified"))),
            true,
            tool(McpToolError::IdentityUnverified),
            None,
        ),
        (
            rpc(code::METHOD_NOT_FOUND, None),
            true,
            cli(CliOnlyCode::CommandUnavailable),
            Some(json!({"reason": "agent-view"})),
        ),
        (
            rpc(code::NOT_IMPLEMENTED, None),
            false,
            cli(CliOnlyCode::CommandUnavailable),
            Some(json!({"reason": "engine-version"})),
        ),
        (
            rpc(code::INVALID_PARAMS, None),
            false,
            cli(CliOnlyCode::InvalidArgs),
            Some(json!({"kind": "invalid-params"})),
        ),
        (
            rpc(code::PRIOR_SNAPSHOT_FAILED, None),
            false,
            tool(McpToolError::Aborted),
            None,
        ),
        (
            rpc(code::REPO_REJECTED, Some(refused("not-a-repo"))),
            false,
            cli(CliOnlyCode::CommandFailed),
            Some(json!({"reason": "repo-rejected", "detail": "not-a-repo"})),
        ),
        (
            rpc(code::GUARD_REJECTED, Some(refused("Not A Stable Name"))),
            false,
            cli(CliOnlyCode::CommandFailed),
            Some(json!({"reason": "guard-rejected"})),
        ),
    ];
    for (err, restricted, want_code, want_params) in cases {
        let (got_code, got_params) = from_client_error(&err, restricted);
        assert_eq!(got_code, want_code, "{err:?}");
        assert_eq!(got_params, want_params, "{err:?}");
    }
}

#[test]
fn an_overlap_is_a_confirmation_and_any_other_rejection_is_not() {
    let overlap = json!({"reason": TmRejectReason::Overlap});
    let (got, params) =
        from_client_error(&rpc(code::OPERATION_REJECTED, Some(overlap.clone())), true);
    assert_eq!(got, CliErrorCode::Tool(McpToolError::ConfirmationRequired));
    assert_eq!(params, Some(overlap));

    let (got, _) = from_client_error(
        &rpc(
            code::OPERATION_REJECTED,
            Some(json!({"reason": "nothing-to-undo"})),
        ),
        true,
    );
    assert_eq!(got, CliErrorCode::Cli(CliOnlyCode::CommandFailed));
}

#[test]
fn one_line_and_nothing_on_stderr() {
    let ok = render_ok(&json!({"worktree": {"untrusted": "shop"}})).expect("serializes");
    let failed = render_fail(
        CliErrorCode::Cli(CliOnlyCode::ReservedCommand),
        Some(json!({"reason": "agent-ancestry"})),
    );
    for line in [ok.as_str(), failed.as_str(), INTERNAL_LITERAL] {
        assert!(!line.contains('\n'), "a single line: {line:?}");
        let value: Value = serde_json::from_str(line).expect("the line is JSON");
        assert_eq!(value["schema_version"], CLI_SCHEMA_VERSION);
        assert!(value.get("data").is_some() ^ value.get("error").is_some());
    }
    let error: Value = serde_json::from_str(&failed).expect("JSON");
    assert_eq!(error["error"]["code"], "reserved-command");
    assert!(error["error"]["params"].is_object());
    // The envelope and the fixed fallback agree: same fields as a rendered rejection.
    let internal: Value = serde_json::from_str(INTERNAL_LITERAL).expect("JSON");
    let rendered = render_fail(CliErrorCode::Tool(McpToolError::Internal), None);
    let rendered: Value = serde_json::from_str(&rendered).expect("JSON");
    assert_eq!(internal["error"]["code"], rendered["error"]["code"]);
    assert_eq!(internal["error"]["params"], rendered["error"]["params"]);
}

#[test]
fn untrusted_params_are_escaped() {
    let hostile = "wt-\u{1b}[31mred\u{202e}x\u{200b}y";
    let params = json!({"path": {"untrusted": hostile}});
    let failed = render_fail(
        CliErrorCode::Tool(McpToolError::RepoUnavailable),
        Some(params),
    );
    let ok = render_ok(&json!({"worktree": {"untrusted": hostile}})).expect("serializes");
    for line in [failed, ok] {
        assert!(!line.as_bytes().contains(&0x1b), "{line:?}");
        for hidden in ['\u{202e}', '\u{200b}'] {
            assert!(!line.contains(hidden), "{line:?}");
        }
        assert!(!line.contains('\n'));
    }
}

#[test]
fn a_non_object_params_becomes_an_empty_object() {
    let line = render_fail(
        CliErrorCode::Tool(McpToolError::Internal),
        Some(json!("a string")),
    );
    let value: Value = serde_json::from_str(&line).expect("JSON");
    assert_eq!(value["error"]["params"], json!({}));
}

fn usage_of(args: &[&str]) -> (clap::Error, String) {
    let err = crate::Cli::try_parse_from(args)
        .err()
        .expect("a usage error");
    let params = {
        let mut params = json!({ "kind": kind_name(err.kind()) });
        if let Some(argument) = defined_argument(&err) {
            params["argument"] = json!(argument);
        }
        params.to_string()
    };
    (err, params)
}

#[test]
fn a_usage_error_names_the_kind_and_never_the_typed_value() {
    let (_, params) = usage_of(&["raptor", "status", "--json", "--no-such-flag-zz"]);
    assert!(params.contains("unknown-argument"), "{params}");
    assert!(!params.contains("zz"), "{params}");

    let (_, params) = usage_of(&["raptor", "--lang", "zz-typed", "status", "--json"]);
    assert!(params.contains("value-validation"), "{params}");
    assert!(params.contains("--lang"), "{params}");
    assert!(!params.contains("zz-typed"), "{params}");

    let (_, params) = usage_of(&["raptor", "no-such-command-zz", "--json"]);
    assert!(!params.contains("zz"), "{params}");
}

#[test]
fn kebab_case_of_a_kind() {
    assert_eq!(kind_name(ErrorKind::UnknownArgument), "unknown-argument");
    assert_eq!(kind_name(ErrorKind::InvalidUtf8), "invalid-utf8");
    assert_eq!(kind_name(ErrorKind::Io), "io");
}
