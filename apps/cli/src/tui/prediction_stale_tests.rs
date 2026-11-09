//! Regressions of the answers to `prediction.list`: a stale details page never ends the walk,
//! and a refused request is answered too.

use gitraptor_api::Untrusted;
use gitraptor_api::event::Event;
use gitraptor_api::messages::ResyncReason;
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
        changes_actor: None,
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

fn resync() -> Msg {
    engine(EngineMsg::Resync {
        scope: Scope::Repo {
            repo_id: REPO.into(),
        },
        reason: ResyncReason::DaemonRestarted,
    })
}

fn refused() -> Msg {
    engine(EngineMsg::PredictionRefused {
        repo_id: REPO.into(),
    })
}

fn walk_cmds(cmds: &[Cmd]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, Cmd::PredictionList { pair_ids, .. } if pair_ids.is_empty()))
        .count()
}

/// A details answer that arrives after a resync must never be taken for the walk's page: the
/// answers come in the order of the requests, and only the one to a walk request is a walk page.
#[test]
fn a_stale_details_page_never_ends_the_walk() {
    let mut m = model();
    let c1 = update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    assert_eq!(asked_ids(&c1), ["p1"]);
    update(&mut m, resync());
    // C1's answer arrives before the new snapshot and asks nothing useful: p1 is asked again (C2).
    let c2 = update(&mut m, page(vec![], None));
    assert_eq!(asked_ids(&c2), ["p1"], "C2: {c2:?}");
    // The snapshot is cut: the walk starts (W) while C2 is still unanswered.
    let walk = update(&mut m, snapshot(9, vec![bolt("p1", 9)], true));
    assert_eq!(walk_cmds(&walk), 1, "the walk request: {walk:?}");
    // C2's answer has no next_cursor; it must not close the walk.
    update(&mut m, page(vec![], None));
    assert!(
        prediction(&m).truncated,
        "the details answer ended the walk early"
    );
    // The walk's own page is then taken as the walk's.
    let next = update(
        &mut m,
        page(vec![detail(overlap("p9", 9), "README.md")], Some("c1")),
    );
    assert!(
        next.iter().any(|c| matches!(
            c,
            Cmd::PredictionList { cursor: Some(cursor), .. } if cursor == "c1"
        )),
        "the walk goes on with its cursor: {next:?}"
    );
    assert!(ids(&m).contains(&"p9".to_owned()), "{:?}", ids(&m));
}

/// A refused or unreadable `prediction.list` is answered, so the walk does not wait forever: it
/// ends unfinished (`truncated` stays true) and nothing asks in a loop.
#[test]
fn a_refused_walk_ends_unfinished_and_does_not_loop() {
    let mut m = model();
    let first = update(&mut m, snapshot(5, vec![bolt("p1", 5)], true));
    assert_eq!(walk_cmds(&first), 1);
    let after = update(&mut m, refused());
    assert_eq!(walk_cmds(&after), 0, "no retry in a loop: {after:?}");
    assert!(prediction(&m).truncated, "the pairs still missing are told");
    // Not stuck: a later event still asks for what is missing.
    let next = update(&mut m, changed(6, vec![bolt("p2", 6)], &[]));
    assert!(asked_ids(&next).contains(&"p2".to_owned()), "{next:?}");
}

#[test]
fn a_refused_details_request_is_not_asked_again_for_the_same_rev() {
    let mut m = model();
    let first = update(&mut m, snapshot(5, vec![bolt("p1", 5)], false));
    assert_eq!(asked_ids(&first), ["p1"]);
    let after = update(&mut m, refused());
    assert!(asked_ids(&after).is_empty(), "{after:?}");
    let newer = update(&mut m, changed(6, vec![bolt("p1", 6)], &[]));
    assert_eq!(asked_ids(&newer), ["p1"], "a new rev asks again");
}
