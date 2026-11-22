//! US-WTL-005 (T008): the TUI row of a linked worktree shows its origin or
//! "foreign · hands off", in English and Spanish; the mark beats `temporary`
//! and `temporary` beats the origin; the main worktree has no label. On
//! `TestBackend` (no daemon, no profile, no repo). The worktrees are built
//! from JSON on purpose: the `origin` field is read through the wire form.

use gitraptor_api::catalog::Layer;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, EngineStateView, RepoStateView, RepoView as ApiRepo,
    WorktreeView,
};
use gitraptor_api::scope::{AutostartView, RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_cli::model::{
    ConnState, EngineMsg, GlobalView, Model, Msg, Requester, Size, Stamped,
};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::{Value, json};

fn worktree(path: &str, main: bool, branch: &str, origin: Option<Value>) -> WorktreeView {
    let mut w = json!({
        "path": {"untrusted": path},
        "main": main,
        "admin_name": null,
        "status": {
            "state": "ready",
            "head": {"kind": "branch", "name": {"untrusted": branch}},
            "counts": {"staged": 0, "unstaged": 0, "untracked": 0},
            "changes": [],
            "divergence": {"state": "no-commits"}
        }
    });
    if let Some(origin) = origin {
        w["origin"] = origin;
    }
    serde_json::from_value(w).expect("WorktreeView must accept origin")
}

fn model_with(lang: Lang, worktrees: Vec<WorktreeView>) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: 140,
            height: 30,
        },
    );
    model.conn = ConnState::Live;
    model.engine.requester = Some(Requester::Unattributed {
        layer: Layer::Cockpit,
    });
    model.engine.global.data = Some(GlobalView {
        repos: Vec::new(),
        engine: EngineStateView::Observing,
        git_version: None,
        autostart: AutostartView::Unknown,
        repo_count: 1,
        attention: Vec::new(),
    });
    update(
        &mut model,
        Msg::Engine(Stamped {
            recv_ns: 0,
            decoded_ns: 0,
            msg: EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(RepoSnapshot {
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
                    worktrees,
                    tier: None,
                    checked_utc_ms: None,
                    kept_temps: None,
                    prediction: None,
                },
            }))),
        }),
    );
    model
}

/// The screen line of the worktree on `branch`.
fn row_of(model: &Model, branch: &str) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|f| view(model, f)).unwrap();
    let lines = buffer_text(terminal.backend().buffer());
    lines
        .iter()
        .find(|l| l.contains(branch))
        .unwrap_or_else(|| panic!("no row for {branch} in:\n{}", lines.join("\n")))
        .clone()
}

fn fleet(lang: Lang) -> Model {
    model_with(
        lang,
        vec![
            worktree("/w/shop", true, "trunk", None),
            worktree(
                "/w/shop/orca-1",
                false,
                "orca-1",
                Some(json!({"tool": "orca", "evidence": "admin-marker"})),
            ),
            worktree(
                "/w/shop/codex-1",
                false,
                "codex-1",
                Some(json!({"tool": "codex", "evidence": "path-pattern"})),
            ),
            worktree(
                "/w/shop/manual-1",
                false,
                "manual-1",
                Some(json!({"tool": "unknown", "evidence": "none"})),
            ),
            worktree(
                "/w/shop/detached-1",
                false,
                "detached-1",
                Some(json!({"tool": "codex", "evidence": "path-pattern", "hands_off": "detached"})),
            ),
        ],
    )
}

#[test]
fn tui_labels_in_english() {
    let m = fleet(Lang::En);
    let certain = row_of(&m, "orca-1");
    assert!(
        certain.contains("Orca") && !certain.contains("Orca?"),
        "a certain origin shows the bare tool name: {certain}"
    );
    let likely = row_of(&m, "codex-1");
    assert!(
        likely.contains("Codex?"),
        "a probable origin shows `Codex?`: {likely}"
    );
    let unknown = row_of(&m, "manual-1");
    assert!(
        unknown.contains("foreign") && !unknown.contains("hands off"),
        "an unknown origin shows `foreign`: {unknown}"
    );
    let marked = row_of(&m, "detached-1");
    assert!(
        marked.contains("foreign · hands off"),
        "a hands-off worktree shows `foreign · hands off`: {marked}"
    );
    assert!(
        !marked.contains("Codex"),
        "the mark replaces the origin label: {marked}"
    );
}

#[test]
fn tui_labels_in_spanish() {
    let m = fleet(Lang::Es);
    let certain = row_of(&m, "orca-1");
    assert!(
        certain.contains("Orca") && !certain.contains("Orca?"),
        "{certain}"
    );
    let likely = row_of(&m, "codex-1");
    assert!(
        likely.contains("¿Codex?"),
        "a probable origin shows `¿Codex?`: {likely}"
    );
    let unknown = row_of(&m, "manual-1");
    assert!(
        unknown.contains("ajeno") && !unknown.contains("no tocar"),
        "an unknown origin shows `ajeno`: {unknown}"
    );
    let marked = row_of(&m, "detached-1");
    assert!(
        marked.contains("ajeno · no tocar"),
        "a hands-off worktree shows `ajeno · no tocar`: {marked}"
    );
}

#[test]
fn the_main_worktree_has_no_label() {
    for lang in [Lang::En, Lang::Es] {
        let main = row_of(&fleet(lang), "trunk");
        for label in ["foreign", "ajeno", "hands off", "no tocar", "Orca", "Codex"] {
            assert!(
                !main.contains(label),
                "the main row must not say {label}: {main}"
            );
        }
        // Control: a linked one does carry a label, so the absence is the rule's.
        assert!(row_of(&fleet(lang), "orca-1").contains("Orca"));
    }
}

#[test]
fn the_mark_beats_temporary_and_temporary_beats_the_origin() {
    let temp = std::env::temp_dir()
        .join("wtl005-scratch")
        .display()
        .to_string();
    let m = model_with(
        Lang::En,
        vec![
            worktree("/w/shop", true, "trunk", None),
            worktree(
                &format!("{temp}/marked"),
                false,
                "marked-1",
                Some(json!({"tool": "unknown", "evidence": "none", "hands_off": "locked"})),
            ),
            worktree(
                &format!("{temp}/plain"),
                false,
                "plain-1",
                Some(json!({"tool": "orca", "evidence": "admin-marker"})),
            ),
        ],
    );
    let marked = row_of(&m, "marked-1");
    assert!(
        marked.contains("foreign · hands off") && !marked.contains("temporary"),
        "the mark must win over `temporary`: {marked}"
    );
    let plain = row_of(&m, "plain-1");
    assert!(
        plain.contains("temporary") && !plain.contains("Orca"),
        "`temporary` must win over the origin: {plain}"
    );
}
