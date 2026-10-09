//! Contract tests of the S3 scope by event kind and of the `s3_evidence`
//! log line.

use std::collections::HashMap;
use std::path::PathBuf;

use gitraptor_api::messages::{GitEventDetails, GitEventKind};

use super::{s3_log_fields, s3_scope};
use crate::daemon::{Field, LOG_FILE, LogLimits, Logger};
use crate::detect::{Diagnostics, S3Counts, S3Scope};
use crate::watch::RawEvent;

fn event(kind: GitEventKind, worktree_inferred: bool) -> RawEvent {
    RawEvent {
        worktree: PathBuf::from("/wt/feat-login"),
        kind,
        details: GitEventDetails {
            worktree_inferred,
            ..GitEventDetails::default()
        },
        observed_ms: 1,
        offset_s: 0,
    }
}

/// Every variant of `GitEventKind`, with the scope it must get. Commit,
/// merge and rebase appear twice: placed by Git, and placed by a fallback.
#[test]
fn s3_scope_follows_the_event_kind() {
    use GitEventKind as K;
    let cases = [
        (K::Reset, false, S3Scope::Worktree),
        (K::BranchSwitch, false, S3Scope::Worktree),
        (K::Commit, false, S3Scope::Worktree),
        (K::Merge, false, S3Scope::Worktree),
        (K::Rebase, false, S3Scope::Worktree),
        (K::Commit, true, S3Scope::Repo),
        (K::Merge, true, S3Scope::Repo),
        (K::Rebase, true, S3Scope::Repo),
        (K::BranchUpdate, false, S3Scope::Repo),
        (K::BranchCreate, false, S3Scope::Repo),
        (K::BranchDelete, false, S3Scope::Repo),
        (K::Push, false, S3Scope::Repo),
        (K::WorktreeCreate, false, S3Scope::Repo),
        (K::WorktreeDelete, false, S3Scope::Repo),
        (K::Reconciled, false, S3Scope::Repo),
    ];
    // The table covers the 12 variants: a new one must be added here.
    let mut kinds: Vec<&str> = cases.iter().map(|(k, _, _)| k.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 12, "every GitEventKind has a case");
    for (kind, inferred, want) in cases {
        assert_eq!(
            s3_scope(&event(kind, inferred)),
            want,
            "{} with worktree_inferred={inferred}",
            kind.as_str()
        );
    }
    // The fallback flag only widens the kinds that create a commit: a reset
    // or a switch is always of its worktree.
    for kind in [K::Reset, K::BranchSwitch] {
        assert_eq!(
            s3_scope(&event(kind, true)),
            S3Scope::Worktree,
            "{}",
            kind.as_str()
        );
    }
}

const COUNTERS: [&str; 8] = [
    "sessions_wt",
    "foreign_wt",
    "foreign_other_wt",
    "foreign_daemon",
    "foreign_by_ancestor",
    "foreign_gitdir",
    "foreign_redirected",
    "gits_after_notice",
];

/// SEC-04: the line carries the scope and integer counters, never a path,
/// a name, a pid, an argv or an environment value.
#[test]
fn the_s3_log_line_carries_only_counters() {
    let diag = Diagnostics {
        samples: 9,
        cwd_unreadable: 10,
        placed_by_ancestor: 11,
        ..Diagnostics::default()
    };
    let counts = S3Counts {
        sessions_wt: 1,
        foreign_wt: 2,
        foreign_other_wt: 3,
        foreign_daemon: 4,
        foreign_by_ancestor: 5,
        foreign_gitdir: 6,
        foreign_redirected: 7,
        gits_after_notice: 8,
    };
    // A repo id that is a path: the canary must never reach the line.
    let fields = s3_log_fields(
        "/home/u/canary-wt",
        GitEventKind::Commit,
        "attributed",
        diag,
        S3Scope::Worktree,
        counts,
    );
    for (key, value) in &fields {
        assert!(
            matches!(value, Field::Int(_) | Field::Text(_) | Field::Id(_)),
            "{key} is an integer, a fixed text or an id"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let logger = Logger::open(tmp.path(), LogLimits::default()).unwrap();
    logger.info("s3_evidence", &fields);
    logger.flush();
    let text = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
    let line = text.lines().next().expect("one line");
    let mut words = line.split(' ');
    assert!(words.next().is_some_and(|ms| ms.parse::<i64>().is_ok()));
    assert_eq!(words.next(), Some("INFO"));
    assert_eq!(words.next(), Some("s3_evidence"));
    let pairs: Vec<(&str, &str)> = words
        .map(|w| w.split_once('=').expect("key=value"))
        .collect();
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys.get(..7),
        Some(
            &[
                "repo",
                "event",
                "outcome",
                "samples",
                "s3_cwd_unreadable",
                "s3_placed_by_ancestor",
                "scope",
            ][..]
        ),
        "today's six fields first, then the scope: {line}"
    );
    let map: HashMap<&str, &str> = pairs.iter().copied().collect();
    assert_eq!(map.len(), pairs.len(), "no key repeats: {line}");
    assert_eq!(map.len(), 7 + COUNTERS.len(), "nothing else: {line}");
    assert_eq!(map.get("repo"), Some(&"invalid-id"));
    assert_eq!(map.get("event"), Some(&"commit"));
    assert_eq!(map.get("outcome"), Some(&"attributed"));
    assert_eq!(map.get("scope"), Some(&"worktree"));
    assert_eq!(map.get("samples"), Some(&"9"));
    assert_eq!(map.get("s3_cwd_unreadable"), Some(&"10"));
    assert_eq!(map.get("s3_placed_by_ancestor"), Some(&"11"));
    for (n, key) in COUNTERS.iter().enumerate() {
        let want = (n + 1).to_string();
        assert_eq!(map.get(key), Some(&want.as_str()), "{key}: {line}");
    }
    for (key, value) in &pairs {
        if !["repo", "event", "outcome", "scope"].contains(key) {
            assert!(value.parse::<i64>().is_ok(), "{key} is an integer: {line}");
        }
        assert!(!value.contains('/'), "{key} holds no path: {line}");
    }
    for canary in [
        "canary",
        "/home",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "--git-dir",
        "--work-tree",
    ] {
        assert!(!line.contains(canary), "{canary} leaked: {line}");
    }

    // The repo scope writes its own fixed text.
    let fields = s3_log_fields(
        "3f2a-00ff",
        GitEventKind::Push,
        "ambiguous",
        diag,
        S3Scope::Repo,
        S3Counts::default(),
    );
    assert!(fields.contains(&("scope", Field::Text("repo"))));
    assert!(fields.contains(&("foreign_redirected", Field::Int(0))));
}
