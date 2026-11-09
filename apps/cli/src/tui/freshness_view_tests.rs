//! The freshness in the panel: the tick that moves the age, and the snapshots of the whole
//! screen (en, es, and ASCII without color).

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, UnavailableReason,
    WorktreeStatus, WorktreeView,
};
use gitraptor_api::prediction::{
    LimitCode, PairLevel, PairSideView, PairStateView, PairSummary, PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use crate::present::i18n::Lang;
use crate::tui::gallery::buffer_text;
use crate::tui::update::update;
use crate::tui::view::view;

const T0: i64 = 1_000_000_000;

fn root(i: usize) -> String {
    if i == 0 {
        "/w/shop".into()
    } else {
        format!("/w/shop/claude-{i}")
    }
}

fn wt(i: usize) -> WorktreeView {
    WorktreeView {
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
        path: Untrusted::new(root(i)),
        main: i == 0,
        admin_name: None,
        status: WorktreeStatus::Unavailable {
            reason: UnavailableReason::Missing,
        },
    }
}

fn pair(
    id: &str,
    a: usize,
    b: usize,
    state: PairStateView,
    level: Option<PairLevel>,
) -> PairSummary {
    let side = |i| PairSideView::Worktree {
        root: Untrusted::new(root(i)),
    };
    PairSummary {
        pair_id: id.into(),
        rev: 1,
        a: side(a),
        b: side(b),
        state,
        reason: None,
        level,
        computed_at_utc_ms: level.map(|_| T0),
        overlap_files: u32::from(level == Some(PairLevel::Overlap)),
        overlap_partial: false,
        conflict_files: u32::from(level == Some(PairLevel::Conflict)),
        conflict_truncated: false,
    }
}

fn model(lang: Lang, pairs: Vec<PairSummary>, now_ms: i64, size: (u16, u16)) -> Model {
    let mut m = Model::new(
        lang,
        Size {
            width: size.0,
            height: size.1,
        },
    );
    m.conn = ConnState::Live;
    let snapshot = ScopeSnapshot::Repo(RepoSnapshot {
        run_id: "run".into(),
        scope_seq: 1,
        repo: ApiRepo {
            fetched_utc_ms: None,
            repo_id: "r1".into(),
            state: RepoStateView::Observed,
            path: Untrusted::new("/w/shop/.git"),
            base: BaseBranchView {
                name: Some(gitraptor_api::UntrustedName::new("main")),
                status: BaseStatusView::Confirmed,
            },
            worktrees: (0..10).map(wt).collect(),
            tier: None,
            checked_utc_ms: None,
            kept_temps: None,
            prediction: Some(PredictionSummary {
                limits: LimitCode::ALL.to_vec(),
                pairs,
                pairs_truncated: false,
            }),
        },
    });
    update(
        &mut m,
        Msg::Engine(Stamped {
            recv_ns: 0,
            decoded_ns: 0,
            msg: EngineMsg::Snapshot(Box::new(snapshot)),
        }),
    );
    update(&mut m, Msg::Tick { now_ms });
    m
}

fn screen(m: &Model) -> String {
    let mut t =
        Terminal::new(TestBackend::new(m.ui.size.width, m.ui.size.height)).expect("backend");
    t.draw(|f| view(m, f)).expect("draw");
    buffer_text(t.backend().buffer()).join("\n")
}

fn snap(name: &str, text: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("snapshots");
    settings.bind(|| insta::assert_snapshot!(name, text));
}

fn conflict() -> PairSummary {
    pair(
        "p12",
        1,
        2,
        PairStateView::Current,
        Some(PairLevel::Conflict),
    )
}

#[test]
fn freshness_tick_marks_dirty_only_while_a_result_has_a_time() {
    let mut m = model(Lang::En, vec![conflict()], T0, (80, 24));
    m.dirty = false;
    update(&mut m, Msg::Tick { now_ms: T0 + 1_000 });
    assert!(m.dirty, "the age of a result moves with the clock");

    let calculating = pair("c", 1, 2, PairStateView::Calculating, None);
    let mut m = model(Lang::En, vec![calculating], T0, (80, 24));
    m.dirty = false;
    update(&mut m, Msg::Tick { now_ms: T0 + 1_000 });
    assert!(
        !m.dirty,
        "a calculating line does not change with the clock"
    );
}

#[test]
fn freshness_tick_moves_the_age() {
    let mut m = model(Lang::Es, vec![conflict()], T0 + 59_000, (80, 24));
    assert!(screen(&m).contains("hace 59 s"));
    update(
        &mut m,
        Msg::Tick {
            now_ms: T0 + 60_000,
        },
    );
    assert!(screen(&m).contains("hace 1 min"));
}

#[test]
fn freshness_initial_calculation_10_worktrees() {
    let pairs: Vec<_> = (0..10)
        .map(|i| {
            pair(
                &format!("p{i}"),
                1 + i % 9,
                1 + (1 + i) % 9,
                PairStateView::Calculating,
                None,
            )
        })
        .collect();
    for (tag, lang) in [("en", Lang::En), ("es", Lang::Es)] {
        let text = screen(&model(lang, pairs.clone(), T0, (80, 24)));
        assert!(!text.contains("Sin conflictos") && !text.contains("No conflicts"));
        snap(&format!("freshness_initial_calculation_{tag}"), &text);
    }
}

#[test]
fn freshness_age_shows_3_min() {
    for (tag, lang) in [("en", Lang::En), ("es", Lang::Es)] {
        let text = screen(&model(lang, vec![conflict()], T0 + 180_000, (80, 24)));
        snap(&format!("freshness_age_3_min_{tag}"), &text);
    }
}

#[test]
fn freshness_mixed_in_ascii_without_color() {
    let pairs = vec![
        conflict(),
        pair("p34", 3, 4, PairStateView::Calculating, None),
        pair(
            "p56",
            5,
            6,
            PairStateView::Recalculating,
            Some(PairLevel::Clean),
        ),
    ];
    for (tag, lang) in [("en", Lang::En), ("es", Lang::Es)] {
        let mut m = model(lang, pairs.clone(), T0 + 40_000, (80, 24));
        m.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Ascii);
        let text = screen(&m);
        assert!(lang == Lang::Es || text.is_ascii(), "{text}");
        snap(&format!("freshness_mixed_ascii_{tag}"), &text);
    }
}
