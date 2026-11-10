//! The panel and the full-screen view list the lanes in the same order: the one of the fleet,
//! not the one the engine answered in (DS-US-CKP-022 D3).

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, DivergenceView, HeadView, RepoStateView,
    RepoView as ApiRepo, SessionStateView, SessionView, SessionsListResult, WorktreeStatus,
    WorktreeView,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};

use super::graph::{GraphRow, ingest, panel, rows};
use crate::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use crate::present::i18n::Lang;
use crate::tui::style::Styles;
use crate::tui::update::update;
use crate::tui::view::Say;
use crate::tui::widgets::graph_lanes::LaneOwner;

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn wt(path: &str, main: bool, branch: &str) -> WorktreeView {
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
            divergence: DivergenceView::NoBase,
        },
    }
}

fn model() -> Model {
    let mut model = Model::new(
        Lang::En,
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
                    base: BaseBranchView {
                        name: Some(UntrustedName::new("main")),
                        status: BaseStatusView::Confirmed,
                    },
                    worktrees: vec![
                        wt("/w/shop", true, "main"),
                        wt("/w/shop/a-none", false, "a-none"),
                        wt("/w/shop/d-act", false, "d-act"),
                    ],
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
                sessions: vec![SessionView {
                    repo_id: "r1".into(),
                    session_id: "s1".into(),
                    worktree: Untrusted::new("/w/shop/d-act"),
                    actor: Actor::Agent {
                        kind: AgentKind::Other,
                        name: Some(UntrustedName::new("claude-1")),
                        origin: AgentOrigin::Registered,
                    },
                    state: SessionStateView::Active,
                    started_utc_ms: 10,
                    state_since_utc_ms: 11,
                    utc_offset_s: 0,
                    ended_utc_ms: None,
                    end_cause: None,
                }],
            }),
        }),
    );
    model
}

fn answer() -> gitraptor_api::graph::GraphLanes {
    let lane = |path: &str, branch: &str| {
        serde_json::json!({
            "worktree": {"untrusted": path},
            "head": {"branch": {"name": {"untrusted": branch}}},
            "state": {"not-computable": {"reason": "no-commits"}}
        })
    };
    serde_json::from_value(serde_json::json!({
        "computed_at_utc_ms": 1,
        "base": {"ready": {"name": {"untrusted": "main"}}},
        "lanes_total": 2,
        // The engine's order: the alphabetical one.
        "lanes": [lane("/w/shop/a-none", "a-none"), lane("/w/shop/d-act", "d-act")]
    }))
    .unwrap()
}

#[test]
fn the_view_lists_the_lanes_in_the_order_of_the_panel() {
    let model = model();
    let say = Say::new(&model, &Styles::new(model.ui.theme));
    let from_panel: Vec<bool> = panel(&model, say)
        .expect("a panel")
        .lanes
        .iter()
        .map(|l| matches!(l.owner, LaneOwner::Agent { .. }))
        .collect();
    assert_eq!(
        from_panel,
        [true, false],
        "the lane with an agent leads the fleet"
    );

    let shown = ingest(&answer());
    let from_view: Vec<bool> = rows(&shown, &model, say)
        .iter()
        .filter_map(|r| match r {
            GraphRow::Header { color, .. } => Some(color.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(
        from_view, from_panel,
        "the view follows the order of the fleet, not the engine's"
    );
}

/// At 120 columns the graph panel sits beside the fleet (55 % of the body for the fleet), the
/// fleet stays readable and the key bar names `g`.
#[test]
fn at_120_columns_the_panel_sits_beside_a_readable_fleet() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let model = model();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| crate::tui::view::view(&model, f))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen: Vec<String> = buffer
        .content()
        .chunks(120)
        .map(|row| row.iter().map(|c| c.symbol()).collect())
        .collect();
    let all = screen.join("\n");

    let title = screen
        .iter()
        .find(|l| l.contains("Fleet") && l.contains("Graph"))
        .unwrap_or_else(|| panic!("the fleet and the graph panel share their top row:\n{all}"));
    assert!(
        title.find("Fleet").unwrap() < title.find("Graph").unwrap(),
        "the graph is to the right of the fleet:\n{all}"
    );
    // The fleet's agent and folder fit whole in its 66 columns.
    let row = screen
        .iter()
        .find(|l| l.contains("d-act") && l.contains("claude-1"))
        .unwrap_or_else(|| panic!("the agent row of d-act:\n{all}"));
    assert!(row.contains("claude-1 · d-act"), "{row}");
    // The panel names the lanes of the fleet, the agent's first.
    assert!(
        screen
            .iter()
            .any(|l| l.contains("│") && l.contains("claude-1 · d-act")),
        "the panel draws the lane of d-act:\n{all}"
    );
    let bar = screen.last().unwrap();
    assert!(bar.contains("g graph"), "the key bar names `g`: {bar}");
}
