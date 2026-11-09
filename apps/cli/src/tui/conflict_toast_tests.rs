//! The toast of a new predicted conflict: the snapshots of the whole screen (en, es, and ASCII
//! without color) and the plural of " and N more".

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
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use crate::present::i18n::Lang;
use crate::tui::gallery::buffer_text;
use crate::tui::update::update;
use crate::tui::view::view;

const T0: i64 = 1_000_000_000;

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

fn conflict(files: u32) -> PairSummary {
    let side = |i| PairSideView::Worktree {
        root: Untrusted::new(root(i)),
    };
    PairSummary {
        pair_id: "p12".into(),
        rev: 2,
        a: side(1),
        b: side(2),
        state: PairStateView::Current,
        reason: None,
        level: Some(PairLevel::Conflict),
        computed_at_utc_ms: Some(T0),
        overlap_files: 0,
        overlap_partial: false,
        conflict_files: files,
        conflict_truncated: false,
    }
}

/// An opened TUI that then receives a new conflict of `files` files and its detail.
fn alerted(lang: Lang, symbols: SymbolSet, files: u32) -> Model {
    let mut m = Model::new(
        lang,
        Size {
            width: 80,
            height: 24,
        },
    );
    m.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, symbols);
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
            worktrees: (0..4).map(wt).collect(),
            tier: None,
            checked_utc_ms: None,
            kept_temps: None,
            prediction: Some(PredictionSummary {
                limits: LimitCode::ALL.to_vec(),
                pairs: vec![],
                pairs_truncated: false,
            }),
        },
    });
    update(&mut m, engine(EngineMsg::Snapshot(Box::new(snapshot))));
    update(&mut m, Msg::Tick { now_ms: T0 });
    let pair = conflict(files);
    let event = Event {
        seq: 2,
        kind: PREDICTION_CHANGED.into(),
        version: 1,
        wall_ms: 0,
        timings: None,
        data: serde_json::to_value(PredictionChanged {
            repo_id: "r1".into(),
            pairs: vec![pair.clone()],
            removed: vec![],
        })
        .expect("event data"),
    };
    update(
        &mut m,
        engine(EngineMsg::Event {
            scope: Scope::Repo {
                repo_id: "r1".into(),
            },
            scope_seq: 2,
            event: Box::new(event),
        }),
    );
    let file = |path: &str| ConflictFileView {
        path: Untrusted::new(path),
        kind: ConflictKindView::Content,
        ours: vec![LineRange { start: 12, len: 7 }],
        theirs: vec![LineRange { start: 14, len: 7 }],
        hunks_truncated: false,
        has_merge_rule: false,
    };
    let paths = ["src/api.rs", "b.rs", "c.rs", "d.rs"];
    let page = PairDetail {
        summary: pair,
        overlap: vec![],
        conflict: Some(ConflictView {
            merge_base: MergeBaseKind::Single,
            files: paths.iter().take(files as usize).map(|p| file(p)).collect(),
            truncated: false,
            rewrites_escalated: false,
        }),
    };
    update(
        &mut m,
        engine(EngineMsg::PredictionPage {
            repo_id: "r1".into(),
            page: Box::new(PredictionPage {
                limits: LimitCode::ALL.to_vec(),
                pairs: vec![page],
                next_cursor: None,
            }),
        }),
    );
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

#[test]
fn conflict_toast_scenario_1_text_en_es_ascii() {
    snap(
        "conflict_toast_80x24_en",
        &screen(&alerted(Lang::En, SymbolSet::Unicode, 1)),
    );
    snap(
        "conflict_toast_80x24_es",
        &screen(&alerted(Lang::Es, SymbolSet::Unicode, 1)),
    );
    let ascii = screen(&alerted(Lang::En, SymbolSet::Ascii, 1));
    assert!(ascii.is_ascii(), "non-ASCII in ASCII mode:\n{ascii}");
    snap("conflict_toast_80x24_ascii", &ascii);
}

#[test]
fn conflict_toast_more_files_singular_and_plural() {
    for (files, en, es) in [
        (2, "in src/api.rs and 1 more", "en src/api.rs y 1 más"),
        (4, "in src/api.rs and 3 more", "en src/api.rs y 3 más"),
    ] {
        let s = screen(&alerted(Lang::En, SymbolSet::Unicode, files));
        assert!(s.contains(en), "{en} missing:\n{s}");
        let s = screen(&alerted(Lang::Es, SymbolSet::Unicode, files));
        assert!(s.contains(es), "{es} missing:\n{s}");
    }
}
