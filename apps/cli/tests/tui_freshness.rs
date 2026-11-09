//! US-CKP-007: freshness of the prediction in the conflict panel, driven only through the public
//! model (snapshot, `prediction.changed`, `Msg::Tick`) on `TestBackend`. No daemon, no Git.
//! The English strings are the contract the expert must satisfy in `FreshnessText`.

use gitraptor_api::Untrusted;
use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, UnavailableReason,
    WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::PREDICTION_CHANGED;
use gitraptor_api::prediction::{
    LimitCode, PairLevel, PairReason, PairSideView, PairStateView, PairSummary, PredictionChanged,
    PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, Scope, ScopeSnapshot};
use gitraptor_cli::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const REPO: &str = "r1";
/// Instant the results were computed at.
const T0: i64 = 1_000_000_000;
const CLEAR_ES: &str = "Sin conflictos previstos";

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

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

/// A pair between worktrees `a` and `b` (`None` = the base branch), computed at `T0`.
fn pair(
    id: &str,
    rev: u64,
    a: usize,
    b: Option<usize>,
    state: PairStateView,
    reason: Option<PairReason>,
    level: Option<PairLevel>,
) -> PairSummary {
    PairSummary {
        pair_id: id.into(),
        rev,
        a: PairSideView::Worktree {
            root: Untrusted::new(root(a)),
        },
        b: match b {
            Some(i) => PairSideView::Worktree {
                root: Untrusted::new(root(i)),
            },
            None => PairSideView::Base {},
        },
        state,
        reason,
        level,
        computed_at_utc_ms: level.map(|_| T0),
        overlap_files: u32::from(level == Some(PairLevel::Overlap)),
        overlap_partial: false,
        conflict_files: u32::from(level == Some(PairLevel::Conflict)),
        conflict_truncated: false,
    }
}

fn current(id: &str, a: usize, b: Option<usize>, level: PairLevel) -> PairSummary {
    pair(id, 1, a, b, PairStateView::Current, None, Some(level))
}

fn calculating(id: &str, a: usize, b: Option<usize>) -> PairSummary {
    pair(id, 1, a, b, PairStateView::Calculating, None, None)
}

/// A repo of 10 worktrees (main + claude-1..9) publishing `pairs`.
fn snapshot(pairs: Vec<PairSummary>) -> Msg {
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: "run".into(),
            scope_seq: 1,
            repo: ApiRepo {
                fetched_utc_ms: None,
                repo_id: REPO.into(),
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
        },
    ))))
}

fn changed(seq: u64, pairs: Vec<PairSummary>) -> Msg {
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
                removed: vec![],
            })
            .unwrap(),
        }),
    })
}

fn model(lang: Lang, pairs: Vec<PairSummary>, now_ms: i64) -> Model {
    let mut m = Model::new(
        lang,
        Size {
            width: 120,
            height: 50,
        },
    );
    m.conn = ConnState::Live;
    update(&mut m, snapshot(pairs));
    update(&mut m, Msg::Tick { now_ms });
    m
}

fn tick(m: &mut Model, now_ms: i64) {
    update(m, Msg::Tick { now_ms });
}

fn lines(m: &Model) -> Vec<String> {
    let (w, h) = (m.ui.size.width, m.ui.size.height);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| view(m, f)).unwrap();
    buffer_text(terminal.backend().buffer())
}

fn screen(m: &Model) -> String {
    lines(m).join("\n")
}

/// The row of the pair between worktrees `a` and `b`: the line naming both.
fn row_of(m: &Model, a: &str, b: &str) -> String {
    let all = lines(m);
    all.iter()
        .find(|l| l.contains(a) && l.contains(b))
        .unwrap_or_else(|| panic!("no row names {a} and {b}:\n{}", all.join("\n")))
        .clone()
}

#[test]
fn s1_initial_calculation_says_calculating_and_never_no_conflicts() {
    let pairs: Vec<_> = (0..10)
        .map(|i| calculating(&format!("p{i}"), 1 + i % 9, Some(((1 + i) % 9) + 1)))
        .collect();
    let es = screen(&model(Lang::Es, pairs.clone(), T0));
    assert!(
        es.contains("Calculando 10 de 10 pares"),
        "missing the aggregate calculating line:\n{es}"
    );
    assert!(
        !es.contains("Sin conflictos"),
        "a pair without result says no conflicts:\n{es}"
    );
    // Contract for FreshnessText (en).
    let en = screen(&model(Lang::En, pairs, T0));
    assert!(
        en.contains("Calculating 10 of 10 pairs"),
        "missing the aggregate calculating line (en):\n{en}"
    );
}

#[test]
fn s2_a_commit_marks_only_that_pair_stale_with_its_age() {
    let start = vec![
        current("p12", 1, Some(2), PairLevel::Conflict),
        current("p34", 3, Some(4), PairLevel::Overlap),
    ];
    let mut m = model(Lang::Es, start, T0 + 40_000);
    let stale = pair(
        "p12",
        2,
        1,
        Some(2),
        PairStateView::Recalculating,
        None,
        Some(PairLevel::Clean),
    );
    update(&mut m, changed(2, vec![stale]));
    let row = row_of(&m, "claude-1", "claude-2");
    assert!(
        row.contains("desactualizado · hace 40 s · recalculando"),
        "the recalculating pair has no stale row with its age: {row}"
    );
    assert!(!row.contains('⚡') && !row.contains('⚠'), "{row}");
    let other = row_of(&m, "claude-3", "claude-4");
    assert!(
        other.contains("hace 40 s"),
        "the other pair shows no age: {other}"
    );
    assert!(!other.contains("desactualizado"), "{other}");
    let fresh = pair(
        "p12",
        3,
        1,
        Some(2),
        PairStateView::Current,
        None,
        Some(PairLevel::Conflict),
    );
    let mut fresh = fresh;
    fresh.computed_at_utc_ms = Some(T0 + 40_000);
    update(&mut m, changed(3, vec![fresh]));
    let row = row_of(&m, "claude-1", "claude-2");
    assert!(
        row.contains("hace 0 s") && !row.contains("desactualizado"),
        "{row}"
    );
}

#[test]
fn s3_a_moved_base_shows_the_base_pairs_calculating() {
    let mut start: Vec<_> = (1..=4)
        .map(|i| current(&format!("b{i}"), i, None, PairLevel::Overlap))
        .collect();
    start.extend(
        (0..6).map(|i| current(&format!("c{i}"), 1 + i, Some(7 + i % 3), PairLevel::Clean)),
    );
    let mut m = model(Lang::Es, start, T0 + 5_000);
    let moved: Vec<_> = (1..=4)
        .map(|i| {
            let mut p = calculating(&format!("b{i}"), i, None);
            p.rev = 2;
            p
        })
        .collect();
    update(&mut m, changed(2, moved));
    let s = screen(&m);
    assert!(
        s.contains("Calculando 4 de 10 pares"),
        "missing the base pairs line:\n{s}"
    );
    assert!(
        !s.contains(CLEAR_ES),
        "a calculating pair enables the clear line:\n{s}"
    );
    assert!(
        !s.lines()
            .any(|l| l.contains("claude-1") && l.contains("hace")),
        "a calculating pair keeps its row:\n{s}"
    );
}

#[test]
fn s4_the_age_is_always_shown_and_moves_with_the_tick() {
    let conflict = vec![current("p12", 1, Some(2), PairLevel::Conflict)];
    let m = model(Lang::Es, conflict.clone(), T0 + 180_000);
    let row = row_of(&m, "claude-1", "claude-2");
    assert!(
        row.contains("hace 3 min"),
        "the age of a 3 min result is not shown: {row}"
    );

    let mut m = model(Lang::Es, conflict.clone(), T0 + 59_000);
    assert!(
        row_of(&m, "claude-1", "claude-2").contains("hace 59 s"),
        "{}",
        screen(&m)
    );
    tick(&mut m, T0 + 60_000);
    let row = row_of(&m, "claude-1", "claude-2");
    assert!(
        row.contains("hace 1 min"),
        "the tick does not move the age: {row}"
    );

    let m = model(Lang::Es, conflict, T0 - 5_000);
    let row = row_of(&m, "claude-1", "claude-2");
    assert!(
        row.contains("hace 0 s"),
        "a negative age is not clamped: {row}"
    );
}

#[test]
fn s5_pending_and_not_computable_group_by_reason_and_never_say_no_conflicts() {
    let pend =
        |id: &str, a, reason| pair(id, 1, a, None, PairStateView::Pending, Some(reason), None);
    let pairs = vec![
        pend("a", 1, PairReason::BaseUnconfirmed),
        pend("b", 2, PairReason::BaseUnconfirmed),
        pend("c", 3, PairReason::BaseUnconfirmed),
        pend("d", 4, PairReason::BaseUnconfirmed),
        pair(
            "e",
            1,
            5,
            None,
            PairStateView::NotComputable,
            Some(PairReason::BaseMissing),
            None,
        ),
    ];
    let s = screen(&model(Lang::Es, pairs, T0));
    assert!(
        s.contains("Pendiente: confirma la rama base (4 pares)"),
        "pending group missing:\n{s}"
    );
    assert!(
        s.contains("No calculable: falta la rama base (1 par)"),
        "not-computable group missing:\n{s}"
    );
    assert!(!s.contains("Sin conflictos"), "{s}");
}

#[test]
fn s6_no_conflicts_line_only_when_every_pair_is_current_and_clean() {
    let clean = |id: &str, a, b| current(id, a, Some(b), PairLevel::Clean);
    let all_clean = vec![clean("a", 1, 2), clean("b", 3, 4)];
    let s = screen(&model(Lang::Es, all_clean.clone(), T0 + 180_000));
    assert!(
        s.contains("Sin conflictos previstos · hace 3 min"),
        "the clear line with the age is missing:\n{s}"
    );
    // The age is the oldest result's.
    let mut older = all_clean.clone();
    older[0].computed_at_utc_ms = Some(T0 - 120_000);
    let s = screen(&model(Lang::Es, older, T0 + 180_000));
    assert!(
        s.contains("Sin conflictos previstos · hace 5 min"),
        "not the oldest age:\n{s}"
    );

    let none = screen(&model(Lang::Es, vec![], T0));
    assert!(
        !none.contains(CLEAR_ES),
        "no pairs, yet the clear line shows:\n{none}"
    );

    let with = |p: PairSummary| {
        let mut v = all_clean.clone();
        v.push(p);
        screen(&model(Lang::Es, v, T0))
    };
    let blockers = [
        pair(
            "x",
            1,
            5,
            Some(6),
            PairStateView::Recalculating,
            None,
            Some(PairLevel::Clean),
        ),
        calculating("x", 5, Some(6)),
        pair(
            "x",
            1,
            5,
            Some(6),
            PairStateView::Pending,
            Some(PairReason::BaseUnconfirmed),
            None,
        ),
        current("x", 5, Some(6), PairLevel::Overlap),
        current("x", 5, Some(6), PairLevel::Conflict),
    ];
    for p in blockers {
        let s = with(p.clone());
        assert!(
            !s.contains(CLEAR_ES),
            "the clear line shows with a {:?} pair:\n{s}",
            p.state
        );
    }
    let single = screen(&model(
        Lang::Es,
        vec![pair(
            "x",
            1,
            5,
            Some(6),
            PairStateView::Recalculating,
            None,
            Some(PairLevel::Clean),
        )],
        T0,
    ));
    assert!(!single.contains(CLEAR_ES), "{single}");
}

#[test]
fn s7_freshness_is_legible_without_color_and_in_ascii() {
    for lang in [Lang::Es, Lang::En] {
        let pairs = vec![
            current("p12", 1, Some(2), PairLevel::Conflict),
            calculating("p34", 3, Some(4)),
            pair(
                "p56",
                1,
                5,
                Some(6),
                PairStateView::Recalculating,
                None,
                Some(PairLevel::Clean),
            ),
        ];
        let mut m = model(lang, pairs, T0 + 40_000);
        m.ui.size = Size {
            width: 80,
            height: 24,
        };
        m.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Ascii);
        let s = screen(&m);
        let (age, stale, calc) = match lang {
            Lang::Es => (
                "hace 40 s",
                "desactualizado - hace 40 s - recalculando",
                "Calculando 1 de 3 pares",
            ),
            _ => ("", "", "Calculating 1 of 3 pairs"),
        };
        assert!(s.contains(calc), "missing the calculating line:\n{s}");
        if lang == Lang::Es {
            assert!(s.contains(age), "missing the age:\n{s}");
            assert!(
                s.contains(stale),
                "the middle dot is not folded to '-':\n{s}"
            );
        }
        assert!(
            s.is_ascii(),
            "a non-ASCII character is left in ASCII mode:\n{s}"
        );
    }
}
