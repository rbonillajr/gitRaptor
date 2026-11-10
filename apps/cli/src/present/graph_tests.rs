//! US-CKP-022 T006 (DS D3, D5, D8, SEC-12): what the panel and the view of the graph say, from
//! what the stream and the answer of `graph.lanes` bring. Mounted by the expert in
//! `present/mod.rs` as `#[cfg(test)] mod graph_tests;`.
//!
//! Exact texts are pinned only in Spanish, which is where the DS and the story fix them
//! ("Rama base develop no encontrada: no calculable"); the English wording is the expert's.

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DivergenceView, HeadView,
    RepoStateView, RepoView as ApiRepo, SessionStateView, SessionView, SessionsListResult,
    WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::graph::panel;
use crate::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use crate::present::i18n::Lang;
use crate::tui::selection::ordered_rows;
use crate::tui::style::Styles;
use crate::tui::update::update;
use crate::tui::view::{Say, shop_model, view};
use crate::tui::widgets::graph_lanes::{Base, LaneOwner};

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn counted(ahead: u64, behind: u64) -> DivergenceView {
    DivergenceView::Counted {
        ahead: CommitCountView {
            count: ahead,
            exact: true,
        },
        behind: CommitCountView {
            count: behind,
            exact: true,
        },
    }
}

fn wt(path: &str, main: bool, branch: &str, divergence: DivergenceView) -> WorktreeView {
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
        status: WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new(branch),
            },
            counts: ChangeCounts::default(),
            changes: Vec::new(),
            divergence,
        },
    }
}

fn session(id: &str, worktree: &str, name: &str, started: i64) -> SessionView {
    SessionView {
        repo_id: "r1".into(),
        session_id: id.into(),
        worktree: Untrusted::new(worktree),
        actor: Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new(name)),
            origin: AgentOrigin::Registered,
        },
        state: SessionStateView::Active,
        started_utc_ms: started,
        state_since_utc_ms: started + 1,
        utc_offset_s: 0,
        ended_utc_ms: None,
        end_cause: None,
    }
}

/// A repo `shop` with the given base and worktrees, and the sessions of the agents.
fn world(
    lang: Lang,
    base: BaseBranchView,
    worktrees: Vec<WorktreeView>,
    sessions: Vec<SessionView>,
) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: 100,
            height: 30,
        },
    );
    model.conn = ConnState::Live;
    update(
        &mut model,
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
            RepoSnapshot {
                run_id: "run".into(),
                scope_seq: 1,
                repo: ApiRepo {
                    fetched_utc_ms: None,
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/shop/.git"),
                    base,
                    worktrees,
                    tier: None,
                    checked_utc_ms: None,
                    kept_temps: None,
                    prediction: None,
                },
            },
        )))),
    );
    update(
        &mut model,
        engine(EngineMsg::Sessions {
            repo_id: "r1".into(),
            result: Box::new(SessionsListResult {
                detection_available: true,
                sessions,
            }),
        }),
    );
    model
}

fn confirmed(name: &str) -> BaseBranchView {
    BaseBranchView {
        name: Some(UntrustedName::new(name)),
        status: BaseStatusView::Confirmed,
    }
}

fn say(model: &Model) -> Say {
    Say::new(model, &Styles::new(model.ui.theme))
}

#[test]
fn panel_follows_fleet_order_and_session_color() {
    // Three linked worktrees and the trunk. `zeta` and `alpha` have an agent (alpha's session
    // started later, so it takes the second color slot); `mid` has none.
    let mut model = world(
        Lang::Es,
        confirmed("main"),
        vec![
            wt("/w/shop", true, "main", counted(0, 0)),
            wt("/w/shop/zeta", false, "feat-zeta", counted(3, 0)),
            wt("/w/shop/mid", false, "feat-mid", counted(2, 1)),
            wt("/w/shop/alpha", false, "feat-alpha", counted(1, 4)),
        ],
        vec![
            session("s-zeta", "/w/shop/zeta", "claude-1", 10),
            session("s-alpha", "/w/shop/alpha", "claude-2", 20),
        ],
    );
    model.engine.graph = true;
    let g = panel(&model, say(&model)).expect("a repo with lanes has a panel");

    assert!(
        matches!(&g.base, Base::Found(name) if name.as_str() == "main"),
        "a confirmed base is found by its name: {:?}",
        g.base
    );
    // One lane per worktree but the trunk, in the order of the fleet (D3).
    let expected: Vec<(u32, u32)> = ordered_rows(&model)
        .iter()
        .filter(|w| !w.main)
        .map(|w| match &w.state {
            crate::model::WorktreeState::Ready {
                divergence: DivergenceView::Counted { ahead, behind },
                ..
            } => (ahead.count as u32, behind.count as u32),
            other => panic!("unexpected state {other:?}"),
        })
        .collect();
    let got: Vec<(u32, u32)> = g.lanes.iter().map(|l| (l.ahead, l.behind)).collect();
    assert_eq!(got.len(), 3, "the trunk is not a lane");
    assert_eq!(
        got, expected,
        "the lanes follow the order of the fleet rows"
    );
    assert!(
        g.lanes.iter().all(|l| !l.conflict),
        "conflict stays false (N5)"
    );

    // The color of a lane is the one of the first agent of its worktree (D8); a worktree with
    // no session is an unattributed lane that names its folder.
    for (lane, (ahead, _)) in g.lanes.iter().zip(&expected) {
        match (ahead, &lane.owner) {
            (3, LaneOwner::Agent { color_index, name }) => {
                assert_eq!(Some(*color_index), model.ui.colors.index("s-zeta"));
                assert!(name.as_str().contains("claude-1"), "{name:?}");
            }
            (1, LaneOwner::Agent { color_index, name }) => {
                assert_eq!(Some(*color_index), model.ui.colors.index("s-alpha"));
                assert!(name.as_str().contains("claude-2"), "{name:?}");
            }
            (2, LaneOwner::Unattributed(label)) => {
                assert!(label.as_str().contains("mid"), "{label:?}");
                assert!(
                    label.as_str().contains("sin agente"),
                    "an unattributed lane reads 'carpeta · sin agente': {label:?}"
                );
            }
            (n, owner) => panic!("the lane with ↑{n} has the wrong owner: {owner:?}"),
        }
    }
    let colors: Vec<usize> = g
        .lanes
        .iter()
        .filter_map(|l| match &l.owner {
            LaneOwner::Agent { color_index, .. } => Some(*color_index),
            LaneOwner::Unattributed(_) => None,
        })
        .collect();
    assert_eq!(colors.len(), 2);
    assert_ne!(colors[0], colors[1], "two agents never share a color slot");
}

#[test]
fn base_not_computable_messages_are_exact() {
    let three = |divergence: &dyn Fn() -> DivergenceView| {
        vec![
            wt("/w/shop", true, "main", divergence()),
            wt("/w/shop/a", false, "feat-a", divergence()),
        ]
    };
    let missing = |model: &Model| match panel(model, say(model)) {
        Some(g) => match g.base {
            Base::Missing(text) => {
                assert!(
                    g.lanes.is_empty(),
                    "no lanes hang from a base that cannot be computed"
                );
                text.as_str().to_owned()
            }
            Base::Found(name) => panic!("the base was found ({name:?}) and must not be"),
        },
        None => panic!("a repo with a base problem must still say so in the panel"),
    };

    // The branch does not exist: DivergenceView::BaseMissing in every worktree.
    let model = world(
        Lang::Es,
        confirmed("develop"),
        three(&|| DivergenceView::BaseMissing),
        Vec::new(),
    );
    assert_eq!(
        missing(&model),
        "Rama base develop no encontrada: no calculable"
    );

    // Resolved but not confirmed: no lanes, even though the stream counted against it (D5).
    let model = world(
        Lang::Es,
        BaseBranchView {
            name: Some(UntrustedName::new("main")),
            status: BaseStatusView::Unconfirmed,
        },
        three(&|| counted(2, 0)),
        Vec::new(),
    );
    let text = missing(&model);
    assert!(
        text.contains("Base main sin confirmar: confirma la rama base para ver los carriles"),
        "{text}"
    );
    assert!(
        text.contains("raptor guard confirm"),
        "the message says how to confirm it: {text}"
    );

    // No base at all (an invalid team declaration).
    let model = world(
        Lang::Es,
        BaseBranchView {
            name: None,
            status: BaseStatusView::Invalid,
        },
        three(&|| DivergenceView::NoBase),
        Vec::new(),
    );
    assert_eq!(missing(&model), "Sin rama base: no calculable");
}

/// Everything a terminal would interpret: escapes, bells, bidi overrides and line separators.
fn dangerous(c: char) -> bool {
    matches!(
        c,
        '\u{1b}' | '\u{7}' | '\u{9b}' | '\u{202e}' | '\u{2066}'
            ..='\u{2069}' | '\u{2028}' | '\u{2029}'
    )
}

#[test]
fn summaries_with_controls_are_sanitized() {
    let mut model = shop_model(Lang::En);
    model.engine.graph = true;
    let cmds = crate::tui::graph::open(&mut model);
    let ticket = match cmds.last() {
        Some(crate::model::Cmd::Graph { ticket, .. }) => *ticket,
        other => panic!("g must ask the graph: {other:?}"),
    };
    let evil_branch = "feat\u{1b}]52;c;ZXZpbA==\u{7}-x";
    let evil_path = "/w/shop/ev\u{202e}il\u{2028}dir";
    let evil_subject = "fix \u{2028}\u{202e}txt \u{1b}]52;c;AAAA\u{7} \u{1b}]0;title\u{7}end";
    let lanes = serde_json::from_value(serde_json::json!({
        "computed_at_utc_ms": 1_793_091_600_000_i64,
        "base": {"ready": {"name": {"untrusted": "main"}}},
        "lanes_total": 1,
        "lanes": [{
            "worktree": {"untrusted": evil_path},
            "head": {"branch": {"name": {"untrusted": evil_branch}}},
            "state": {"ready": {
                "merge_base": "a".repeat(40),
                "total": {"count": 1, "exact": true},
                "commits": [{
                    "oid": "b".repeat(40),
                    "summary": {"untrusted": evil_subject},
                    "committed_utc_ms": 1_793_091_500_000_i64,
                    "merge": false
                }]
            }}
        }]
    }))
    .unwrap();
    crate::tui::graph::apply_reply(&mut model, ticket, Ok(Box::new(lanes)));
    assert!(model.ui.graph.is_some(), "the answer leaves the view open");

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| view(&model, f)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        screen.contains("fix") && screen.contains("end"),
        "the commit was painted (the visible parts of its subject): {screen}"
    );
    for c in screen.chars() {
        assert!(
            !dangerous(c),
            "a control character reached the screen: {:?}",
            c
        );
    }
}
