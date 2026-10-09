//! The contract of the full `status` of the MCP: the shape of what the tool sends (a field
//! allowlist, SEC-12), the bound of every name, the cut of a page to its budget, the compact
//! output schema and the capability that gates it all. Dedicated file: the criteria's tests
//! must not live in a production file.
//!
//! Every test goes through the same door the tool uses ([`McpStatusView`], [`McpStatus::for_mcp`],
//! [`fit_page`]), so a field the daemon fills but the view drops fails here.

use gitraptor_api::capability;
use gitraptor_api::mcp_view::{
    MAX_MCP_NAME_CHARS, MAX_MCP_PART_BYTES, MCP_CURSOR_PLACEHOLDER, MCP_FIT_BYTES,
    MCP_OUTPUT_SCHEMA_TOKENS, McpStatusView, check_token_budget, compact_schema, fit_page,
    wire_len,
};
use gitraptor_api::messages::{RepoStateView, SessionStateView, UnavailableReason};
use gitraptor_api::methods::{
    CAP_MCP_STATUS_FULL, MCP_CURSOR_LEN, MCP_UNAVAILABLE, McpBase, McpBaseState, McpHere,
    McpListRef, McpPage, McpProtectionState, McpRepo, McpSession, McpStatus, McpStatusParams,
    McpWorktree, valid_cursor,
};
use gitraptor_api::rpc::error_name;
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use serde_json::{Value, json};

const HERE_CURSOR: &str = "9f2c4b1a7d3e5f60";
const WORKTREES_CURSOR: &str = "41ab9c03d5e6f782";

fn list(total: u64, cursor: &str) -> McpListRef {
    McpListRef {
        total,
        cursor: cursor.to_owned(),
    }
}

fn agent(name: &str) -> Actor {
    Actor::Agent {
        kind: AgentKind::Other,
        name: Some(UntrustedName::new(name)),
        origin: AgentOrigin::Registered,
    }
}

fn session(name: &str, state: SessionStateView) -> McpSession {
    McpSession {
        actor: agent(name),
        state,
    }
}

fn typical_repo() -> McpRepo {
    McpRepo {
        engine: None,
        base: McpBase {
            name: Some(UntrustedName::new("main")),
            state: McpBaseState::Confirmed,
        },
        protection: McpProtectionState::Full,
        protection_lost: None,
        diagnostics: Vec::new(),
        fetch_age_s: Some(180),
        gaps: Vec::new(),
        gaps_total: None,
        sessions_unknown: false,
        worktrees: Some(list(3, WORKTREES_CURSOR)),
    }
}

/// The typical default answer: the caller's worktree with 3000 changes and a repo with three
/// more worktrees.
fn default_status() -> McpStatus {
    McpStatus {
        repo_id: "f".repeat(64),
        repo_state: RepoStateView::Observed,
        worktree: UntrustedName::new("shop-feat-a"),
        branch: Some(UntrustedName::new("feat-a")),
        main: false,
        requester: Actor::Unattributed,
        action: None,
        here: Some(McpHere {
            changes: Some(list(3000, HERE_CURSOR)),
            ..McpHere::default()
        }),
        repo: Some(typical_repo()),
        page: None,
    }
}

fn with_page(page: McpPage) -> McpStatus {
    McpStatus {
        here: None,
        repo: None,
        page: Some(page),
        ..default_status()
    }
}

fn worktree_page(items: Vec<McpWorktree>, total: u64) -> McpPage {
    McpPage {
        of: None,
        total,
        worktrees: items,
        paths: Vec::new(),
        truncated: false,
        cursor: None,
    }
}

fn other_worktree(name: &str) -> McpWorktree {
    McpWorktree {
        name: UntrustedName::new(name),
        branch: Some(UntrustedName::new("feat-b")),
        main: false,
        unavailable: None,
        state: McpHere {
            sessions: vec![session("claude-2", SessionStateView::Active)],
            changes: Some(list(4, "00c0ffee00c0ffee")),
            ahead: Some(2),
            ..McpHere::default()
        },
    }
}

fn paths_page(n: usize, total: u64, truncated: bool) -> McpPage {
    McpPage {
        of: Some(UntrustedName::new("shop-feat-a")),
        total,
        worktrees: Vec::new(),
        paths: (0..n)
            .map(|i| Untrusted::new(format!("src/module_{i:04}/file.rs")))
            .collect(),
        truncated,
        cursor: truncated.then(|| MCP_CURSOR_PLACEHOLDER.to_owned()),
    }
}

fn view(status: &McpStatus) -> Value {
    serde_json::to_value(McpStatusView::from(status)).unwrap()
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

/// SEC-12 and RES-MCP-02: the default answer is its fields and nothing else, in the form the
/// design fixes; the lists are `{total, cursor}`, never their items.
#[test]
fn the_default_status_view_is_its_field_allowlist() {
    let status = default_status();
    assert_eq!(
        serde_json::to_string(&McpStatusView::from(&status)).unwrap(),
        concat!(
            r#"{"worktree":{"untrusted":"shop-feat-a"},"branch":{"untrusted":"feat-a"},"#,
            r#""requester":{"actor":"unattributed"},"#,
            r#""here":{"changes":{"total":3000,"cursor":"9f2c4b1a7d3e5f60"}},"#,
            r#""repo":{"base":{"name":{"untrusted":"main"}},"protection":"full","#,
            r#""fetch_age_s":180,"worktrees":{"total":3,"cursor":"41ab9c03d5e6f782"}}}"#,
        )
    );
    let everything = McpStatus {
        main: true,
        action: Some(gitraptor_api::methods::McpStatusAction::RegisterToWrite),
        ..status.clone()
    };
    assert_eq!(
        sorted_keys(&view(&everything)),
        [
            "action",
            "branch",
            "here",
            "main",
            "repo",
            "requester",
            "worktree"
        ]
    );
    // No `repo_id`, no `repo_state`, and nothing but the view reaches the model.
    let json = view(&status).to_string();
    assert!(!json.contains(&"f".repeat(64)), "{json}");
    // What is empty or at its default is left out (RES-MCP-04).
    let quiet = McpStatus {
        here: Some(McpHere::default()),
        ..status
    };
    assert_eq!(view(&quiet)["here"], json!({}));
}

/// The page of worktrees: each one with its branch, its present sessions with actor and
/// state, its changes as `{total, cursor}` and its ahead/behind; the cursor only while there is
/// more.
#[test]
fn a_worktrees_page_is_its_field_allowlist() {
    let status = with_page(worktree_page(vec![other_worktree("shop-feat-b")], 1));
    let wire = view(&status);
    assert_eq!(
        sorted_keys(&wire),
        ["branch", "page", "requester", "worktree"]
    );
    assert_eq!(
        wire["page"],
        json!({
            "total": 1,
            "worktrees": [{
                "name": {"untrusted": "shop-feat-b"},
                "branch": {"untrusted": "feat-b"},
                "sessions": [{
                    "actor": {
                        "actor": "agent", "kind": "other",
                        "name": {"untrusted": "claude-2"}, "origin": "registered"
                    },
                    "state": "active"
                }],
                "changes": {"total": 4, "cursor": "00c0ffee00c0ffee"},
                "ahead": 2
            }]
        })
    );
    // An unavailable worktree says why and nothing else of its state.
    let gone = McpWorktree {
        unavailable: Some(UnavailableReason::Missing),
        state: McpHere::default(),
        branch: None,
        ..other_worktree("shop-gone")
    };
    let wire = view(&with_page(worktree_page(vec![gone], 1)));
    assert_eq!(
        wire["page"]["worktrees"][0],
        json!({"name": {"untrusted": "shop-gone"}, "unavailable": "missing"})
    );
    // A page of paths: the worktree it is of, the paths as untrusted text, the cursor to go on.
    let wire = view(&with_page(paths_page(2, 3000, true)));
    assert_eq!(
        wire["page"],
        json!({
            "of": {"untrusted": "shop-feat-a"},
            "total": 3000,
            "paths": [
                {"untrusted": "src/module_0000/file.rs"},
                {"untrusted": "src/module_0001/file.rs"}
            ],
            "truncated": true,
            "cursor": MCP_CURSOR_PLACEHOLDER
        })
    );
    assert_eq!(MCP_CURSOR_PLACEHOLDER.len(), MCP_CURSOR_LEN);
}

/// A base that is not confirmed is declared, never refused; a confirmed one leaves its state out
/// and keeps its name.
#[test]
fn every_base_state_is_declared() {
    let base = |name: Option<&str>, state| McpStatus {
        repo: Some(McpRepo {
            base: McpBase {
                name: name.map(UntrustedName::new),
                state,
            },
            ..typical_repo()
        }),
        ..default_status()
    };
    let wire = |status: &McpStatus| view(status)["repo"]["base"].clone();
    assert_eq!(
        wire(&base(Some("main"), McpBaseState::Confirmed)),
        json!({"name": {"untrusted": "main"}})
    );
    for (state, text) in [
        (McpBaseState::Unconfirmed, "unconfirmed"),
        (McpBaseState::Pending, "pending"),
        (McpBaseState::Invalid, "invalid"),
    ] {
        assert_eq!(
            wire(&base(Some("main"), state)),
            json!({"name": {"untrusted": "main"}, "state": text})
        );
    }
    assert_eq!(
        wire(&base(None, McpBaseState::Invalid)),
        json!({"state": "invalid"})
    );
}

/// D5: a page is cut from its end until it fits, and says so: `truncated` and a cursor to go on.
#[test]
fn fit_page_cuts_items_and_marks_them() {
    assert_eq!(MCP_FIT_BYTES, MAX_MCP_PART_BYTES - 1024);
    let full = with_page(worktree_page(
        (0..8)
            .map(|i| other_worktree(&format!("shop-feat-{i}")))
            .collect(),
        20,
    ));
    let whole = wire_len(&full);
    assert!(
        whole > 800,
        "the page under test must be worth cutting: {whole} B"
    );

    // A page that fits is left as it is.
    let mut fits = full.clone();
    fit_page(&mut fits, whole);
    assert_eq!(fits, full);

    // One that does not loses items from the end, as few as it takes, and keeps its total.
    let budget = whole / 2;
    let mut cut = full.clone();
    fit_page(&mut cut, budget);
    let (original, page) = (full.page.as_ref().unwrap(), cut.page.as_ref().unwrap());
    assert!(
        wire_len(&cut) <= budget,
        "{} B over {budget}",
        wire_len(&cut)
    );
    assert!(!page.worktrees.is_empty() && page.worktrees.len() < 8);
    assert_eq!(page.worktrees, original.worktrees[..page.worktrees.len()]);
    assert_eq!(page.total, 20);
    assert!(page.truncated);
    assert_eq!(page.cursor.as_deref(), Some(MCP_CURSOR_PLACEHOLDER));
    let one_more = McpStatus {
        page: Some(McpPage {
            worktrees: original.worktrees[..=page.worktrees.len()].to_vec(),
            ..page.clone()
        }),
        ..cut.clone()
    };
    assert!(wire_len(&one_more) > budget, "it cut more than it had to");

    // The same for a page of paths.
    let paths = with_page(paths_page(32, 3000, false));
    let budget = wire_len(&paths) / 2;
    let mut cut = paths.clone();
    fit_page(&mut cut, budget);
    let page = cut.page.as_ref().unwrap();
    assert!(wire_len(&cut) <= budget);
    assert!(!page.paths.is_empty() && page.paths.len() < 32);
    assert_eq!(
        page.paths,
        paths.page.as_ref().unwrap().paths[..page.paths.len()]
    );
    assert!(page.truncated);
    assert_eq!(page.cursor.as_deref(), Some(MCP_CURSOR_PLACEHOLDER));

    // An answer without a page has nothing to cut.
    let mut plain = default_status();
    fit_page(&mut plain, 1);
    assert_eq!(plain, default_status());
}

/// RES-MCP-01: `here`, `repo` and `page` are opaque objects in the output schema: spelled out,
/// they would cost three times the budget of the schema.
#[test]
fn the_output_schema_keeps_the_detail_opaque() {
    let mut schema = serde_json::to_value(schemars::schema_for!(McpStatusView)).unwrap();
    compact_schema(&mut schema);
    for field in ["here", "repo", "page"] {
        assert_eq!(
            schema["properties"][field],
            json!({"type": "object"}),
            "{field} in {schema}"
        );
    }
    let required = schema["required"].as_array().unwrap();
    for field in ["here", "repo", "page"] {
        assert!(!required.iter().any(|r| r == field), "{field} is optional");
    }
    for name in ["McpHere", "McpRepo", "McpPage", "McpWorktree", "McpListRef"] {
        assert!(
            schema["$defs"].get(name).is_none(),
            "{name} leaked: {schema}"
        );
    }
    check_token_budget(
        "RES-MCP-01",
        "the compact output schema",
        &schema.to_string(),
        MCP_OUTPUT_SCHEMA_TOKENS,
    )
    .unwrap();
}

/// ADR-MCP-001 § 6: every name of the answer is cut at its bound wherever it is, the nested ones
/// of `here`, `repo` and `page` included.
#[test]
fn names_in_here_repo_and_page_are_cut_at_their_bound() {
    let long = || UntrustedName::new("n".repeat(1024));
    let hostile_session = || McpSession {
        actor: Actor::Agent {
            kind: AgentKind::Other,
            name: Some(long()),
            origin: AgentOrigin::Registered,
        },
        state: SessionStateView::Inactive,
    };
    let status = McpStatus {
        here: Some(McpHere {
            sessions: vec![hostile_session()],
            ..McpHere::default()
        }),
        repo: Some(McpRepo {
            base: McpBase {
                name: Some(long()),
                state: McpBaseState::Unconfirmed,
            },
            ..typical_repo()
        }),
        page: Some(McpPage {
            of: Some(long()),
            total: 1,
            worktrees: vec![McpWorktree {
                name: long(),
                branch: Some(long()),
                main: false,
                unavailable: None,
                state: McpHere {
                    sessions: vec![hostile_session()],
                    ..McpHere::default()
                },
            }],
            paths: Vec::new(),
            truncated: false,
            cursor: None,
        }),
        ..default_status()
    }
    .for_mcp();

    let actor_name = |s: &McpSession| match &s.actor {
        Actor::Agent { name, .. } => name.clone().expect("a declared name"),
        Actor::Unattributed => panic!("an agent"),
    };
    let here = status.here.as_ref().unwrap();
    let repo = status.repo.as_ref().unwrap();
    let page = status.page.as_ref().unwrap();
    let worktree = &page.worktrees[0];
    let names = [
        actor_name(&here.sessions[0]),
        repo.base.name.clone().unwrap(),
        page.of.clone().unwrap(),
        worktree.name.clone(),
        worktree.branch.clone().unwrap(),
        actor_name(&worktree.state.sessions[0]),
    ];
    for name in &names {
        assert_eq!(name.raw().chars().count(), MAX_MCP_NAME_CHARS);
        assert!(name.is_truncated());
    }
    // And the view the tool sends carries them cut.
    let wire = view(&status).to_string();
    assert!(
        !wire.contains(&"n".repeat(MAX_MCP_NAME_CHARS + 1)),
        "{} B",
        wire.len()
    );
}

/// ADR-GRP-016: the full status is a capability of the `mcp` module, its error is in the
/// module's block with a stable name, and a cursor has one form.
#[test]
fn the_full_status_needs_its_capability() {
    assert_eq!(CAP_MCP_STATUS_FULL.name, "mcp.status-full");
    assert!(
        capability::all().any(|c| c.name == CAP_MCP_STATUS_FULL.name),
        "the capability is in no module's list"
    );
    assert_eq!(MCP_UNAVAILABLE.code, -33080);
    assert_eq!(MCP_UNAVAILABLE.name, "mcp-unavailable");
    assert_eq!(error_name(MCP_UNAVAILABLE.code), Some("mcp-unavailable"));

    assert!(valid_cursor("0123456789abcdef"));
    for bad in [
        "",
        "0123456789abcde",
        "0123456789abcdef0",
        "0123456789ABCDEF",
        "0123456789abcdeg",
        "0123456789abcde\u{e9}",
        " 123456789abcdef",
    ] {
        assert!(!valid_cursor(bad), "{bad:?}");
    }

    // The parameter is optional, and nothing else is a parameter.
    assert_eq!(
        serde_json::from_value::<McpStatusParams>(json!({})).unwrap(),
        McpStatusParams { cursor: None }
    );
    let with = serde_json::from_value::<McpStatusParams>(json!({"cursor": HERE_CURSOR})).unwrap();
    assert_eq!(with.cursor.as_deref(), Some(HERE_CURSOR));
    assert!(serde_json::from_value::<McpStatusParams>(json!({"repo": "x"})).is_err());
}
