//! The pure parts of `mcp.conflicts` (US-MCP-016, DS-US-MCP-016): where the caller is, the order
//! and the sides of a page, what a pair without a result says, the age and the cursor table.
//! Dedicated file (wired from `mcp_conflicts.rs` by one `#[cfg(test)] #[path]` line): the
//! criteria's tests must not live in the production file. Real folders only under a temporary
//! directory (NFR-01).

use std::path::{Path, PathBuf};

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, DivergenceView, HeadView, RepoStateView, RepoView,
    WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::{McpConflictsScope, McpPair, McpPairCounts, McpSide};
use gitraptor_api::prediction::{
    ConflictFileView, ConflictKindView, ConflictView, LineRange, MergeBaseKind, PairDetail,
    PairLevel, PairReason, PairSideView, PairStateView, PairSummary,
};
use gitraptor_api::{Untrusted, UntrustedName};

use super::{ConflictsLocated, build_page, locate_conflicts};
use crate::channel::mcp_status::{CursorEntry, McpCursors};
use crate::predict::store::{PredictionStore, predict_read};

const NOW_MS: i64 = 1_793_001_246_567;

// ----- Builders -------------------------------------------------------------------------------

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

fn worktree(root: &Path, main: bool) -> WorktreeView {
    WorktreeView {
        path: Untrusted::from_os(root.as_os_str()),
        main,
        admin_name: None,
        status: WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new("main"),
            },
            counts: Default::default(),
            changes: Vec::new(),
            divergence: DivergenceView::BaseMissing,
        },
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
    }
}

fn repo(id: &str, common: &Path, worktrees: Vec<WorktreeView>) -> RepoView {
    RepoView {
        repo_id: id.to_owned(),
        state: RepoStateView::Observed,
        path: Untrusted::from_os(common.as_os_str()),
        base: BaseBranchView {
            name: Some(UntrustedName::new("main")),
            status: BaseStatusView::Confirmed,
        },
        worktrees,
        fetched_utc_ms: None,
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
        prediction: None,
    }
}

fn root(name: &str) -> Untrusted {
    Untrusted::new(format!("/w/shop/{name}"))
}

fn wt(name: &str) -> PairSideView {
    PairSideView::Worktree { root: root(name) }
}

fn range(start: u32, len: u32) -> LineRange {
    LineRange { start, len }
}

fn summary(
    id: &str,
    a: PairSideView,
    b: PairSideView,
    state: PairStateView,
    level: Option<PairLevel>,
    computed: Option<i64>,
) -> PairSummary {
    PairSummary {
        pair_id: id.to_owned(),
        rev: 1,
        a,
        b,
        state,
        reason: None,
        level,
        computed_at_utc_ms: computed,
        overlap_files: 0,
        overlap_partial: false,
        conflict_files: 0,
        conflict_truncated: false,
    }
}

fn detail(summary: PairSummary) -> PairDetail {
    PairDetail {
        summary,
        overlap: Vec::new(),
        conflict: None,
    }
}

/// A ⚡ current between `a` and `b`, with one file: `ours` are the ranges of `a`.
fn conflict(
    id: &str,
    a: PairSideView,
    b: PairSideView,
    ours: Vec<LineRange>,
    theirs: Vec<LineRange>,
) -> PairDetail {
    let mut s = summary(
        id,
        a,
        b,
        PairStateView::Current,
        Some(PairLevel::Conflict),
        Some(NOW_MS - 12_000),
    );
    s.conflict_files = 1;
    PairDetail {
        summary: s,
        overlap: Vec::new(),
        conflict: Some(ConflictView {
            merge_base: MergeBaseKind::Single,
            files: vec![ConflictFileView {
                path: Untrusted::new("src/pago.rs"),
                kind: ConflictKindView::Content,
                ours,
                theirs,
                hunks_truncated: false,
                has_merge_rule: false,
            }],
            truncated: false,
            rewrites_escalated: false,
        }),
    }
}

fn overlap(id: &str, a: PairSideView, b: PairSideView, state: PairStateView) -> PairDetail {
    let mut s = summary(
        id,
        a,
        b,
        state,
        Some(PairLevel::Overlap),
        Some(NOW_MS - 5_000),
    );
    s.overlap_files = 1;
    PairDetail {
        summary: s,
        overlap: vec![Untrusted::new("src/cart.rs")],
        conflict: None,
    }
}

fn current_none(id: &str, a: PairSideView, b: PairSideView) -> PairDetail {
    detail(summary(
        id,
        a,
        b,
        PairStateView::Current,
        Some(PairLevel::Clean),
        Some(NOW_MS - 1_000),
    ))
}

fn calculating(id: &str, a: PairSideView, b: PairSideView) -> PairDetail {
    detail(summary(id, a, b, PairStateView::Calculating, None, None))
}

fn pending(id: &str, a: PairSideView, b: PairSideView, reason: PairReason) -> PairDetail {
    let mut s = summary(id, a, b, PairStateView::Pending, None, None);
    s.reason = Some(reason);
    detail(s)
}

fn not_computable(id: &str, a: PairSideView, b: PairSideView) -> PairDetail {
    let mut s = summary(id, a, b, PairStateView::NotComputable, None, None);
    s.reason = Some(PairReason::Shallow);
    detail(s)
}

fn page(
    details: &[PairDetail],
    caller: Option<&str>,
) -> (McpPairCounts, Vec<McpPair>, Option<(u8, String)>) {
    let caller = caller.map(|n| PathBuf::from(format!("/w/shop/{n}")));
    build_page(
        details,
        caller.as_deref(),
        Some(&UntrustedName::new("main")),
        None,
        NOW_MS,
    )
}

fn side_name(side: &McpSide) -> String {
    match side {
        McpSide::Worktree(name) => name.raw().to_owned(),
        McpSide::Base(_) => "base".to_owned(),
    }
}

fn names(pair: &McpPair) -> (String, String) {
    (side_name(&pair.a), side_name(&pair.b))
}

fn is_conflict(pair: &McpPair) -> bool {
    pair.level == Some(PairLevel::Conflict)
}

// ----- Tests ----------------------------------------------------------------------------------

/// Esquema (calculando): a pair without a result is declared `calculating`, has no level, is
/// counted, and nothing else is invented.
#[test]
fn declares_a_calculating_prediction_without_inventing_pairs() {
    let (counts, pairs, next) = page(
        &[calculating("p1", wt("wt-a"), PairSideView::Base {})],
        Some("wt-a"),
    );
    assert_eq!(pairs.len(), 1, "MCP016-CALCULATING: no pair is invented");
    assert_eq!(pairs[0].state, Some(PairStateView::Calculating));
    assert_eq!(
        pairs[0].level, None,
        "MCP016-CALCULATING: no level without a result"
    );
    assert_eq!(pairs[0].age_s, None);
    assert_eq!(counts.calculating, 1, "MCP016-CALCULATING: counts says it");
    assert_eq!(
        (counts.conflict, counts.overlap, counts.none, counts.pending),
        (0, 0, 0, 0)
    );
    assert_eq!(next, None);
    let (counts, pairs, _) = page(&[], Some("wt-a"));
    assert!(
        pairs.is_empty(),
        "MCP016-CALCULATING: nothing published, nothing said"
    );
    assert_eq!(counts, McpPairCounts::default());
}

/// Esquema (pendiente): `pending` with its reason, no level, counted.
#[test]
fn declares_a_pending_prediction_with_its_reason_without_inventing_pairs() {
    let (counts, pairs, _) = page(
        &[pending(
            "p1",
            wt("wt-a"),
            PairSideView::Base {},
            PairReason::BaseUnconfirmed,
        )],
        Some("wt-a"),
    );
    assert_eq!(pairs.len(), 1, "MCP016-PENDING: no pair is invented");
    assert_eq!(pairs[0].state, Some(PairStateView::Pending));
    assert_eq!(pairs[0].reason, Some(PairReason::BaseUnconfirmed));
    assert_eq!(
        pairs[0].level, None,
        "MCP016-PENDING: no level while pending"
    );
    assert!(matches!(&pairs[0].b, McpSide::Base(Some(name)) if name.raw() == "main"));
    assert_eq!(counts.pending, 1, "MCP016-PENDING: counts says it");
}

/// D8: `level` only on `current` and `recalculating` (the last result's), `none` explicit; a
/// `current` pair omits its state.
#[test]
fn level_is_absent_without_a_result_and_none_is_explicit() {
    let mut recalculating = overlap("p5", wt("wt-a"), wt("wt-e"), PairStateView::Recalculating);
    recalculating.summary.level = Some(PairLevel::Overlap);
    let details = [
        conflict(
            "p1",
            wt("wt-a"),
            wt("wt-b"),
            vec![range(40, 19)],
            vec![range(40, 19)],
        ),
        current_none("p2", wt("wt-a"), wt("wt-c")),
        recalculating,
        calculating("p3", wt("wt-a"), wt("wt-d")),
        pending(
            "p4",
            wt("wt-a"),
            PairSideView::Base {},
            PairReason::PowerSaving,
        ),
        not_computable("p6", wt("wt-a"), wt("wt-f")),
    ];
    let (counts, pairs, _) = page(&details, Some("wt-a"));
    let by = |b: &str| {
        pairs
            .iter()
            .find(|p| names(p).1 == b)
            .unwrap_or_else(|| panic!("no pair with {b}"))
    };

    assert_eq!(by("wt-b").level, Some(PairLevel::Conflict));
    assert_eq!(
        by("wt-b").state,
        None,
        "MCP016-LEVEL: a current pair omits its state"
    );
    assert_eq!(
        by("wt-c").level,
        Some(PairLevel::Clean),
        "MCP016-LEVEL: none is explicit"
    );
    assert_eq!(by("wt-c").state, None);
    assert_eq!(by("wt-e").state, Some(PairStateView::Recalculating));
    assert_eq!(
        by("wt-e").level,
        Some(PairLevel::Overlap),
        "MCP016-LEVEL: the last result's level"
    );
    for b in ["wt-d", "base", "wt-f"] {
        assert_eq!(
            by(b).level,
            None,
            "MCP016-LEVEL: {b} has no result, so no level"
        );
    }
    assert_eq!(by("wt-f").reason, Some(PairReason::Shallow));
    assert_eq!(
        (
            counts.conflict,
            counts.overlap,
            counts.none,
            counts.calculating,
            counts.pending,
            counts.not_computable
        ),
        (1, 1, 1, 1, 1, 1),
        "MCP016-LEVEL: counts by class, a recalculating pair by its last level"
    );
}

/// P6: the main worktree is a worktree like the others: its pairs are those that include it as a
/// `worktree` side, and it goes on side a.
#[test]
fn the_main_worktree_gets_the_pairs_that_include_it() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let (common, main_root) = (base.join("shop.git"), base.join("shop"));
    for dir in [&common, &main_root] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let repos = [repo("r1", &common, vec![worktree(&main_root, true)])];
    assert_eq!(
        locate_conflicts(Some(main_root.join("src").as_path()), &repos, None),
        ConflictsLocated::Worktree {
            repo: 0,
            worktree: 0
        },
        "MCP016-MAIN-WORKTREE: the main worktree is located like any other"
    );
    let main_side = PairSideView::Worktree {
        root: Untrusted::new("/w/shop/shop"),
    };
    let (_, pairs, _) = page(
        &[conflict(
            "p1",
            wt("wt-b"),
            main_side,
            vec![range(1, 2)],
            vec![range(3, 4)],
        )],
        Some("shop"),
    );
    assert_eq!(names(&pairs[0]), ("shop".to_owned(), "wt-b".to_owned()));
}

/// P6/D8: no pair published while others calculate is not "no conflicts": the page says what it
/// does not know.
#[test]
fn no_pairs_while_calculating_is_not_no_conflicts() {
    let details = [
        calculating("p1", wt("wt-a"), wt("wt-b")),
        calculating("p2", wt("wt-a"), PairSideView::Base {}),
    ];
    let (counts, pairs, _) = page(&details, Some("wt-a"));
    assert_eq!(counts.conflict + counts.overlap + counts.none, 0);
    assert_eq!(
        counts.calculating, 2,
        "MCP016-MAIN-WORKTREE: the page declares them calculating"
    );
    assert!(
        pairs
            .iter()
            .all(|p| p.state == Some(PairStateView::Calculating)),
        "MCP016-MAIN-WORKTREE: an answer whose pairs are all calculating says so pair by pair"
    );
    assert_eq!(pairs.len(), 2);
}

/// D11: a worktree deleted during the session is `Missing`; a folder under the common directory
/// of the repo but under no worktree is `RepoOnly`; any other is `Outside`.
#[test]
fn a_deleted_worktree_is_worktree_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let (common, main_root, linked) = (base.join("shop.git"), base.join("shop"), base.join("wt-x"));
    for dir in [&common, &main_root, &linked, &base.join("elsewhere")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let repos = [repo(
        "r1",
        &common,
        vec![worktree(&main_root, true), worktree(&linked, false)],
    )];
    let gone = base.join("wt-gone");
    assert_eq!(
        locate_conflicts(None, &repos, Some(&("r1".to_owned(), gone))),
        ConflictsLocated::Missing { repo: 0 },
        "MCP016-DELETED: a root that is gone says worktree-missing, as status does"
    );
    assert_eq!(
        locate_conflicts(None, &repos, Some(&("r1".to_owned(), linked.clone()))),
        ConflictsLocated::Outside,
        "an unreadable folder that still exists is not a deleted worktree"
    );
    assert_eq!(
        locate_conflicts(Some(common.join("hooks").as_path()), &repos, None),
        ConflictsLocated::RepoOnly { repo: 0 },
        "MCP016-DELETED: under the common directory and no worktree: the repo scope (D11)"
    );
    assert_eq!(
        locate_conflicts(Some(linked.as_path()), &repos, None),
        ConflictsLocated::Worktree {
            repo: 0,
            worktree: 1
        }
    );
    assert_eq!(
        locate_conflicts(Some(base.join("elsewhere").as_path()), &repos, None),
        ConflictsLocated::Outside
    );
}

/// D5: ⚡ current, ⚡ recalculating, ⚠ current, ⚠ recalculating, calculating, pending,
/// not-computable, none; a cursor continues without a repeat or a loss.
#[test]
fn conflicts_come_first_then_overlaps() {
    let mut conflict_recalc = conflict(
        "c2",
        wt("wt-a"),
        wt("wt-2"),
        vec![range(1, 1)],
        vec![range(1, 1)],
    );
    conflict_recalc.summary.state = PairStateView::Recalculating;
    let mut overlap_recalc = overlap("o2", wt("wt-a"), wt("wt-4"), PairStateView::Recalculating);
    overlap_recalc.summary.level = Some(PairLevel::Overlap);
    // In the reverse of the expected order.
    let details = [
        current_none("n1", wt("wt-a"), wt("wt-8")),
        not_computable("x1", wt("wt-a"), wt("wt-7")),
        pending("q1", wt("wt-a"), wt("wt-6"), PairReason::Dormant),
        calculating("k1", wt("wt-a"), wt("wt-5")),
        overlap_recalc,
        overlap("o1", wt("wt-a"), wt("wt-3"), PairStateView::Current),
        conflict_recalc,
        conflict(
            "c1",
            wt("wt-a"),
            wt("wt-1"),
            vec![range(1, 1)],
            vec![range(1, 1)],
        ),
    ];
    let (_, pairs, next) = page(&details, Some("wt-a"));
    let order: Vec<String> = pairs.iter().map(|p| names(p).1).collect();
    assert_eq!(
        order,
        [
            "wt-1", "wt-2", "wt-3", "wt-4", "wt-5", "wt-6", "wt-7", "wt-8"
        ],
        "MCP016-ORDER: ⚡ current, ⚡ recalculating, ⚠ current, ⚠ recalculating, calculating, pending, not-computable, none"
    );
    assert_eq!(next, None);

    // 60 pairs: a page is at most 50 and the cursor continues where it stopped.
    let many: Vec<PairDetail> = (0..60)
        .map(|i| {
            conflict(
                &format!("{i:016x}"),
                wt("wt-a"),
                wt(&format!("wt-{i:02}")),
                vec![range(1, 1)],
                vec![range(1, 1)],
            )
        })
        .collect();
    let base = UntrustedName::new("main");
    let caller = PathBuf::from("/w/shop/wt-a");
    let (counts, first, cursor) = build_page(&many, Some(&caller), Some(&base), None, NOW_MS);
    assert_eq!(
        counts.conflict, 60,
        "MCP016-ORDER: counts are of the whole scope"
    );
    assert_eq!(
        first.len(),
        50,
        "MCP016-ORDER: 50 pairs is the most of a page"
    );
    let cursor = cursor.expect("MCP016-ORDER: a page that leaves pairs gives where it stopped");
    let (_, rest, end) = build_page(&many, Some(&caller), Some(&base), Some(&cursor), NOW_MS);
    assert_eq!(end, None);
    let mut all: Vec<String> = first.iter().chain(&rest).map(|p| names(p).1).collect();
    assert_eq!(all.len(), 60, "MCP016-ORDER: no pair lost or repeated");
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 60, "MCP016-ORDER: no pair repeated");
}

/// D4: a conflicts cursor lives in the table of the connection with its repo; the variant is
/// its own, so a cursor of the other list is not one of conflicts.
#[test]
fn a_status_cursor_is_not_a_conflicts_cursor() {
    let mut cursors = McpCursors::default();
    let status = cursors
        .mint(CursorEntry::Worktrees {
            repo_id: "r1".to_owned(),
            after: None,
        })
        .expect("a cursor");
    let conflicts = cursors
        .mint(CursorEntry::Conflicts {
            repo_id: "r1".to_owned(),
            scope: McpConflictsScope::Repo,
            root: None,
            after: (0, "0000000000000001".to_owned()),
        })
        .expect("a cursor");
    assert!(
        !matches!(cursors.get(&status), Some(CursorEntry::Conflicts { .. })),
        "MCP016-CURSOR-DAEMON: a status cursor is not a conflicts cursor"
    );
    let entry = cursors.get(&conflicts).expect("the conflicts cursor");
    assert!(matches!(entry, CursorEntry::Conflicts { .. }));
    assert_eq!(
        entry.repo_id(),
        "r1",
        "MCP016-CURSOR-DAEMON: it is for its repo"
    );
    assert!(cursors.get("0123456789abcdef").is_none());
}

/// D7: `age_s` is `now - computed_at_utc_ms` by the daemon's clock, in seconds, never negative.
#[test]
fn age_is_computed_by_the_daemon_and_never_negative() {
    let past = conflict(
        "p1",
        wt("wt-a"),
        wt("wt-b"),
        vec![range(1, 1)],
        vec![range(1, 1)],
    );
    let mut future = conflict(
        "p2",
        wt("wt-a"),
        wt("wt-c"),
        vec![range(1, 1)],
        vec![range(1, 1)],
    );
    future.summary.computed_at_utc_ms = Some(NOW_MS + 90_000);
    let (_, pairs, _) = page(&[past, future], Some("wt-a"));
    let age = |b: &str| pairs.iter().find(|p| names(p).1 == b).unwrap().clone();
    assert_eq!(age("wt-b").age_s, Some(12), "MCP016-AGE: 12 s ago");
    assert_eq!(age("wt-b").computed_at_utc_ms, Some(NOW_MS - 12_000));
    assert_eq!(
        age("wt-c").age_s,
        Some(0),
        "MCP016-AGE: a clock that went back is 0, not negative"
    );
}

/// D14: in the worktree scope the caller goes on side a, with its ranges; when the engine has it
/// on b the view swaps the sides and their ranges.
#[test]
fn the_caller_is_side_a_with_its_ranges() {
    let engine_has_caller_on_b = conflict(
        "p1",
        wt("wt-b"),
        wt("wt-a"),
        vec![range(10, 3)],
        vec![range(40, 19)],
    );
    let engine_has_caller_on_a = conflict(
        "p2",
        wt("wt-a"),
        wt("wt-c"),
        vec![range(5, 2)],
        vec![range(6, 4)],
    );
    let (_, pairs, _) = page(
        &[engine_has_caller_on_b, engine_has_caller_on_a],
        Some("wt-a"),
    );

    let with_b = pairs
        .iter()
        .find(|p| names(p).1 == "wt-b")
        .expect("the pair with wt-b");
    assert_eq!(
        names(with_b).0,
        "wt-a",
        "MCP016-SIDE-A: the caller on side a"
    );
    let file = &with_b.conflict.as_ref().expect("a ⚡").files[0];
    assert_eq!(
        file.a,
        vec![[40, 19]],
        "MCP016-SIDE-A: the ranges of the caller go on a"
    );
    assert_eq!(file.b, vec![[10, 3]], "MCP016-SIDE-A: and the other's on b");

    let with_c = pairs
        .iter()
        .find(|p| names(p).1 == "wt-c")
        .expect("the pair with wt-c");
    let file = &with_c.conflict.as_ref().expect("a ⚡").files[0];
    assert_eq!(
        (file.a.clone(), file.b.clone()),
        (vec![[5, 2]], vec![[6, 4]]),
        "no swap when it is already on a"
    );
}

/// D13: a repo the predictor has not published is not "no pairs": the store tells them apart,
/// and reading it gives nothing and no error.
#[test]
fn an_unpublished_prediction_is_declared() {
    let store = PredictionStore::new(true);
    assert!(
        !store.published("r1"),
        "MCP016-UNPUBLISHED: a repo without a publication is unpublished"
    );
    assert!(predict_read(&store, "r1", None).is_empty());
    store.record("r1", Vec::new(), &[]);
    assert!(
        store.published("r1"),
        "MCP016-UNPUBLISHED: a published repo with no pairs is published"
    );
    assert!(predict_read(&store, "r1", None).is_empty());
}
