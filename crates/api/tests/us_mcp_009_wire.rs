//! US-MCP-009 T002: the `commit` contract of `crates/api`: message validation, the shapes that
//! travel on the wire, the call limit and the closed tool codes.

use std::collections::BTreeSet;
use std::time::Duration;

use gitraptor_api::catalog::{
    CommitDone, MAX_COMMIT_MESSAGE_BYTES, MAX_REPORTED_COMMIT_PATHS, MessageProblem, RejectReason,
    check_commit_message,
};
use gitraptor_api::mcp_view::{
    MCP_COMMIT_CALL_LIMIT, MCP_COMMIT_MARGIN, MCP_COMMIT_TOKENS, MCP_PROGRESS_INTERVAL,
    MCP_QUEUE_WAIT, MCP_TIME_LIMIT, McpCommitView, McpToolError, PRIOR_DEADLINE_BOUND,
};
use gitraptor_api::methods::{OperationFailedData, StepFailureView};
use gitraptor_api::timemachine::McpOperationRunResult;
use gitraptor_api::{Untrusted, UntrustedName};
use serde_json::json;

#[test]
fn commit_messages_are_checked_like_the_story_says() {
    let ok = |m: &str| check_commit_message(m);
    // required: empty or blank after trim
    for blank in ["", "   ", "\n\t \n"] {
        assert_eq!(
            ok(blank),
            Err(MessageProblem::Required),
            "{blank:?} must be `required`"
        );
    }
    // allowed: newline, tab, CRLF, text, multi-byte text
    for good in [
        "fix: a thing",
        "subject\n\nbody with\ttab",
        "subject\r\n\r\nbody",
        "feat: acentos y eñes ñ é 日本語",
        "x",
    ] {
        assert_eq!(ok(good), Ok(()), "{good:?} must be accepted");
    }
    // invalid chars: NUL, control, lone CR, bidi, zero width
    for (bad, why) in [
        ("a\0b", "NUL"),
        ("a\u{1b}[31mb", "ESC"),
        ("a\u{7f}b", "DEL"),
        ("a\rb", "a lone CR"),
        ("a\r", "a trailing CR"),
        ("a\u{202e}b", "bidi override"),
        ("a\u{2066}b", "bidi isolate"),
        ("a\u{200b}b", "zero width space"),
        ("a\u{feff}b", "BOM"),
        ("a\u{2028}b", "line separator"),
        ("a\u{85}b", "C1 control"),
    ] {
        assert_eq!(
            ok(bad),
            Err(MessageProblem::InvalidChars),
            "{why} must be `invalid-chars`"
        );
    }
    assert_eq!(MessageProblem::Required.as_str(), "required");
    assert_eq!(MessageProblem::TooLong.as_str(), "too-long");
    assert_eq!(MessageProblem::InvalidChars.as_str(), "invalid-chars");
}

#[test]
fn a_message_over_16_kib_is_too_long_before_reading_chars() {
    assert_eq!(MAX_COMMIT_MESSAGE_BYTES, 16 * 1024);
    // The limit is in bytes and inclusive.
    assert_eq!(
        check_commit_message(&"a".repeat(MAX_COMMIT_MESSAGE_BYTES)),
        Ok(()),
        "exactly 16 KiB must be accepted"
    );
    assert_eq!(
        check_commit_message(&"a".repeat(MAX_COMMIT_MESSAGE_BYTES + 1)),
        Err(MessageProblem::TooLong)
    );
    // 8193 two-byte chars are 16386 bytes: bytes count, not characters.
    assert_eq!(
        check_commit_message(&"é".repeat(MAX_COMMIT_MESSAGE_BYTES / 2 + 1)),
        Err(MessageProblem::TooLong),
        "the limit counts bytes"
    );
    // Too long wins over invalid chars: the length is checked before any char is walked.
    let mut long_and_bad = "a".repeat(MAX_COMMIT_MESSAGE_BYTES + 1);
    long_and_bad.push('\0');
    assert_eq!(
        check_commit_message(&long_and_bad),
        Err(MessageProblem::TooLong),
        "a message over the limit with a forbidden char must still be `too-long`"
    );
}

#[test]
fn the_commit_shapes_round_trip() {
    // CommitDone
    let done = CommitDone {
        oid: "0123456789abcdef0123456789abcdef01234567".into(),
        paths: vec![Untrusted::new("src/a.rs")],
        paths_total: 1,
    };
    let v = serde_json::to_value(&done).unwrap();
    assert_eq!(
        v,
        json!({"oid":"0123456789abcdef0123456789abcdef01234567",
               "paths":[{"untrusted":"src/a.rs"}],"paths_total":1})
    );
    assert_eq!(serde_json::from_value::<CommitDone>(v).unwrap(), done);
    assert!(
        serde_json::from_value::<CommitDone>(
            json!({"oid":"a","paths":[],"paths_total":0,"extra":1})
        )
        .is_err(),
        "CommitDone must deny unknown fields"
    );
    assert_eq!(MAX_REPORTED_COMMIT_PATHS, 200);

    // McpCommitView: the allowlist of fields, never the message
    let view = McpCommitView {
        worktree: UntrustedName::new("repo"),
        oid: done.oid.clone(),
        paths: done.paths.clone(),
        paths_total: 1,
        truncated: false,
        operation_id: "op-1".into(),
    };
    let v = serde_json::to_value(&view).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "oid",
            "operation_id",
            "paths",
            "paths_total",
            "truncated",
            "worktree"
        ]
    );
    assert_eq!(serde_json::from_value::<McpCommitView>(v).unwrap(), view);

    // StepFailureView
    for (f, s) in [
        (StepFailureView::Changed, "changed"),
        (StepFailureView::Unchanged, "unchanged"),
        (StepFailureView::TimeLimit, "time-limit"),
    ] {
        assert_eq!(serde_json::to_value(f).unwrap(), json!(s));
        assert_eq!(
            serde_json::from_value::<StepFailureView>(json!(s)).unwrap(),
            f
        );
    }

    // OperationFailedData: `failure` only when present
    let plain = OperationFailedData {
        operation_id: "op-1".into(),
        failure: None,
    };
    assert_eq!(
        serde_json::to_value(&plain).unwrap(),
        json!({"operation_id":"op-1"}),
        "no failure must not be serialized"
    );
    let with = OperationFailedData {
        operation_id: "op-1".into(),
        failure: Some(StepFailureView::TimeLimit),
    };
    let v = serde_json::to_value(&with).unwrap();
    assert_eq!(v, json!({"operation_id":"op-1","failure":"time-limit"}));
    assert_eq!(
        serde_json::from_value::<OperationFailedData>(v).unwrap(),
        with
    );
    assert_eq!(
        serde_json::from_value::<OperationFailedData>(json!({"operation_id":"op-1"})).unwrap(),
        plain
    );

    // The three new reject reasons
    for (r, s) in [
        (RejectReason::NothingToCommit, "nothing-to-commit"),
        (RejectReason::PathNotFound, "path-not-found"),
        (RejectReason::GitUnavailable, "git-unavailable"),
    ] {
        assert_eq!(serde_json::to_value(r).unwrap(), json!(s));
        assert_eq!(serde_json::from_value::<RejectReason>(json!(s)).unwrap(), r);
    }

    // The MCP run result carries `commit` and omits it when absent
    let base = json!({
        "operation_id":"op-1","prior_snapshot_id":"s-1","actor":{"actor":"unattributed"},
        "changed_refs":[],"outcome":"done"
    });
    let without: McpOperationRunResult = serde_json::from_value(base.clone()).unwrap();
    assert!(without.commit.is_none());
    assert_eq!(serde_json::to_value(&without).unwrap(), base);
    let mut with_commit = base.clone();
    with_commit["commit"] = serde_json::to_value(&done).unwrap();
    let r: McpOperationRunResult = serde_json::from_value(with_commit.clone()).unwrap();
    assert_eq!(r.commit, Some(done));
    assert_eq!(serde_json::to_value(&r).unwrap(), with_commit);
}

#[test]
fn the_call_limit_is_the_sum_of_its_named_parts() {
    assert_eq!(MCP_QUEUE_WAIT, Duration::from_secs(60));
    assert_eq!(PRIOR_DEADLINE_BOUND, Duration::from_secs(10));
    assert_eq!(MCP_TIME_LIMIT, Duration::from_millis(300_000));
    assert_eq!(MCP_COMMIT_MARGIN, Duration::from_secs(10));
    assert_eq!(
        MCP_COMMIT_CALL_LIMIT,
        MCP_QUEUE_WAIT + PRIOR_DEADLINE_BOUND + MCP_TIME_LIMIT + MCP_COMMIT_MARGIN,
        "the call limit must be the sum of its named parts"
    );
    assert_eq!(MCP_COMMIT_CALL_LIMIT, Duration::from_secs(380));
    assert_eq!(MCP_PROGRESS_INTERVAL, Duration::from_secs(5));
    assert_eq!(MCP_COMMIT_TOKENS, 800);
}

/// The 7 variants of DS-009 are in the closed set; every code is kebab-case and unique. The
/// count is not asserted exactly: DS-010 adds five more variants in the same run.
#[test]
fn mcp_tool_codes_are_kebab_and_closed() {
    let new_009 = [
        (McpToolError::DetachedHead, "detached-head"),
        (McpToolError::NothingToCommit, "nothing-to-commit"),
        (McpToolError::PathNotFound, "path-not-found"),
        (McpToolError::PolicyDenied, "policy-denied"),
        (McpToolError::Aborted, "aborted"),
        (McpToolError::FailedUnchanged, "failed-unchanged"),
        (McpToolError::FailedChanged, "failed-changed"),
    ];
    let mut seen = BTreeSet::new();
    for code in McpToolError::ALL {
        let s = code.as_str();
        assert!(
            !s.is_empty()
                && s.split('-')
                    .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase())),
            "{s} is not kebab-case"
        );
        assert_eq!(serde_json::to_value(code).unwrap(), json!(s));
        assert!(seen.insert(s), "duplicate code {s}");
    }
    assert!(
        McpToolError::ALL.len() >= 28,
        "the 21 of today plus the 7 of safe_commit, got {}",
        McpToolError::ALL.len()
    );
    for (variant, wire) in new_009 {
        assert_eq!(variant.as_str(), wire);
        assert!(
            McpToolError::ALL.contains(&variant),
            "{wire} must be listed in McpToolError::ALL"
        );
        assert_eq!(
            serde_json::from_value::<McpToolError>(json!(wire)).unwrap(),
            variant
        );
    }
}
