//! The contract of `check_conflicts` (US-MCP-016, DS-US-MCP-016): the field allowlist of the
//! view (SEC-12, MCP10), the cut of a page to its budget (D5, D12) and the escape and bound of
//! hostile texts (D6). Dedicated file: the criteria's tests must not live in a production file.
//!
//! Every test goes through the door the tool uses: [`McpConflictsView::for_mcp`], then
//! [`for_mcp`], measured by [`conflicts_wire_len`] and cut by [`fit_conflicts`]. Sizes are bytes
//! of compact JSON, the unit of `wire_len`; `target` is `MCP_PAGE_TOKENS * MCP_BYTES_PER_TOKEN`
//! and `hard` is `MCP_FIT_BYTES`.

use std::collections::BTreeSet;

use gitraptor_api::mcp_view::{
    MAX_MCP_NAME_CHARS, MAX_MCP_PATH_BYTES, MCP_BYTES_PER_TOKEN, MCP_CURSOR_PLACEHOLDER,
    MCP_FIT_BYTES, MCP_PAGE_TOKENS, McpConflictsView, conflicts_wire_len, fit_conflicts, for_mcp,
};
use gitraptor_api::messages::RepoStateView;
use gitraptor_api::methods::{
    McpConflictFile, McpConflicts, McpConflictsScope, McpPair, McpPairConflict, McpPairCounts,
    McpSide,
};
use gitraptor_api::prediction::{
    ConflictKindView, LimitCode, PairLevel, PairReason, PairStateView,
};
use gitraptor_api::{Untrusted, UntrustedName};
use serde_json::{Value, json};

const TARGET: usize = MCP_PAGE_TOKENS * MCP_BYTES_PER_TOKEN;

fn side(name: &str) -> McpSide {
    McpSide::Worktree(UntrustedName::new(name))
}

fn file(path: &str, ranges: usize) -> McpConflictFile {
    McpConflictFile {
        path: Untrusted::new(path),
        kind: ConflictKindView::Content,
        a: (0..ranges).map(|i| [40 + 50 * i as u32, 19]).collect(),
        b: (0..ranges).map(|i| [41 + 50 * i as u32, 18]).collect(),
        hunks_truncated: false,
        merge_rule: false,
    }
}

fn conflict_pair(other: &str, files: Vec<McpConflictFile>) -> McpPair {
    McpPair {
        a: side("shop-feat-a"),
        b: side(other),
        state: None,
        reason: None,
        level: Some(PairLevel::Conflict),
        age_s: Some(12),
        computed_at_utc_ms: Some(1_793_001_234_567),
        overlap: Vec::new(),
        overlap_partial: false,
        conflict: Some(McpPairConflict {
            files,
            truncated: false,
        }),
    }
}

fn overlap_pair(other: &str, paths: Vec<Untrusted>) -> McpPair {
    McpPair {
        a: side("shop-feat-a"),
        b: side(other),
        state: None,
        reason: None,
        level: Some(PairLevel::Overlap),
        age_s: Some(12),
        computed_at_utc_ms: Some(1_793_001_234_567),
        overlap: paths,
        overlap_partial: false,
        conflict: None,
    }
}

fn answer(pairs: Vec<McpPair>) -> McpConflicts {
    let conflicts = pairs.iter().filter(|p| p.conflict.is_some()).count() as u32;
    McpConflicts {
        repo_id: "repo-id-never-on-the-wire".into(),
        repo_state: RepoStateView::Observed,
        worktree: Some(UntrustedName::new("shop-feat-a")),
        scope: McpConflictsScope::Worktree,
        reason: None,
        limits: LimitCode::ALL.to_vec(),
        unpublished: false,
        counts: McpPairCounts {
            conflict: conflicts,
            overlap: pairs.len() as u32 - conflicts,
            none: 0,
            calculating: 0,
            pending: 0,
            not_computable: 0,
        },
        pairs,
        truncated: false,
        cursor: None,
    }
}

/// What the tool sends: the view, then the escape pass.
fn wire(c: &McpConflicts) -> Value {
    let mut value = serde_json::to_value(McpConflictsView::for_mcp(c)).expect("view serializes");
    for_mcp(&mut value);
    value
}

fn keys_of(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                into.insert(key.clone());
                keys_of(inner, into);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| keys_of(i, into)),
        _ => {}
    }
}

fn typical_page() -> McpConflicts {
    let conflicts = ["shop-feat-b", "shop-feat-c", "shop-feat-d"]
        .map(|other| conflict_pair(other, vec![file("src/pago.rs", 2), file("src/cart.rs", 2)]));
    let overlaps = ["shop-feat-e", "shop-feat-f"].map(|other| {
        overlap_pair(
            other,
            vec![
                Untrusted::new("src/a/x.rs"),
                Untrusted::new("src/b/y.rs"),
                Untrusted::new("src/c/z.rs"),
            ],
        )
    });
    answer(conflicts.into_iter().chain(overlaps).collect())
}

/// SEC-12 / MCP10: a ⚡ pair carries exactly the allowlisted fields.
#[test]
fn a_conflict_pair_view_is_its_field_allowlist() {
    let c = answer(vec![conflict_pair(
        "shop-feat-b",
        vec![file("src/pago.rs", 1)],
    )]);
    let value = wire(&c);

    let mut keys = BTreeSet::new();
    keys_of(&value["pairs"], &mut keys);
    let allowed: BTreeSet<String> = [
        "a",
        "b",
        "state",
        "reason",
        "level",
        "age_s",
        "computed_at_utc_ms",
        "overlap",
        "overlap_partial",
        "conflict",
        "path",
        "kind",
        "hunks_truncated",
        "merge_rule",
        "files",
        "truncated",
        "worktree",
        "base",
        "untrusted",
        "lossy",
    ]
    .map(String::from)
    .into();
    let extra: Vec<_> = keys.difference(&allowed).collect();
    assert!(
        extra.is_empty(),
        "MCP016-FIELD-ALLOWLIST: the ⚡ pair view carries fields outside the allowlist: {extra:?}"
    );
    for never in [
        "pair_id",
        "rev",
        "merge_base",
        "rewrites_escalated",
        "repo_id",
    ] {
        assert!(
            !keys.contains(never) && value.get(never).is_none(),
            "MCP016-FIELD-ALLOWLIST: `{never}` must never reach the model"
        );
    }
    assert!(
        value.get("repo_state").is_none(),
        "MCP016-FIELD-ALLOWLIST: repo_state does not go in the view"
    );
    assert!(
        !value.to_string().contains("repo-id-never-on-the-wire"),
        "MCP016-FIELD-ALLOWLIST: the repo id leaked into the view"
    );
    assert_eq!(
        value["pairs"][0],
        json!({
            "a": {"worktree": {"untrusted": "shop-feat-a"}},
            "b": {"worktree": {"untrusted": "shop-feat-b"}},
            "level": "conflict",
            "age_s": 12,
            "computed_at_utc_ms": 1_793_001_234_567_i64,
            "conflict": {"files": [{
                "path": {"untrusted": "src/pago.rs"},
                "kind": "content",
                "a": [[40, 19]],
                "b": [[41, 18]]
            }]}
        }),
        "MCP016-FIELD-ALLOWLIST: the ⚡ pair is not the shape of § Contratos (current omits state, empty and false fields are omitted)"
    );
    // A pair without a result: state and reason, never a level.
    let mut pending = overlap_pair("shop-feat-c", Vec::new());
    pending.level = None;
    pending.state = Some(PairStateView::Pending);
    pending.reason = Some(PairReason::BaseUnconfirmed);
    pending.age_s = None;
    pending.computed_at_utc_ms = None;
    let value = wire(&answer(vec![pending]));
    assert_eq!(
        value["pairs"][0]["state"], "pending",
        "MCP016-FIELD-ALLOWLIST: a pair without a result carries its state"
    );
    assert_eq!(value["pairs"][0]["reason"], "base-unconfirmed");
    assert!(
        value["pairs"][0].get("level").is_none(),
        "MCP016-FIELD-ALLOWLIST: a pending pair has no level"
    );
}

/// D5: a typical page fits the target of 800 tokens.
#[test]
fn a_typical_page_fits_its_token_target() {
    let page = typical_page();
    let len = conflicts_wire_len(&page);
    assert!(
        len > 0 && len <= TARGET,
        "MCP016-PAGE-FIT: the typical page (3 ⚡ with 2 files and 2 ranges, 2 ⚠ with 3 paths, the limits) is {len} B, over {TARGET} B ({MCP_PAGE_TOKENS} tokens)"
    );
    let mut fitted = page.clone();
    fit_conflicts(&mut fitted, TARGET, MCP_FIT_BYTES);
    assert_eq!(
        fitted, page,
        "MCP016-PAGE-FIT: a page that fits is not touched"
    );
    assert!(!fitted.truncated && fitted.cursor.is_none());
}

/// D5: the cut keeps the ⚡ first and says it cut.
#[test]
fn fit_conflicts_keeps_conflicts_first_and_marks_the_cut() {
    let mut pairs: Vec<McpPair> = (0..10)
        .map(|i| {
            conflict_pair(
                &format!("shop-feat-{i}"),
                vec![file("src/pago.rs", 2), file("src/cart.rs", 2)],
            )
        })
        .collect();
    pairs.extend((0..30).map(|i| {
        overlap_pair(
            &format!("shop-ov-{i}"),
            (0..3)
                .map(|n| Untrusted::new(format!("src/m{n}/f.rs")))
                .collect(),
        )
    }));
    let original = answer(pairs);
    assert!(
        conflicts_wire_len(&original) > TARGET,
        "the fixture must be over the target"
    );

    let mut cut = original.clone();
    fit_conflicts(&mut cut, TARGET, MCP_FIT_BYTES);

    assert!(
        cut.truncated,
        "MCP016-PAGE-FIT: a cut page must say truncated"
    );
    assert_eq!(
        cut.cursor.as_deref(),
        Some(MCP_CURSOR_PLACEHOLDER),
        "MCP016-PAGE-FIT: a cut page carries the cursor placeholder"
    );
    assert!(
        !cut.pairs.is_empty() && cut.pairs.len() < original.pairs.len(),
        "MCP016-PAGE-FIT: the cut takes pairs off the end, never all"
    );
    assert_eq!(
        cut.pairs[..],
        original.pairs[..cut.pairs.len()],
        "MCP016-PAGE-FIT: the kept pairs are the first ones, in order (⚡ before ⚠)"
    );
    assert_eq!(cut.pairs[0].level, Some(PairLevel::Conflict));
    let len = conflicts_wire_len(&cut);
    assert!(
        len <= TARGET,
        "MCP016-PAGE-FIT: the cut page is {len} B, over the target of {TARGET} B"
    );
    // The longest prefix that fits: one more pair would not.
    let mut one_more = cut.clone();
    one_more.pairs = original.pairs[..cut.pairs.len() + 1].to_vec();
    assert!(
        conflicts_wire_len(&one_more) > TARGET,
        "MCP016-PAGE-FIT: the cut dropped a pair that still fitted"
    );
}

/// D12: a single pair over the bound is cut in its files and marked.
#[test]
fn a_single_pair_over_the_bound_is_cut_in_its_files() {
    let files: Vec<_> = (0..200)
        .map(|i| file(&format!("src/mod{i}/lib.rs"), 50))
        .collect();
    let mut conflict = answer(vec![conflict_pair("shop-feat-b", files)]);
    assert!(conflicts_wire_len(&conflict) > MCP_FIT_BYTES);
    fit_conflicts(&mut conflict, TARGET, MCP_FIT_BYTES);
    assert_eq!(
        conflict.pairs.len(),
        1,
        "MCP016-PAGE-FIT: the first pair is always kept"
    );
    let kept = conflict.pairs[0].conflict.as_ref().expect("still a ⚡");
    assert!(
        kept.truncated,
        "MCP016-PAGE-FIT: a pair cut in its files must say conflict.truncated"
    );
    assert!(
        !kept.files.is_empty() && kept.files.len() < 200,
        "MCP016-PAGE-FIT: the cut keeps some files and drops the rest, kept {}",
        kept.files.len()
    );
    let len = conflicts_wire_len(&conflict);
    assert!(
        len <= MCP_FIT_BYTES,
        "MCP016-PAGE-FIT: the cut pair is {len} B, over MCP_FIT_BYTES ({MCP_FIT_BYTES} B)"
    );

    let paths = (0..200)
        .map(|i| Untrusted::new(format!("{i:03}-{}", "p".repeat(1000))))
        .collect();
    let mut overlap = answer(vec![overlap_pair("shop-feat-c", paths)]);
    assert!(conflicts_wire_len(&overlap) > MCP_FIT_BYTES);
    fit_conflicts(&mut overlap, TARGET, MCP_FIT_BYTES);
    assert_eq!(overlap.pairs.len(), 1);
    assert!(
        overlap.pairs[0].overlap_partial,
        "MCP016-PAGE-FIT: an overlap list cut must say overlap_partial"
    );
    assert!(
        !overlap.pairs[0].overlap.is_empty() && overlap.pairs[0].overlap.len() < 200,
        "MCP016-PAGE-FIT: the cut keeps some paths and drops the rest"
    );
    assert!(conflicts_wire_len(&overlap) <= MCP_FIT_BYTES);
}

/// SEC-12 / D6: hostile texts leave escaped, names at 100 characters, paths at 1.024 bytes.
#[test]
fn hostile_paths_and_names_are_escaped_and_cut() {
    let hostile = "\u{1b}]52;c;cHduZWQ=\u{7}evil\u{202e}rtl\u{61c}\u{2028}\u{2063}\u{e0041}tail";
    let long_name = "n".repeat(300);
    let long_path = format!("{hostile}/{}", "p".repeat(4096));
    let mut pair = conflict_pair("unused", vec![file(hostile, 1), file(&long_path, 1)]);
    pair.a = side(&long_name);
    pair.b = side(hostile);
    let mut c = answer(vec![pair]);
    c.worktree = Some(UntrustedName::new(&long_name));
    let value = wire(&c);

    let forbidden = [
        '\u{1b}',
        '\u{7}',
        '\u{202e}',
        '\u{61c}',
        '\u{2028}',
        '\u{2063}',
        '\u{e0041}',
    ];
    let text = value.to_string();
    for ch in forbidden {
        assert!(
            !text.contains(ch) && !text.contains(&format!("\\u{:04x}", ch as u32)),
            "MCP016-SANITIZE-VIEW: U+{:04X} reached the model unescaped",
            ch as u32
        );
    }
    assert!(
        !text.contains("cHduZWQ"),
        "MCP016-SANITIZE-VIEW: the body of the OSC 52 sequence survived"
    );

    for name in [
        &value["worktree"]["untrusted"],
        &value["pairs"][0]["a"]["worktree"]["untrusted"],
    ] {
        let name = name.as_str().expect("a name");
        assert_eq!(
            name.chars().count(),
            MAX_MCP_NAME_CHARS,
            "MCP016-SANITIZE-VIEW: a 300-character name is cut at {MAX_MCP_NAME_CHARS} characters"
        );
    }
    assert_eq!(
        value["pairs"][0]["a"]["worktree"]["truncated"], true,
        "MCP016-SANITIZE-VIEW: a cut name is marked truncated"
    );
    let path = &value["pairs"][0]["conflict"]["files"][1]["path"];
    assert!(
        path["untrusted"].as_str().expect("a path").len() <= MAX_MCP_PATH_BYTES,
        "MCP016-SANITIZE-VIEW: a 4 KiB path is cut at {MAX_MCP_PATH_BYTES} bytes"
    );
    assert_eq!(
        path["truncated"], true,
        "MCP016-SANITIZE-VIEW: a cut path is marked truncated"
    );
}
