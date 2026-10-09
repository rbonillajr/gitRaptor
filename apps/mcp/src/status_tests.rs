//! The full `status` as the tool sends it: the cursor argument, the refusals of a repo or worktree
//! that cannot be read, the size of every answer and page, the catalog entry and its compact
//! output schema, and the mark on every text that is not the binary's. Dedicated file (wired from
//! `server.rs` by one line): the criteria's tests must not live in the production file.
//!
//! Sizes are measured as estimated tokens of the compact JSON, with the repo's own helpers
//! (`check_token_budget`), on the part the model reads: the text and the structured content.

use gitraptor_api::guard::{Diagnostic, LossCause};
use gitraptor_api::mcp_view::{
    MCP_FIT_BYTES, MCP_PAGE_TOKENS, MCP_REFUSAL_TOKENS, MCP_STATUS_TOKENS, MCP_TOOL_TOKENS,
    catalog_overruns, check_token_budget, fit_page,
};
use gitraptor_api::messages::{SessionStateView, UnavailableReason};
use gitraptor_api::methods::{
    McpBase, McpBaseState, McpEngineState, McpGap, McpHere, McpListRef, McpPage,
    McpProtectionState, McpRepo, McpSession, McpUncounted, McpWorktree, valid_cursor,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted};
use serde_json::{Value, json};

use super::tests::conforms;
use super::*;

const CURSOR_A: &str = "9f2c4b1a7d3e5f60";
const CURSOR_B: &str = "41ab9c03d5e6f782";

fn list(total: u64, cursor: &str) -> McpListRef {
    McpListRef {
        total,
        cursor: cursor.to_owned(),
    }
}

fn agent(name: UntrustedName) -> Actor {
    Actor::Agent {
        kind: AgentKind::Other,
        name: Some(name),
        origin: AgentOrigin::Registered,
    }
}

fn session(name: UntrustedName, state: SessionStateView) -> McpSession {
    McpSession {
        actor: agent(name),
        state,
    }
}

fn repo_of(base: UntrustedName) -> McpRepo {
    McpRepo {
        engine: None,
        base: McpBase {
            name: Some(base),
            state: McpBaseState::Confirmed,
        },
        protection: McpProtectionState::Full,
        protection_lost: None,
        diagnostics: Vec::new(),
        fetch_age_s: Some(180),
        gaps: Vec::new(),
        gaps_total: None,
        sessions_unknown: false,
        worktrees: Some(list(3, CURSOR_B)),
    }
}

/// The typical default answer, with the three names of the caller given: its worktree, its
/// branch and the base of the repo. Three own changes, nothing ahead or behind, a confirmed base,
/// full protection, fetched 180 s ago, three more worktrees, no gaps and no other session.
fn default_with(worktree: UntrustedName, branch: UntrustedName, base: UntrustedName) -> McpStatus {
    McpStatus {
        repo_id: "f".repeat(64),
        repo_state: RepoStateView::Observed,
        worktree,
        branch: Some(branch),
        main: false,
        requester: Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        },
        action: None,
        here: Some(McpHere {
            changes: Some(list(3, CURSOR_A)),
            ..McpHere::default()
        }),
        repo: Some(repo_of(base)),
        page: None,
    }
}

/// The three references of RES-MCP-02: the typical repo, and the same with the three names at
/// their 100 characters, in ASCII and with accents (2 bytes each).
fn default_references() -> Vec<(&'static str, McpStatus)> {
    let same = |text: &str| {
        let name = UntrustedName::new(text.repeat(1024));
        default_with(name.clone(), name.clone(), name)
    };
    vec![
        (
            "the typical repo",
            default_with(
                UntrustedName::new("shop-feat-a"),
                UntrustedName::new("feat/login-form"),
                UntrustedName::new("main"),
            ),
        ),
        ("names at 100 ASCII characters", same("n")),
        ("names at 100 accented characters", same("é")),
    ]
}

fn page_status(page: McpPage) -> McpStatus {
    McpStatus {
        here: None,
        repo: None,
        page: Some(page),
        ..default_with(
            UntrustedName::new("shop-feat-a"),
            UntrustedName::new("feat/login-form"),
            UntrustedName::new("main"),
        )
    }
}

fn worktrees_page(items: Vec<McpWorktree>) -> McpPage {
    McpPage {
        of: None,
        total: items.len() as u64,
        worktrees: items,
        paths: Vec::new(),
        truncated: false,
        cursor: None,
    }
}

fn paths_page(of: UntrustedName, paths: Vec<Untrusted>) -> McpPage {
    McpPage {
        of: Some(of),
        total: 3000,
        worktrees: Vec::new(),
        paths,
        truncated: true,
        cursor: Some(CURSOR_A.to_owned()),
    }
}

/// A page of 8 typical worktrees: one session, 4 changes and 2 ahead each.
fn typical_worktrees_page() -> McpStatus {
    page_status(worktrees_page(
        (0..8)
            .map(|i| McpWorktree {
                name: UntrustedName::new(format!("shop-feat-{i}")),
                branch: Some(UntrustedName::new(format!("feat/login-form-{i}"))),
                main: false,
                unavailable: None,
                state: McpHere {
                    sessions: vec![session(
                        UntrustedName::new("claude-2"),
                        SessionStateView::Active,
                    )],
                    changes: Some(list(4, CURSOR_A)),
                    ahead: Some(2),
                    ..McpHere::default()
                },
            })
            .collect(),
    ))
}

/// A page of 32 paths of about 35 bytes.
fn typical_paths_page() -> McpStatus {
    page_status(paths_page(
        UntrustedName::new("shop-feat-a"),
        (0..32)
            .map(|i| Untrusted::new(format!("crates/core/src/module_{i:03}/lib.rs")))
            .collect(),
    ))
}

/// Characters that grow when escaped, at the bound of every name: U+202E is 3 bytes as the
/// replacement character it becomes.
fn hostile_name() -> UntrustedName {
    UntrustedName::new("\u{202e}".repeat(1024))
}

fn hostile_sessions() -> Vec<McpSession> {
    (0..8)
        .map(|_| session(hostile_name(), SessionStateView::Inactive))
        .collect()
}

fn hostile_default() -> McpStatus {
    let mut status = default_with(hostile_name(), hostile_name(), hostile_name());
    status.requester = agent(hostile_name());
    status.here = Some(McpHere {
        sessions: hostile_sessions(),
        sessions_total: Some(12),
        changes: Some(list(3000, CURSOR_A)),
        ..McpHere::default()
    });
    let repo = status.repo.as_mut().unwrap();
    repo.gaps = (1..=3)
        .map(|n| McpGap {
            from_s_ago: 86_000 - n,
            to_s_ago: Some(80_000 - n),
        })
        .collect();
    repo.gaps_total = Some(5);
    status
}

fn hostile_worktrees_page() -> McpStatus {
    page_status(worktrees_page(
        (0..8)
            .map(|_| McpWorktree {
                name: hostile_name(),
                branch: Some(hostile_name()),
                main: false,
                unavailable: None,
                state: McpHere {
                    sessions: hostile_sessions(),
                    sessions_total: Some(12),
                    changes: Some(list(4, CURSOR_A)),
                    ahead: Some(2),
                    ..McpHere::default()
                },
            })
            .collect(),
    ))
}

fn hostile_paths_page() -> McpStatus {
    page_status(paths_page(
        hostile_name(),
        (0..32).map(|_| Untrusted::new("\"".repeat(1024))).collect(),
    ))
}

/// The call as the model's client sees it.
fn wire(status: &McpStatus, lang: Lang) -> Value {
    serde_json::to_value(respond(status_value(status), lang)).unwrap()
}

fn parts(wire: &Value) -> [(&'static str, String); 2] {
    [
        (
            "text",
            wire["content"][0]["text"].as_str().unwrap().to_owned(),
        ),
        ("structured", wire["structuredContent"].to_string()),
    ]
}

fn args(value: Value) -> JsonObject {
    value.as_object().cloned().unwrap()
}

/// NFR-02 and ADR-MCP-001 § 4.2: the only argument is a cursor of the form the daemon hands out;
/// anything else is refused as `invalid-params` naming the field, before the engine is asked.
#[test]
fn a_malformed_cursor_is_refused_before_the_engine() {
    use crate::status::cursor_argument;

    assert_eq!(cursor_argument(None), Ok(None));
    assert_eq!(cursor_argument(Some(&args(json!({})))), Ok(None));
    assert_eq!(
        cursor_argument(Some(&args(json!({"cursor": CURSOR_A})))),
        Ok(Some(CURSOR_A.to_owned()))
    );
    for bad in [
        json!({"cursor": "xyz"}),
        json!({"cursor": ""}),
        json!({"cursor": "9F2C4B1A7D3E5F60"}),
        json!({"cursor": "9f2c4b1a7d3e5f6"}),
        json!({"cursor": "9f2c4b1a7d3e5f600"}),
        json!({"cursor": "9f2c4b1a7d3e5f6\u{e9}"}),
        json!({"cursor": 9_007_199_254_740_991u64}),
        json!({"cursor": [CURSOR_A]}),
        json!({"cursor": {"id": CURSOR_A}}),
    ] {
        assert_eq!(
            cursor_argument(Some(&args(bad.clone()))),
            Err("cursor"),
            "{bad}"
        );
    }
    // No other argument names a repo or anything else (NFR-02).
    assert_eq!(
        cursor_argument(Some(&args(json!({"repo": "/elsewhere"})))),
        Err("repo")
    );
    assert_eq!(
        cursor_argument(Some(&args(json!({"cursor": CURSOR_A, "path": "/x"})))),
        Err("path")
    );
    // What the model reads back: `-32602` with the field as untrusted text.
    let error = malformed("cursor");
    assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    assert_eq!(error.message, "invalid-params");
    assert_eq!(error.data, Some(json!({"field": {"untrusted": "cursor"}})));
}

/// RES-MCP-03 and D7: every refusal for a repo or worktree that cannot be read is
/// `{code, message, action}` of its own, within 80 tokens in English and in Spanish, and carries
/// nothing of the repo even when the engine layer handed some over.
#[test]
fn every_unavailable_refusal_fits_and_carries_no_repo_data() {
    let codes = [
        McpToolError::WorktreeMissing,
        McpToolError::RepoOtherOwner,
        McpToolError::WorktreeUntrusted,
        McpToolError::RepoUnavailable,
        McpToolError::InvalidCursor,
    ];
    let repo_data = json!({
        "worktree": "secret-worktree", "branch": "secret-branch",
        "path": "/home/someone/secret", "repo_id": "f".repeat(64), "reason": "other-owner",
    });
    let mut seen = std::collections::BTreeSet::new();
    for lang in [Lang::En, Lang::Es] {
        let generic = messages::refusal(McpToolError::Internal, lang);
        for code in codes {
            let refusal = ToolRefusal {
                code,
                params: Some(repo_data.clone()),
            };
            let result = serde_json::to_value(respond(Err(refusal), lang)).unwrap();
            assert_eq!(result["isError"], true, "{code:?}");
            assert!(result.get("structuredContent").is_none());
            let text = result["content"][0]["text"].as_str().unwrap();
            let body: Value = serde_json::from_str(text).unwrap();
            let mut keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(keys, ["action", "code", "message"], "{code:?}: {text}");
            assert_eq!(body["code"], code.as_str());
            let (message, action) = (body["message"].as_str(), body["action"].as_str());
            assert!(
                message.is_some_and(|m| !m.is_empty()),
                "{code:?} {lang:?}: {text}"
            );
            assert!(
                action.is_some_and(|a| !a.is_empty()),
                "{code:?} {lang:?}: {text}"
            );
            assert_ne!(
                body["message"], generic["message"],
                "{code:?} has no text of its own"
            );
            // The texts stay about the situation: never a way to widen Git's trust.
            for forbidden in ["secret", "someone", "safe.directory", "git config"] {
                assert!(!text.contains(forbidden), "{forbidden} in {text}");
            }
            assert!(!text.contains(&"f".repeat(64)));
            let what = format!("{} ({lang:?})", code.as_str());
            eprintln!("RES-MCP-03: {what}: {} B", text.len());
            check_token_budget("RES-MCP-03", &what, text, MCP_REFUSAL_TOKENS).unwrap();
            seen.insert((lang == Lang::Es, body["message"].to_string()));
        }
    }
    // One text per code and language.
    assert_eq!(seen.len(), 2 * codes.len());
}

/// RES-MCP-02: the default answer, as the tool sends it, fits 300 tokens in each part for the
/// typical repo and for the one with its three names at their bound, in ASCII and with accents.
#[test]
fn the_default_status_fits_its_token_budget() {
    for (what, status) in default_references() {
        let wire = wire(&status, Lang::En);
        assert_eq!(wire["isError"], false, "{what}: {wire}");
        let shown = &wire["structuredContent"];
        assert!(
            shown.get("here").is_some() && shown.get("repo").is_some(),
            "{what}: the answer lacks the caller's situation or the repo: {shown}"
        );
        for (part, body) in parts(&wire) {
            let what = format!("{what} ({part} part)");
            eprintln!("RES-MCP-02: {what}: {} B", body.len());
            check_token_budget("RES-MCP-02", &what, &body, MCP_STATUS_TOKENS).unwrap();
        }
    }
}

/// RES-MCP-02, amended: a page of 8 typical worktrees and one of 32 typical paths fit 800 tokens
/// in each part.
#[test]
fn every_page_fits_its_token_budget() {
    for (what, status) in [
        ("a page of 8 worktrees", typical_worktrees_page()),
        ("a page of 32 paths", typical_paths_page()),
    ] {
        let wire = wire(&status, Lang::En);
        assert_eq!(wire["isError"], false, "{what}: {wire}");
        assert!(
            wire["structuredContent"].get("page").is_some(),
            "{what}: the answer lacks its page: {wire}"
        );
        for (part, body) in parts(&wire) {
            let what = format!("{what} ({part} part)");
            eprintln!("RES-MCP-02: {what}: {} B", body.len());
            check_token_budget("RES-MCP-02", &what, &body, MCP_PAGE_TOKENS).unwrap();
        }
    }
}

/// D5: the worst a repo can make of an answer, with every text at its bound and growing when
/// escaped, still goes out whole in each part, and a page that had to be cut says so and gives a
/// cursor to go on.
#[test]
fn a_hostile_status_and_page_fit_each_part() {
    let mut worktrees = hostile_worktrees_page();
    let mut paths = hostile_paths_page();
    fit_page(&mut worktrees, MCP_FIT_BYTES);
    fit_page(&mut paths, MCP_FIT_BYTES);
    for (what, status, items) in [
        ("the default status", hostile_default(), 0),
        ("a page of worktrees", worktrees, 8),
        ("a page of paths", paths, 32),
    ] {
        let wire = wire(&status, Lang::En);
        assert_eq!(wire["isError"], false, "{what}: {wire}");
        for (part, body) in parts(&wire) {
            eprintln!("{what} ({part} part): {} B", body.len());
            assert!(
                body.len() <= MAX_MCP_PART_BYTES,
                "{what} ({part}): {} B",
                body.len()
            );
        }
        let shown = &wire["structuredContent"];
        if items == 0 {
            assert!(
                shown.get("here").is_some() && shown.get("repo").is_some(),
                "{what}: {shown}"
            );
            continue;
        }
        let page = &shown["page"];
        let kept = page["worktrees"].as_array().map_or(0, Vec::len)
            + page["paths"].as_array().map_or(0, Vec::len);
        assert!(
            kept > 0 && kept < items,
            "{what}: {kept} of {items} items kept"
        );
        assert_eq!(page["truncated"], true, "{what}: {page}");
        assert!(page["cursor"].is_string(), "{what}: {page}");
    }
}

/// RES-MCP-01: the `status` tool, with its `cursor` argument, as the model reads it (name,
/// description and `inputSchema`) fits 150 tokens, and the catalog stays within its budget.
#[test]
fn the_status_tool_with_its_cursor_fits_its_token_budget() {
    let tool = serde_json::to_value(status_tool()).unwrap();
    assert_eq!(tool["name"], STATUS_TOOL);
    assert_eq!(
        tool["inputSchema"],
        json!({
            "type": "object",
            "properties": {"cursor": {"type": "string"}},
            "additionalProperties": false
        })
    );
    assert!(tool["description"].as_str().unwrap().contains("cursor"));
    let mut read = tool.clone();
    read.as_object_mut().unwrap().remove("outputSchema");
    check_token_budget(
        "RES-MCP-01",
        "the status tool (name, description, inputSchema)",
        &read.to_string(),
        MCP_TOOL_TOKENS,
    )
    .unwrap();
    let info = serde_json::to_value(Raptor::default().get_info()).unwrap();
    let snapshot = serde_json::to_value(snapshot::tool()).unwrap();
    let overruns = catalog_overruns(&info, &[tool, snapshot]);
    assert!(overruns.is_empty(), "{overruns:#?}");
}

/// ADR-MCP-001 § 5: the compact `outputSchema` keeps `here`, `repo` and `page` as opaque
/// objects, and still validates every answer the tool sends and rejects what it never sends.
#[test]
fn the_compact_output_schema_validates_every_reference() {
    let tool = serde_json::to_value(status_tool()).unwrap();
    let schema = &tool["outputSchema"];
    for field in ["here", "repo", "page"] {
        assert_eq!(
            schema["properties"][field],
            json!({"type": "object"}),
            "{field} in {schema}"
        );
    }
    let mut every: Vec<(String, McpStatus)> = default_references()
        .into_iter()
        .map(|(what, status)| (what.to_owned(), status))
        .collect();
    every.push(("a page of worktrees".into(), typical_worktrees_page()));
    every.push(("a page of paths".into(), typical_paths_page()));
    for (what, status) in every {
        let wire = wire(&status, Lang::En);
        let value = &wire["structuredContent"];
        if let Err(why) = conforms(schema, schema, value) {
            panic!("{what}: {why}\n{value}\n{schema}");
        }
    }
    let base = json!({"worktree": {"untrusted": "w"}, "requester": {"actor": "unattributed"}});
    for (field, bad) in [
        ("here", json!("x")),
        ("repo", json!(3)),
        ("page", json!([])),
    ] {
        let mut value = base.clone();
        value[field] = bad;
        assert!(conforms(schema, schema, &value).is_err(), "{value}");
    }
}

/// The literals of the binary: the values the enums of the contract take, which are not text of
/// the repo.
const LITERALS: &[&str] = &[
    "agent",
    "unattributed",
    "claude-code",
    "other",
    "detected",
    "registered",
    "active",
    "inactive",
    "full",
    "mcp-only",
    "unconfirmed",
    "pending",
    "invalid",
    "reconciling",
    "dormant",
    "waiting-for-git",
    "register-to-write",
    "missing",
    "untrusted",
    "unreadable",
    "base-missing",
    "no-base",
    "no-commits",
    "hookspath-changed",
    "template-outdated",
    "base-unconfirmed",
    "config-unreadable",
];

/// The strings of `value` that are neither under `untrusted`, nor a literal of the contract, nor a
/// cursor.
fn unmarked(value: &Value, key: Option<&str>, found: &mut Vec<String>) {
    match value {
        Value::Object(map) if map.contains_key("untrusted") => {
            // An untrusted text carries only its own marks.
            assert!(map["untrusted"].is_string(), "{value}");
            for (k, v) in map {
                assert!(
                    k == "untrusted" || ((k == "truncated" || k == "lossy") && v.is_boolean()),
                    "{value}"
                );
            }
        }
        Value::Object(map) => map.iter().for_each(|(k, v)| unmarked(v, Some(k), found)),
        Value::Array(items) => items.iter().for_each(|v| unmarked(v, key, found)),
        Value::String(text) => {
            let cursor = key == Some("cursor") && valid_cursor(text);
            if !cursor && !LITERALS.contains(&text.as_str()) {
                found.push(text.clone());
            }
        }
        _ => {}
    }
}

/// C12: every text from the repo or from another agent (worktrees, branches, base, agent names,
/// paths) is marked as untrusted, cut at its bound and escaped; everything else is a literal of
/// the binary.
#[test]
fn every_repo_text_in_the_status_is_marked() {
    let mut full = hostile_default();
    let repo = full.repo.as_mut().unwrap();
    repo.engine = Some(McpEngineState::Reconciling);
    repo.base.state = McpBaseState::Pending;
    repo.protection = McpProtectionState::McpOnly;
    repo.protection_lost = Some(LossCause::HookspathChanged);
    repo.diagnostics = vec![Diagnostic::TemplateOutdated, Diagnostic::BaseUnconfirmed];
    repo.sessions_unknown = true;
    let other = McpWorktree {
        name: hostile_name(),
        branch: Some(hostile_name()),
        main: false,
        unavailable: Some(UnavailableReason::Missing),
        state: McpHere {
            sessions: vec![session(hostile_name(), SessionStateView::Active)],
            uncounted: Some(McpUncounted::BaseMissing),
            ..McpHere::default()
        },
    };
    let worktrees = page_status(worktrees_page(vec![other]));
    let paths = page_status(paths_page(
        hostile_name(),
        vec![Untrusted::new("dir/a\u{202e}b.rs")],
    ));

    // Where each answer has names of the repo (cut at 100 characters) and paths (cut in bytes).
    let cases = [
        (
            "the default status",
            full,
            vec![
                "/worktree",
                "/branch",
                "/requester/name",
                "/here/sessions/0/actor/name",
                "/repo/base/name",
            ],
            vec![],
        ),
        (
            "a page of worktrees",
            worktrees,
            vec![
                "/page/worktrees/0/name",
                "/page/worktrees/0/branch",
                "/page/worktrees/0/sessions/0/actor/name",
            ],
            vec![],
        ),
        (
            "a page of paths",
            paths,
            vec!["/page/of"],
            vec!["/page/paths/0"],
        ),
    ];
    for (what, status, names, paths) in cases {
        let wire = wire(&status, Lang::En);
        assert_eq!(wire["isError"], false, "{what}: {wire}");
        let shown = &wire["structuredContent"];
        let mut found = Vec::new();
        unmarked(shown, None, &mut found);
        assert!(
            found.is_empty(),
            "{what}: text of the repo that is not marked: {found:?}"
        );
        let marked = |at: &str| {
            let text = shown
                .pointer(at)
                .unwrap_or_else(|| panic!("{what}: nothing at {at} in {shown}"));
            let body = text["untrusted"]
                .as_str()
                .unwrap_or_else(|| panic!("{what}: {at} is not marked: {text}"));
            assert!(!body.contains('\u{202e}'), "{what}: {at} is not escaped");
            (text.clone(), body.to_owned())
        };
        for at in names {
            let (text, body) = marked(at);
            assert_eq!(body.chars().count(), 100, "{what}: {at}");
            assert_eq!(text["truncated"], true, "{what}: {at}");
        }
        for at in paths {
            let (_, body) = marked(at);
            assert!(body.len() <= gitraptor_api::mcp_view::MAX_MCP_PATH_BYTES);
        }
    }
}
