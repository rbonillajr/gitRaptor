//! T002 of DS-US-CKP-006: the model of the published prediction and the commands `update`
//! asks for. Everything goes through `update` (snapshot, `prediction.changed`, page): the
//! names under test are the ones of the DS (`RepoView.prediction`, `Cmd::PredictionList`,
//! `PredictionModel`, `known_conflict`).

use gitraptor_api::Untrusted;
use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, UnavailableReason,
    WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::PREDICTION_CHANGED;
use gitraptor_api::prediction::{
    ConflictFileView, ConflictKindView, ConflictView, LimitCode, LineRange, MergeBaseKind,
    PairDetail, PairLevel, PairSideView, PairStateView, PairSummary, PredictionChanged,
    PredictionPage, PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, Scope, ScopeSnapshot};

use super::prediction::PredictionModel;
use super::update::update;
use crate::model::{Cmd, ConnState, EngineMsg, Model, Msg, Size, Stamped};
use crate::present::i18n::Lang;

const REPO: &str = "r1";
const MAIN: &str = "/w/shop";
const A: &str = "/w/shop/a";
const B: &str = "/w/shop/b";
const C: &str = "/w/shop/c";

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn wt(path: &str, main: bool) -> WorktreeView {
    WorktreeView {
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        path: Untrusted::new(path),
        main,
        admin_name: None,
        status: WorktreeStatus::Unavailable {
            reason: UnavailableReason::Missing,
        },
    }
}

/// A repo snapshot with the shop's worktrees and the given publication.
fn snapshot(seq: u64, pairs: Vec<PairSummary>, truncated: bool) -> Msg {
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: "run".into(),
            scope_seq: seq,
            repo: ApiRepo {
                fetched_utc_ms: None,
                repo_id: REPO.into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/shop/.git"),
                base: BaseBranchView {
                    name: Some(gitraptor_api::UntrustedName::new("main")),
                    status: BaseStatusView::Confirmed,
                },
                worktrees: vec![wt(MAIN, true), wt(A, false), wt(B, false), wt(C, false)],
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
                prediction: Some(PredictionSummary {
                    limits: LimitCode::ALL.to_vec(),
                    pairs,
                    pairs_truncated: truncated,
                }),
            },
        },
    ))))
}

fn pair(
    id: &str,
    rev: u64,
    a: &str,
    b: Option<&str>,
    state: PairStateView,
    level: Option<PairLevel>,
) -> PairSummary {
    PairSummary {
        pair_id: id.into(),
        rev,
        a: PairSideView::Worktree {
            root: Untrusted::new(a),
        },
        b: match b {
            Some(root) => PairSideView::Worktree {
                root: Untrusted::new(root),
            },
            None => PairSideView::Base {},
        },
        state,
        reason: None,
        level,
        computed_at_utc_ms: Some(1),
        overlap_files: 1,
        overlap_partial: false,
        conflict_files: u32::from(level == Some(PairLevel::Conflict)),
        conflict_truncated: false,
    }
}

fn bolt(id: &str, rev: u64) -> PairSummary {
    pair(
        id,
        rev,
        A,
        Some(B),
        PairStateView::Current,
        Some(PairLevel::Conflict),
    )
}

fn overlap(id: &str, rev: u64) -> PairSummary {
    pair(
        id,
        rev,
        A,
        Some(C),
        PairStateView::Current,
        Some(PairLevel::Overlap),
    )
}

fn detail(summary: PairSummary, path: &str) -> PairDetail {
    let conflict = (summary.level == Some(PairLevel::Conflict)).then(|| ConflictView {
        merge_base: MergeBaseKind::Single,
        files: vec![ConflictFileView {
            path: Untrusted::new(path),
            kind: ConflictKindView::Content,
            ours: vec![LineRange { start: 12, len: 7 }],
            theirs: vec![LineRange { start: 14, len: 7 }],
            hunks_truncated: false,
            has_merge_rule: false,
        }],
        truncated: false,
        rewrites_escalated: false,
    });
    PairDetail {
        summary,
        overlap: vec![Untrusted::new(path)],
        conflict,
    }
}

fn page(details: Vec<PairDetail>, next_cursor: Option<&str>) -> Msg {
    engine(EngineMsg::PredictionPage {
        repo_id: REPO.into(),
        page: Box::new(PredictionPage {
            limits: LimitCode::ALL.to_vec(),
            pairs: details,
            next_cursor: next_cursor.map(str::to_owned),
        }),
    })
}

fn changed(seq: u64, pairs: Vec<PairSummary>, removed: &[&str]) -> Msg {
    engine(EngineMsg::Event {
        scope: Scope::Repo {
            repo_id: REPO.into(),
        },
        scope_seq: seq,
        event: Box::new(Event {
            seq,
            kind: PREDICTION_CHANGED.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: serde_json::to_value(PredictionChanged {
                repo_id: REPO.into(),
                pairs,
                removed: removed.iter().map(|s| (*s).to_owned()).collect(),
            })
            .unwrap(),
        }),
    })
}

fn model() -> Model {
    let mut model = Model::new(
        Lang::En,
        Size {
            width: 80,
            height: 24,
        },
    );
    model.conn = ConnState::Live;
    model
}

fn prediction(model: &Model) -> &PredictionModel {
    model
        .engine
        .repo
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .and_then(|r| r.prediction.as_ref())
        .expect("RepoView.prediction is filled from the snapshot's `prediction` (T002)")
}

fn key_of(model: &Model, folder: &str) -> u64 {
    let repo = model.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
    repo.worktrees
        .iter()
        .find(|w| w.name.as_str() == folder)
        .unwrap_or_else(|| panic!("no worktree {folder}"))
        .key
}

/// Every id asked by the `PredictionList` commands that name pairs, in order.
fn asked_ids(cmds: &[Cmd]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::PredictionList { pair_ids, .. } => Some(pair_ids.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn ids(model: &Model) -> Vec<String> {
    prediction(model)
        .pairs
        .iter()
        .map(|p| p.pair_id.clone())
        .collect()
}

// ------------------------------------------------------------ CKP006-REV-RULE

#[test]
fn detail_with_equal_rev_is_shown() {
    let mut m = model();
    let cmds = update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    assert_eq!(
        asked_ids(&cmds),
        ["p1"],
        "the snapshot's ⚡ pair is asked with prediction.list"
    );
    update(
        &mut m,
        page(vec![detail(bolt("p1", 5), "src/api.rs")], None),
    );
    let shown = prediction(&m)
        .details
        .get("p1")
        .expect("a detail with the summary's rev is kept to be shown");
    assert_eq!(shown.rev, 5);
    let files = &shown.conflict.as_ref().expect("the ⚡ detail").files;
    assert_eq!(files[0].path.as_str(), "src/api.rs");
}

#[test]
fn detail_with_greater_rev_waits_for_the_event() {
    let mut m = model();
    update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    update(
        &mut m,
        page(vec![detail(bolt("p1", 6), "src/api.rs")], None),
    );
    assert!(
        !prediction(&m).details.contains_key("p1"),
        "a detail with a greater rev than the summary is not shown yet"
    );
    let cmds = update(&mut m, changed(6, vec![bolt("p1", 6)], &[]));
    let adopted = prediction(&m)
        .details
        .get("p1")
        .expect("the summary reaching the early detail's rev adopts it");
    assert_eq!(adopted.rev, 6);
    assert!(
        asked_ids(&cmds).is_empty(),
        "an adopted early detail needs no second call: {cmds:?}"
    );
}

#[test]
fn detail_with_lower_rev_is_dropped() {
    let mut m = model();
    update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    update(&mut m, page(vec![detail(bolt("p1", 4), "old.rs")], None));
    assert!(
        !prediction(&m).details.contains_key("p1"),
        "a detail with a lower rev than the summary is dropped"
    );
    let cmds = update(&mut m, changed(6, vec![bolt("p1", 6)], &[]));
    assert!(
        !prediction(&m).details.contains_key("p1"),
        "the dropped detail is never adopted later"
    );
    assert_eq!(asked_ids(&cmds), ["p1"], "the new rev is asked again");
}

// ------------------------------------------------------------ CKP006-CHANGED

#[test]
fn changed_batch_replaces_by_pair_id_and_removes() {
    let mut m = model();
    update(
        &mut m,
        snapshot(
            1,
            vec![overlap("pa", 1), overlap("pb", 1), overlap("pc", 1)],
            false,
        ),
    );
    update(
        &mut m,
        changed(2, vec![bolt("pa", 2), overlap("pd", 2)], &["pb"]),
    );
    let ids = ids(&m);
    assert!(
        ids.contains(&"pa".into()) && ids.contains(&"pc".into()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"pd".into()),
        "a new pair is inserted: {ids:?}"
    );
    assert!(
        !ids.contains(&"pb".into()),
        "`removed` drops the pair: {ids:?}"
    );
    assert_eq!(
        ids.len(),
        3,
        "replaced by pair_id, never duplicated: {ids:?}"
    );
    let a = prediction(&m)
        .pairs
        .iter()
        .find(|p| p.pair_id == "pa")
        .unwrap();
    assert_eq!((a.rev, a.level), (2, Some(PairLevel::Conflict)));
}

#[test]
fn older_summary_never_replaces_a_newer_one() {
    let mut m = model();
    update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    update(&mut m, changed(6, vec![overlap("p1", 3)], &[]));
    let kept = &prediction(&m).pairs[0];
    assert_eq!(
        (kept.rev, kept.level),
        (5, Some(PairLevel::Conflict)),
        "a summary with a lower rev must not replace the kept one"
    );
}

#[test]
fn a_pair_absent_from_the_publication_is_never_drawn() {
    let mut m = model();
    update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    update(&mut m, page(vec![detail(bolt("ghost", 5), "x.rs")], None));
    assert_eq!(ids(&m), ["p1"], "the TUI does not invent pairs (P7)");
    assert!(
        !prediction(&m).details.contains_key("ghost"),
        "no detail is kept for a pair the engine did not publish"
    );
}

// ------------------------------------------------------------ CKP006-WANTED

#[test]
fn asks_conflicts_first_in_batches_of_fifty() {
    let mut m = model();
    // The engine's order puts the ⚠ first on purpose.
    let mut pairs: Vec<PairSummary> = (0..28).map(|i| overlap(&format!("o{i:02}"), 5)).collect();
    pairs.push(pair(
        "calc",
        5,
        A,
        Some(B),
        PairStateView::Calculating,
        None,
    ));
    pairs.push(pair(
        "none",
        5,
        B,
        Some(C),
        PairStateView::Current,
        Some(PairLevel::Clean),
    ));
    for i in 0..25 {
        let state = if i < 20 {
            PairStateView::Current
        } else {
            PairStateView::Recalculating
        };
        pairs.push(pair(
            &format!("c{i:02}"),
            5,
            A,
            Some(B),
            state,
            Some(PairLevel::Conflict),
        ));
    }
    let cmds = update(&mut m, snapshot(5, pairs, false));
    let lens: Vec<usize> = cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::PredictionList { pair_ids, .. } => Some(pair_ids.len()),
            _ => None,
        })
        .collect();
    assert_eq!(
        lens,
        [50, 3],
        "53 asked pairs go in batches of 50: {cmds:?}"
    );
    let asked = asked_ids(&cmds);
    assert!(
        asked[..25].iter().all(|id| id.starts_with('c')),
        "⚡ first: {asked:?}"
    );
    assert!(
        asked[25..].iter().all(|id| id.starts_with('o')),
        "then ⚠, never the calculating or clean ones: {asked:?}"
    );
}

#[test]
fn never_asks_twice_for_the_same_rev() {
    let mut m = model();
    let first = update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    assert_eq!(asked_ids(&first), ["p1"]);
    // Another pair changes while p1's call is in flight: p1 is not asked again.
    let other = update(&mut m, changed(6, vec![overlap("p2", 6)], &[]));
    assert!(
        !asked_ids(&other).contains(&"p1".into()),
        "p1's rev 5 is in flight: {other:?}"
    );
    // A new rev invalidates the detail: it is asked again.
    let newer = update(&mut m, changed(7, vec![bolt("p1", 7)], &[]));
    assert!(
        asked_ids(&newer).contains(&"p1".into()),
        "a ⚡ whose rev changed is asked again: {newer:?}"
    );
    // Once its detail arrived it is never asked for that rev again.
    update(
        &mut m,
        page(vec![detail(bolt("p1", 7), "src/api.rs")], None),
    );
    let after = update(&mut m, changed(8, vec![overlap("p3", 8)], &[]));
    assert!(
        !asked_ids(&after).contains(&"p1".into()),
        "p1 already has the detail of its rev: {after:?}"
    );
}

#[test]
fn truncated_summary_walks_the_cursor() {
    let mut m = model();
    let first = update(&mut m, snapshot(5, vec![bolt("p1", 5)], true));
    assert!(
        first.iter().any(|c| matches!(
            c,
            Cmd::PredictionList { pair_ids, cursor: None, .. } if pair_ids.is_empty()
        )),
        "pairs_truncated starts the walk with a prediction.list without pair_ids: {first:?}"
    );
    let second = update(
        &mut m,
        page(vec![detail(overlap("p9", 5), "README.md")], Some("c1")),
    );
    assert!(
        second.iter().any(|c| matches!(
            c,
            Cmd::PredictionList { pair_ids, cursor: Some(cursor), .. }
                if pair_ids.is_empty() && cursor == "c1"
        )),
        "the next_cursor is followed: {second:?}"
    );
    let last = update(&mut m, page(vec![], None));
    assert!(
        !last.iter().any(|c| matches!(
            c,
            Cmd::PredictionList {
                cursor: Some(_),
                ..
            }
        )),
        "no next_cursor ends the walk: {last:?}"
    );
    let p = prediction(&m);
    assert!(
        p.pairs.iter().any(|row| row.pair_id == "p9"),
        "the summaries of the pages are joined: {:?}",
        ids(&m)
    );
    assert!(!p.truncated, "the walk is over");
}

// ------------------------------------------------------------ CKP006-KNOWN-CONFLICT

#[test]
fn row_rises_with_a_known_conflict_only() {
    use PairLevel::{Clean, Conflict, Overlap};
    use PairStateView::{Calculating, Current, Pending, Recalculating};
    let cases = [
        (Current, Some(Conflict), true),
        (Recalculating, Some(Conflict), true),
        (Calculating, None, false),
        (Pending, None, false),
        (Current, Some(Overlap), false),
        (Current, Some(Clean), false),
    ];
    for (state, level, known) in cases {
        let mut m = model();
        update(
            &mut m,
            snapshot(5, vec![pair("p1", 5, A, Some(B), state, level)], false),
        );
        let (a, b, c) = (key_of(&m, "a"), key_of(&m, "b"), key_of(&m, "c"));
        let p = prediction(&m);
        assert_eq!(
            (p.known_conflict(a), p.known_conflict(b)),
            (known, known),
            "{state:?} {level:?}: both sides of the pair"
        );
        assert!(
            !p.known_conflict(c),
            "{state:?} {level:?}: a worktree outside the pair"
        );
    }
}
