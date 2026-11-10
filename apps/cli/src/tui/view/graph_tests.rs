//! US-CKP-022 T006 (DS D1, D10, D20, G2): the graph on screen. The panel is a region of the
//! fleet that yields first; the view is the whole body. Mounted by the expert in `view.rs` as
//! `#[cfg(test)] mod graph_tests;` (next to `a11y_tests`). Snapshots live in
//! `tui/snapshots/graph_{panel_100x30,view_80x24}_{en,es,ascii}.snap`; the expert accepts them
//! after reviewing the first render.

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, WorktreeView,
};
use gitraptor_api::prediction::{
    LimitCode, PairLevel, PairSideView, PairStateView, PairSummary, PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::layout::Rect;

use super::Say;
use super::tests::{engine, lines, model_with, ready, render, screen, shop_model, worktree};
use crate::model::{ConnEvent, EngineMsg, Model, Msg};
use crate::present::i18n::Lang;
use crate::tui::graph;
use crate::tui::style::Styles;
use crate::tui::update::update;
use crate::tui::widgets::layout::{self, BodyPlan};

const MAIN: &str = "/w/shop";
const A: &str = "/w/shop/feat-a";
const B: &str = "/w/shop/feat-b";
const C: &str = "/w/shop/feat-c";

fn say(model: &Model) -> Say {
    Say::new(model, &Styles::new(model.ui.theme))
}

fn worktrees() -> Vec<WorktreeView> {
    vec![
        worktree(MAIN, true, ready("main", 0, 0, 0)),
        worktree(A, false, ready("feat-a", 3, 5, 0)),
        worktree(B, false, ready("feat-b", 2, 2, 1)),
        worktree(C, false, ready("feat-c", 1, 7, 0)),
    ]
}

fn side(root: Option<&str>) -> PairSideView {
    match root {
        Some(root) => PairSideView::Worktree {
            root: Untrusted::new(root),
        },
        None => PairSideView::Base {},
    }
}

fn conflict(id: &str, a: Option<&str>, b: Option<&str>) -> PairSummary {
    PairSummary {
        pair_id: id.into(),
        rev: 2,
        a: side(a),
        b: side(b),
        state: PairStateView::Current,
        reason: None,
        level: Some(PairLevel::Conflict),
        computed_at_utc_ms: Some(1),
        overlap_files: 1,
        overlap_partial: false,
        conflict_files: 1,
        conflict_truncated: false,
    }
}

/// The shop of three agent branches, a confirmed `main` and, with `conflicts`, six predicted
/// conflicts (every pair and every branch against the base): a conflict panel as tall as the
/// layout lets it be.
fn world(lang: Lang, conflicts: bool) -> Model {
    let mut model = model_with(lang, worktrees(), None);
    let pairs = if conflicts {
        vec![
            conflict("c1", Some(A), Some(B)),
            conflict("c2", Some(A), Some(C)),
            conflict("c3", Some(B), Some(C)),
            conflict("c4", Some(A), None),
            conflict("c5", Some(B), None),
            conflict("c6", Some(C), None),
        ]
    } else {
        Vec::new()
    };
    update(
        &mut model,
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
            RepoSnapshot {
                run_id: "run".into(),
                scope_seq: 2,
                repo: ApiRepo {
                    fetched_utc_ms: None,
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/shop/.git"),
                    base: BaseBranchView {
                        name: Some(UntrustedName::new("main")),
                        status: BaseStatusView::Confirmed,
                    },
                    worktrees: worktrees(),
                    tier: None,
                    checked_utc_ms: None,
                    kept_temps: None,
                    prediction: conflicts.then_some(PredictionSummary {
                        limits: LimitCode::ALL.to_vec(),
                        pairs,
                        pairs_truncated: false,
                    }),
                },
            },
        )))),
    );
    update(&mut model, Msg::Conn(ConnEvent::Prediction(conflicts)));
    model
}

#[test]
fn the_graph_yields_before_the_conflict_panel_at_80x24() {
    let area = Rect::new(0, 0, 80, 24);

    // With conflicts: the conflict panel is there, and the graph panel is not.
    let with = world(Lang::En, true);
    let panel = super::conflict_panel::build(&with, say(&with), layout::ALERTS_MAX)
        .expect("six predicted conflicts make a conflict panel");
    let alerts = panel.height(area.width);
    let graph_wanted = crate::present::graph::panel(&with, say(&with))
        .expect("a repo with agent branches over a confirmed base has a graph panel");
    let title = graph_wanted.title.as_str().to_owned();
    assert!(!title.trim().is_empty(), "the graph panel has a title");

    let regions = layout::layout(
        area,
        BodyPlan {
            alerts,
            graph: true,
            detail: false,
        },
    )
    .unwrap();
    assert!(
        regions.graph.is_none() && regions.alerts.is_some(),
        "the layout gives the alerts ({alerts} rows) what the graph would take"
    );
    assert!(
        regions.list.height as usize >= 10,
        "the list keeps LIST_MIN rows: {:?}",
        regions.list
    );

    let shown = screen(&with, 80, 24);
    assert!(
        !shown.contains(&title),
        "the graph panel (\"{title}\") is painted at 80x24 next to the conflict panel:\n{shown}"
    );
    for branch in ["feat-a", "feat-b", "feat-c", "main"] {
        assert!(shown.contains(branch), "the fleet lost {branch}:\n{shown}");
    }

    // Without conflicts the graph panel has room at the same size: it yields only when needed.
    let without = world(Lang::En, false);
    let title = crate::present::graph::panel(&without, say(&without))
        .expect("a graph panel")
        .title
        .as_str()
        .to_owned();
    let shown = screen(&without, 80, 24);
    assert!(
        shown.contains(&title),
        "with no alerts the graph panel (\"{title}\") has its region at 80x24:\n{shown}"
    );
}

/// The shop with the graph view open on an answer: a long lane with a merge, a lane without
/// merge-base and a lane of the fleet with no commits to show.
fn open_view(lang: Lang, theme: Option<Theme>) -> Model {
    let mut model = shop_model(lang);
    if let Some(theme) = theme {
        model = model.with_theme(theme);
    }
    model.engine.graph = true;
    let ticket = match graph::open(&mut model).last() {
        Some(crate::model::Cmd::Graph { ticket, .. }) => *ticket,
        other => panic!("g must ask the graph: {other:?}"),
    };
    let commit = |n: u64, summary: &str, merge: bool| {
        serde_json::json!({
            "oid": format!("{:040x}", 0xa1b2c3d0_u64 + n),
            "summary": {"untrusted": summary},
            "committed_utc_ms": 1_793_091_500_000_i64 - (n as i64) * 3_600_000,
            "merge": merge
        })
    };
    let answer = serde_json::from_value(serde_json::json!({
        "computed_at_utc_ms": 1_793_091_792_000_i64,
        "base": {"ready": {"name": {"untrusted": "main"}}},
        "lanes_total": 3,
        "lanes": [
            {"worktree": {"untrusted": "/w/shop/feat-docs"},
             "head": {"branch": {"name": {"untrusted": "feat-docs"}}},
             "state": {"ready": {
                 "merge_base": format!("{:040x}", 0xa1b2c3d_u64),
                 "total": {"count": 1, "exact": true},
                 "commits": [commit(9, "Rewrite the README", false)]}}},
            {"worktree": {"untrusted": "/w/shop/feat-login"},
             "head": {"branch": {"name": {"untrusted": "feat-login"}}},
             "state": {"not-computable": {"reason": "no-merge-base"}}},
            {"worktree": {"untrusted": "/w/shop/feat-pagos"},
             "head": {"branch": {"name": {"untrusted": "feat-pagos"}}},
             "state": {"ready": {
                 "merge_base": format!("{:040x}", 0xa1b2c3d_u64),
                 "total": {"count": 54, "exact": true},
                 "commits": [
                     commit(1, "Add payment retry", false),
                     commit(2, "Merge branch 'main' into feat-pagos", true),
                     commit(3, "Fix the rounding of totals", false),
                     commit(4, "Add the pay endpoint", false)]}}}
        ]
    }))
    .unwrap();
    graph::apply_reply(&mut model, ticket, Ok(Box::new(answer)));
    assert!(model.ui.graph.is_some());
    model
}

fn ascii(lang: Lang) -> Model {
    open_view(
        lang,
        Some(Theme::new(
            ColorMode::NoColor,
            Contrast::Normal,
            SymbolSet::Ascii,
        )),
    )
}

fn panel_model(lang: Lang, theme: Option<Theme>) -> Model {
    let model = shop_model(lang);
    match theme {
        Some(theme) => model.with_theme(theme),
        None => model,
    }
}

#[test]
fn graph_snapshots_panel_100x30_and_view_80x24() {
    let no_color = || {
        Some(Theme::new(
            ColorMode::NoColor,
            Contrast::Normal,
            SymbolSet::Ascii,
        ))
    };
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| {
        for (name, panel, view) in [
            ("en", panel_model(Lang::En, None), open_view(Lang::En, None)),
            ("es", panel_model(Lang::Es, None), open_view(Lang::Es, None)),
            ("ascii", panel_model(Lang::En, no_color()), ascii(Lang::En)),
        ] {
            let panel_screen = lines(&render(&panel, 100, 30)).join("\n");
            let view_screen = lines(&render(&view, 80, 24)).join("\n");
            if name == "ascii" {
                assert!(panel_screen.is_ascii(), "{panel_screen}");
                assert!(view_screen.is_ascii(), "{view_screen}");
            }
            insta::assert_snapshot!(format!("graph_panel_100x30_{name}"), panel_screen);
            insta::assert_snapshot!(format!("graph_view_80x24_{name}"), view_screen);
        }
    });
}
