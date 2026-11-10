//! US-CKP-022 T005 (DS D9, D11, D20): the state of the graph view. Pure: the asking and the
//! answer travel as [`Cmd`] and [`EngineMsg::Graph`]. Mounted by the expert in `tui/mod.rs` as
//! `#[cfg(test)] mod graph_tests;`.
//!
//! Assumed beyond the DS signatures: `graph::scroll(model, down: bool)` like `diff::scroll`;
//! `Anchor::Lane(u64)` carries the lane's index or key, which these tests never read.

use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::UntrustedName;
use gitraptor_api::event::Event;
use gitraptor_api::graph::GraphLanes;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DivergenceView, HeadView,
    RepoStateView, RepoView as ApiRepo, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::GIT_EVENT;
use gitraptor_api::scope::{RepoSnapshot, Scope, ScopeSnapshot};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::graph::{self, Anchor, REFRESH_MIN};
use super::selection::tests::{model_with, row};
use super::update::update;
use crate::model::{Cmd, ConnState, EngineMsg, GraphFailure, Head, Model, Msg, Notice, Stamped};
use crate::model::{SafeText, WorktreeState};

fn oid(n: u64) -> String {
    format!("{n:040x}")
}

/// An answer with one lane `/w/a` listing `commits` (newest first) and a ready base `main`.
fn lanes(commits: &[u64]) -> Box<GraphLanes> {
    let list: Vec<_> = commits
        .iter()
        .map(|n| {
            serde_json::json!({
                "oid": oid(*n),
                "summary": {"untrusted": format!("commit {n}")},
                "committed_utc_ms": 1_793_091_500_000_i64 + *n as i64,
                "merge": false
            })
        })
        .collect();
    Box::new(
        serde_json::from_value(serde_json::json!({
            "computed_at_utc_ms": 1_793_091_600_000_i64,
            "base": {"ready": {"name": {"untrusted": "main"}}},
            "lanes_total": 1,
            "lanes": [{
                "worktree": {"untrusted": "/w/a"},
                "head": {"branch": {"name": {"untrusted": "feat-a"}}},
                "state": {"ready": {
                    "merge_base": oid(0),
                    "total": {"count": commits.len(), "exact": true},
                    "commits": list
                }}
            }]
        }))
        .unwrap(),
    )
}

/// A model with a repo on screen, a ready row and an engine that offers `graph.lanes`.
fn ready_model() -> Model {
    let mut w = row("a", 1, false);
    w.state = WorktreeState::Ready {
        head: Head::Branch(SafeText::name("feat-a")),
        changes: 0,
        divergence: DivergenceView::NoBase,
    };
    let mut model = model_with(vec![w]);
    model.engine.graph = true;
    model.engine.diff = true;
    model.conn = ConnState::Live;
    model
}

fn asked(cmds: &[Cmd]) -> u64 {
    match cmds.last() {
        Some(Cmd::Graph { ticket, repo_id }) => {
            assert_eq!(repo_id, "r1", "the graph is asked of the repo on screen");
            *ticket
        }
        other => panic!("expected Cmd::Graph, got {other:?}"),
    }
}

fn state(model: &mut Model) -> &mut graph::GraphState {
    model.ui.graph.as_mut().expect("the graph view is open")
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Cmd> {
    update(model, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

#[test]
fn refresh_spacing_is_twice_the_last_duration_and_one_in_flight() {
    let mut model = ready_model();
    let ticket = asked(&graph::open(&mut model));
    assert_eq!(
        state(&mut model).in_flight,
        Some(ticket),
        "opening leaves one request in flight"
    );
    graph::apply_reply(&mut model, ticket, Ok(lanes(&[1])));
    assert_eq!(
        state(&mut model).in_flight,
        None,
        "the reply frees the slot"
    );

    // A slow answer (3 s): the next one waits 2 x 3 s = 6 s, not the 1 s floor.
    let t0 = Instant::now();
    {
        let s = state(&mut model);
        s.last_request = Some(t0);
        s.last_duration = Duration::from_secs(3);
    }
    graph::mark_stale(&mut model);
    assert!(state(&mut model).stale);
    assert!(
        graph::tick(&mut model, t0 + Duration::from_secs(5)).is_empty(),
        "stale but only 5 s after a 3 s answer: wait 2 x duration"
    );
    let cmds = graph::tick(&mut model, t0 + Duration::from_secs(6));
    assert_eq!(cmds.len(), 1, "after 6 s the view asks again, once");
    let second = asked(&cmds);
    assert_ne!(second, ticket, "every request has its own ticket");

    // One in flight: however stale and however late, nothing else is asked.
    graph::mark_stale(&mut model);
    assert!(
        graph::tick(&mut model, t0 + Duration::from_secs(600)).is_empty(),
        "only one request in flight at a time"
    );

    // A fast answer: the floor is REFRESH_MIN (1 s).
    assert_eq!(REFRESH_MIN, Duration::from_secs(1));
    graph::apply_reply(&mut model, second, Ok(lanes(&[1, 2])));
    let t1 = t0 + Duration::from_secs(10);
    {
        let s = state(&mut model);
        s.last_request = Some(t1);
        s.last_duration = Duration::from_millis(100);
    }
    graph::mark_stale(&mut model);
    assert!(
        graph::tick(&mut model, t1 + REFRESH_MIN - Duration::from_millis(1)).is_empty(),
        "2 x 100 ms is under the floor: wait 1 s, not 200 ms"
    );
    let cmds = graph::tick(&mut model, t1 + REFRESH_MIN);
    assert_eq!(cmds.len(), 1, "after the floor the view asks again");

    // Answered and not stale: nothing is asked however long it has been.
    let third = asked(&cmds);
    graph::apply_reply(&mut model, third, Ok(lanes(&[1, 2, 3])));
    assert!(!state(&mut model).stale);
    assert!(
        graph::tick(&mut model, t1 + Duration::from_secs(3600)).is_empty(),
        "a view that is not stale does not ask again"
    );
}

fn snapshot(seq: u64, base: &str, branch: &str, ahead: u64, extra: bool) -> Msg {
    let wt = |path: &str, main: bool, branch: &str, ahead: u64| WorktreeView {
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
        path: Untrusted::new(path),
        main,
        admin_name: None,
        status: WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new(branch),
            },
            counts: ChangeCounts::default(),
            changes: Vec::new(),
            divergence: DivergenceView::Counted {
                ahead: CommitCountView {
                    count: ahead,
                    exact: true,
                },
                behind: CommitCountView {
                    count: 0,
                    exact: true,
                },
            },
        },
    };
    let mut worktrees = vec![
        wt("/w/shop", true, "main", 0),
        wt("/w/a", false, branch, ahead),
    ];
    if extra {
        worktrees.push(wt("/w/b", false, "feat-b", 1));
    }
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: "run".into(),
            scope_seq: seq,
            repo: ApiRepo {
                fetched_utc_ms: None,
                repo_id: "r1".into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/shop/.git"),
                base: BaseBranchView {
                    name: Some(UntrustedName::new(base)),
                    status: BaseStatusView::Confirmed,
                },
                worktrees,
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
                prediction: None,
            },
        },
    ))))
}

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn commit_event(scope_seq: u64) -> Msg {
    use gitraptor_api::messages::{
        DeclaredAuthorship, GitEventDetails, GitEventKind, GitEventView, GitIdentity,
    };
    let who = || GitIdentity {
        name: Untrusted::new("Ana"),
        email: Untrusted::new("ana@example.com"),
    };
    let view = GitEventView {
        repo_id: "r1".into(),
        seq: 50 + scope_seq as i64,
        worktree: Untrusted::new("/w/a"),
        kind: GitEventKind::Commit,
        actor: gitraptor_api::Actor::Unattributed,
        observed_utc_ms: 0,
        utc_offset_s: 0,
        details: GitEventDetails::default(),
        gap_id: None,
        inferred: None,
        authorship: Some(DeclaredAuthorship {
            author: who(),
            committer: who(),
            coauthors: Vec::new(),
        }),
    };
    engine(EngineMsg::Event {
        scope: Scope::Repo {
            repo_id: "r1".into(),
        },
        scope_seq,
        event: Box::new(Event {
            seq: 200 + scope_seq,
            kind: GIT_EVENT.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: serde_json::to_value(view).unwrap(),
        }),
    })
}

/// The view open and answered, on the repo of [`snapshot`] (`main`, `feat-a` 3 ahead).
fn open_and_answered() -> Model {
    let mut model = ready_model();
    model.engine.repo = None;
    update(&mut model, snapshot(2, "main", "feat-a", 3, false));
    model.engine.graph = true;
    let ticket = asked(&graph::open(&mut model));
    graph::apply_reply(&mut model, ticket, Ok(lanes(&[3, 2, 1])));
    state(&mut model).stale = false;
    model
}

#[test]
fn a_snapshot_without_changes_does_not_mark_stale() {
    let mut model = open_and_answered();

    // The same branch, divergence, base and worktrees under a new sequence: nothing to redo.
    update(&mut model, snapshot(3, "main", "feat-a", 3, false));
    assert!(
        !state(&mut model).stale,
        "a snapshot with no change of branch, divergence, base or worktrees is not a reason to ask"
    );
    update(&mut model, snapshot(4, "main", "feat-a", 3, false));
    assert!(!state(&mut model).stale);

    // Each of the four changes marks the view stale.
    for (label, msg) in [
        ("divergence", snapshot(5, "main", "feat-a", 4, false)),
        ("branch", snapshot(6, "main", "feat-z", 4, false)),
        ("base", snapshot(7, "develop", "feat-z", 4, false)),
        ("worktree set", snapshot(8, "develop", "feat-z", 4, true)),
    ] {
        update(&mut model, msg);
        assert!(
            state(&mut model).stale,
            "a change of {label} must mark the open view stale"
        );
        state(&mut model).stale = false;
    }

    // A git.event of the repo does (an --amend leaves no other trace, D9).
    update(&mut model, commit_event(9));
    assert!(
        state(&mut model).stale,
        "a git.event of the repo must mark the open view stale"
    );
    state(&mut model).stale = false;

    // A closed view has nothing to mark.
    let _ = graph::close(&mut model);
    update(&mut model, snapshot(10, "main", "feat-a", 9, false));
    assert!(model.ui.graph.is_none());
}

#[test]
fn anchor_survives_new_commits_on_top() {
    let mut model = open_and_answered();
    assert!(
        matches!(state(&mut model).anchor, Anchor::Top),
        "an answered view starts at the top"
    );
    graph::scroll(&mut model, true);
    graph::scroll(&mut model, true);
    let Anchor::Commit { oid: anchored, .. } = state(&mut model).anchor.clone() else {
        panic!("scrolling past the header anchors the view to the first visible commit (D9)");
    };
    assert!(
        [oid(3), oid(2), oid(1)].contains(&anchored),
        "the anchor is the oid of a listed commit, not a line number"
    );

    // Two commits land on top of the lane: the same commit stays at the top of the window.
    let ticket = {
        state(&mut model).stale = true;
        asked(&graph::tick(
            &mut model,
            Instant::now() + Duration::from_secs(3600),
        ))
    };
    graph::apply_reply(&mut model, ticket, Ok(lanes(&[5, 4, 3, 2, 1])));
    match &state(&mut model).anchor {
        Anchor::Commit { oid: now, .. } => assert_eq!(
            *now, anchored,
            "new commits on top must not move what the developer is reading"
        ),
        other => panic!("the anchor was lost: {other:?}"),
    }

    // The anchored commit vanishes (a rebase): the view goes to the head of its lane.
    graph::scroll(&mut model, false);
    graph::scroll(&mut model, true);
    let Anchor::Commit { oid: anchored, .. } = state(&mut model).anchor.clone() else {
        panic!("still anchored to a commit");
    };
    let remaining: Vec<u64> = (1..=5).filter(|n| oid(*n) != anchored).collect();
    state(&mut model).stale = true;
    let ticket = asked(&graph::tick(
        &mut model,
        Instant::now() + Duration::from_secs(7200),
    ));
    graph::apply_reply(&mut model, ticket, Ok(lanes(&remaining)));
    assert!(
        matches!(state(&mut model).anchor, Anchor::Lane(_)),
        "an anchor whose commit disappeared falls back to the header of its lane"
    );
}

#[test]
fn a_graph_reply_for_an_old_ticket_is_dropped() {
    let mut model = ready_model();
    let first = asked(&graph::open(&mut model));
    assert_eq!(
        graph::close(&mut model),
        vec![Cmd::CancelQuery { ticket: first }]
    );
    let second = asked(&graph::open(&mut model));
    assert_ne!(first, second, "a ticket is never reused");

    graph::apply_reply(&mut model, first, Ok(lanes(&[1])));
    let s = state(&mut model);
    assert_eq!(
        s.in_flight,
        Some(second),
        "the answer of an old ticket does not free the current request"
    );
    assert!(s.shown.is_none(), "and shows nothing");

    graph::apply_reply(&mut model, second, Ok(lanes(&[1])));
    let s = state(&mut model);
    assert_eq!(s.in_flight, None);
    assert!(s.shown.is_some(), "the current ticket's answer is shown");

    // A failure for an old ticket is dropped too; the current one keeps its result.
    graph::apply_reply(&mut model, first, Err(GraphFailure::Lost));
    let s = state(&mut model);
    assert!(s.failure.is_none() && s.shown.is_some());
}

#[test]
fn closing_the_graph_while_loading_cancels() {
    let mut model = ready_model();
    let ticket = asked(&press(&mut model, KeyCode::Char('g')));
    assert!(model.ui.graph.is_some(), "g opens the view");
    assert_eq!(
        press(&mut model, KeyCode::Esc),
        vec![Cmd::CancelQuery { ticket }],
        "Esc with the request in flight cancels it"
    );
    assert!(model.ui.graph.is_none());
    // The cancelled answer arrives after: nothing to show, nothing to reopen.
    graph::apply_reply(&mut model, ticket, Err(GraphFailure::Cancelled));
    assert!(model.ui.graph.is_none());

    // `g` closes it too, and a view with its answer closes with nothing to cancel.
    let ticket = asked(&press(&mut model, KeyCode::Char('g')));
    assert_eq!(
        press(&mut model, KeyCode::Char('g')),
        vec![Cmd::CancelQuery { ticket }]
    );
    let ticket = asked(&press(&mut model, KeyCode::Char('g')));
    graph::apply_reply(&mut model, ticket, Ok(lanes(&[1])));
    assert!(press(&mut model, KeyCode::Esc).is_empty());
    assert!(model.ui.graph.is_none());
}

#[test]
fn g_is_inert_without_a_repo_the_capability_or_with_the_diff_open() {
    // No repo on screen: nothing happens.
    let mut model = Model::new(
        crate::present::i18n::Lang::En,
        crate::model::Size {
            width: 80,
            height: 24,
        },
    );
    model.engine.graph = true;
    assert!(press(&mut model, KeyCode::Char('g')).is_empty());
    assert!(model.ui.graph.is_none(), "g without a repo opens nothing");

    // An engine without graph.lanes: the notice and no command (D11).
    let mut model = ready_model();
    model.engine.graph = false;
    assert!(press(&mut model, KeyCode::Char('g')).is_empty());
    assert!(model.ui.graph.is_none());
    assert_eq!(model.ui.notice, Some(Notice::GraphUnsupported));

    // The diff open: g does nothing, and the diff stays (D20).
    let mut model = ready_model();
    let cmds = press(&mut model, KeyCode::Char('d'));
    assert!(
        matches!(cmds.last(), Some(Cmd::Diff { .. })),
        "d opens the diff: {cmds:?}"
    );
    assert!(model.ui.diff.is_some());
    assert!(
        press(&mut model, KeyCode::Char('g')).is_empty(),
        "g with the diff open asks nothing"
    );
    assert!(model.ui.graph.is_none() && model.ui.diff.is_some());

    // And the other way round: with the graph open, d does nothing.
    let mut model = ready_model();
    let _ = asked(&press(&mut model, KeyCode::Char('g')));
    let cmds = press(&mut model, KeyCode::Char('d'));
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::Diff { .. })),
        "d with the graph open asks no diff: {cmds:?}"
    );
    assert!(model.ui.diff.is_none() && model.ui.graph.is_some());
}
