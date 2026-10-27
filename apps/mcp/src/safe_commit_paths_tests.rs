//! The lexical check of the paths of `safe_commit` in `raptor-mcp` (US-MCP-010, DS-US-MCP-010
//! T003): a bad path or too many paths are refused with their code before the engine is
//! asked. Dedicated file (wired from `safe_commit_paths.rs` by `#[path]`): the criteria's
//! tests must not live in the production file.

use super::*;

use std::sync::Arc;

use gitraptor_api::mcp_view::{MCP_REFUSAL_TOKENS, McpToolError, check_token_budget};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::messages::{Lang, ToolRefusal, refusal};
use crate::safe_commit::{CommitCall, call};

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| (*p).to_owned()).collect()
}

fn refused(list: &[&str]) -> ToolRefusal {
    check(Some(&paths(list))).expect_err("the paths must be refused")
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn a_bad_path_never_reaches_the_engine() {
    // No paths, or good ones, pass the check.
    assert!(check(None).is_ok());
    assert!(check(Some(&paths(&["src/a.rs", "docs/b.md"]))).is_ok());
    // `:(glob)**` and `:!x` are literal names for the lexical check (⛔2.1).
    assert!(check(Some(&paths(&[":(glob)**", ":!x"]))).is_ok());

    for (bad, code) in [
        ("/etc/passwd", McpToolError::InvalidPath),
        ("\\\\servidor\\compartido\\a.rs", McpToolError::InvalidPath),
        ("C:x", McpToolError::InvalidPath),
        ("a\u{1b}b", McpToolError::InvalidPath),
        ("-f", McpToolError::InvalidPath),
        ("../otro-repo/secreto", McpToolError::PathOutsideWorktree),
        (".git/config", McpToolError::PathOutsideWorktree),
        ("sub/.GIT/hooks/x", McpToolError::PathOutsideWorktree),
    ] {
        assert_eq!(refused(&["src/a.rs", bad]).code, code, "{bad:?}");
    }

    // And the whole call stops there: a valid message with a bad path is refused with the
    // path's code, before any engine call.
    let engine = Arc::new(Engine::default());
    let outcome = block_on(call(
        &engine,
        CommitCall {
            message: "fix: a".to_owned(),
            paths: Some(paths(&["src/a.rs", "../otro-repo/secreto"])),
        },
        None,
    ));
    let refusal = outcome.expect_err("a bad path refuses the whole call");
    assert_eq!(refusal.code, McpToolError::PathOutsideWorktree);
}

#[test]
fn two_hundred_fifty_seven_paths_are_too_many_paths() {
    let many = |n: usize| -> Vec<String> { (0..n).map(|i| format!("src/f{i}.rs")).collect() };
    assert!(
        check(Some(&many(256))).is_ok(),
        "256 paths are the limit, allowed"
    );
    let refused = check(Some(&many(257))).expect_err("257 paths are too many");
    assert_eq!(refused.code, McpToolError::TooManyPaths);
    assert_eq!(refused.params, Some(json!({"max": 256})));

    // The count comes before looking at any path.
    let mut with_bad = many(257);
    with_bad[0] = "/etc/passwd".to_owned();
    let refused = check(Some(&with_bad)).expect_err("too many comes first");
    assert_eq!(refused.code, McpToolError::TooManyPaths);

    // Never `invalid-params`: the full call answers with the domain refusal.
    let engine = Arc::new(Engine::default());
    let outcome = block_on(call(
        &engine,
        CommitCall {
            message: "fix: a".to_owned(),
            paths: Some(many(257)),
        },
        None,
    ));
    assert_eq!(
        outcome.expect_err("too many paths").code,
        McpToolError::TooManyPaths
    );
}

#[test]
fn the_refused_path_travels_as_untrusted() {
    let body = refusal(refused(&["../otro-repo/secreto"]), Lang::En);
    assert_eq!(body["code"], "path-outside-worktree", "{body}");
    assert_eq!(
        body["params"]["path"]["untrusted"], "../otro-repo/secreto",
        "the path is repo-side text: {{\"untrusted\": …}}: {body}"
    );

    let body = refusal(refused(&["/etc/passwd"]), Lang::En);
    assert_eq!(body["code"], "invalid-path", "{body}");
    assert_eq!(body["params"]["path"]["untrusted"], "/etc/passwd", "{body}");

    // A path made to forge what the developer sees keeps its text as data, and one far
    // over the bound is cut at 1 024 bytes.
    let huge = format!("../{}", "a".repeat(3_000));
    let body = refusal(refused(&[&huge]), Lang::En);
    let kept = body["params"]["path"]["untrusted"].as_str().unwrap();
    assert!(kept.len() <= 1_024, "{} bytes kept", kept.len());
    assert!(kept.starts_with("../aaa"), "the head of the path is kept");
    assert!(body["params"].get("path").is_some_and(Value::is_object));
}

#[test]
fn the_spanish_texts_say_what_the_story_says_for_paths() {
    let es = |code: McpToolError, path: Option<&str>| {
        refusal(
            ToolRefusal {
                code,
                params: path.map(|p| json!({"path": {"untrusted": p}})),
            },
            Lang::Es,
        )
    };
    let name_files = "Nombra archivos del worktree, relativos a su raíz, con su nombre exacto.";

    let invalid = es(McpToolError::InvalidPath, Some("/etc/passwd"));
    assert!(
        invalid["message"]
            .as_str()
            .unwrap()
            .starts_with("Ruta no válida:"),
        "{invalid}"
    );
    assert_eq!(invalid["action"], name_files, "{invalid}");

    let outside = es(
        McpToolError::PathOutsideWorktree,
        Some("../otro-repo/secreto"),
    );
    assert!(
        outside["message"]
            .as_str()
            .unwrap()
            .starts_with("Ruta fuera del worktree:"),
        "{outside}"
    );
    assert_eq!(outside["action"], name_files, "{outside}");

    let many = refusal(
        ToolRefusal {
            code: McpToolError::TooManyPaths,
            params: Some(json!({"max": 256})),
        },
        Lang::Es,
    );
    assert_eq!(
        many["message"], "Demasiadas rutas: como mucho 256.",
        "{many}"
    );
    assert_eq!(many["params"], json!({"max": 256}), "{many}");
    assert_eq!(many["action"], name_files, "{many}");

    let ignored = es(McpToolError::PathIgnored, Some(".env"));
    assert_eq!(
        ignored["message"], "La ruta .env está ignorada por Git; no se añade.",
        "{ignored}"
    );

    let shared = es(McpToolError::OtherSessionPresent, None);
    assert_eq!(
        shared["message"], "Hay otra sesión de agente en este worktree.",
        "{shared}"
    );
    assert_eq!(
        shared["action"], "Indica tus rutas: el worktree es compartido.",
        "{shared}"
    );

    // RES-MCP-03 for each, in both languages.
    for lang in [Lang::En, Lang::Es] {
        for (code, path) in [
            (McpToolError::InvalidPath, Some("/etc/passwd")),
            (
                McpToolError::PathOutsideWorktree,
                Some("../otro-repo/secreto"),
            ),
            (McpToolError::PathIgnored, Some(".env")),
            (McpToolError::OtherSessionPresent, None),
        ] {
            let body = refusal(
                ToolRefusal {
                    code,
                    params: path.map(|p| json!({"path": {"untrusted": p}})),
                },
                lang,
            )
            .to_string();
            check_token_budget("RES-MCP-03", code.as_str(), &body, MCP_REFUSAL_TOKENS).unwrap();
        }
    }
}
