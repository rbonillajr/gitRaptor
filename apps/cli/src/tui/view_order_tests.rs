//! Snapshots of the fleet ordered by attention, with shared worktrees and the last agent of an
//! ended one, in English and Spanish. The theme is pinned (no color, ASCII symbols) so the
//! output never depends on the terminal or the OS; the clock is the fixed one of `Msg::Tick`.

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DivergenceView, EngineStateView,
    HeadView, RepoStateView, SessionEndCauseView, SessionStateView, SessionView,
    SessionsListResult, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::model::{ConnState, EngineMsg, GlobalView, Model, Msg, Requester, Size, Stamped};
use crate::present::i18n::Lang;
use crate::tui::gallery::buffer_text;
use crate::tui::update::update;
use crate::tui::view::view;

const MON9: i64 = 1_760_000_000_000;
const H: i64 = 3_600_000;

type Tree = (&'static str, &'static str);

const ORDER_TREES: &[Tree] = &[
    ("", "main"),
    ("a-none", "br-none"),
    ("b-done", "br-done"),
    ("c-idle", "br-idle"),
    ("d-act", "br-d"),
    ("m-act", "br-m"),
];
const PAGOS_TREES: &[Tree] = &[("", "main"), ("wt-pagos", "feat-pagos")];
const OLD_TREES: &[Tree] = &[
    ("", "main"),
    ("wt-old", "feat-old"),
    ("wt-pagos", "feat-pagos"),
];

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn path(folder: &str) -> String {
    if folder.is_empty() {
        "/w/shop".into()
    } else {
        format!("/w/shop/{folder}")
    }
}

fn worktrees(trees: &[Tree]) -> Vec<WorktreeView> {
    let zero = || CommitCountView {
        count: 0,
        exact: true,
    };
    trees
        .iter()
        .map(|(folder, branch)| WorktreeView {
            last_activity_utc_ms: None,
            last_activity_in_gap: false,
            detached_at: None,
            path: Untrusted::new(path(folder)),
            main: folder.is_empty(),
            admin_name: None,
            status: WorktreeStatus::Ready {
                head: HeadView::Branch {
                    name: UntrustedName::new(*branch),
                },
                counts: ChangeCounts {
                    staged: 0,
                    unstaged: 0,
                    untracked: 0,
                },
                changes: Vec::new(),
                divergence: DivergenceView::Counted {
                    ahead: zero(),
                    behind: zero(),
                },
            },
            operation: None,
            unmerged_paths: 0,
        })
        .collect()
}

fn snapshot(trees: &[Tree]) -> Msg {
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: "run".into(),
            scope_seq: 1,
            repo: gitraptor_api::messages::RepoView {
                fetched_utc_ms: None,
                repo_id: "r1".into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/shop/.git"),
                base: BaseBranchView {
                    name: Some(UntrustedName::new("main")),
                    status: BaseStatusView::Confirmed,
                },
                worktrees: worktrees(trees),
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
            },
        },
    ))))
}

fn sess(
    id: &str,
    agent: &str,
    folder: &str,
    state: SessionStateView,
    started: i64,
    end: Option<i64>,
) -> SessionView {
    SessionView {
        repo_id: "r1".into(),
        session_id: id.into(),
        worktree: Untrusted::new(path(folder)),
        actor: Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new(agent)),
            origin: AgentOrigin::Registered,
        },
        state,
        started_utc_ms: started,
        state_since_utc_ms: end.unwrap_or(started),
        utc_offset_s: 0,
        ended_utc_ms: end,
        end_cause: end.map(|_| SessionEndCauseView::RegistrationWithdrawn),
    }
}

fn active(id: &str, agent: &str, folder: &str, started: i64) -> SessionView {
    sess(id, agent, folder, SessionStateView::Active, started, None)
}

fn inactive(id: &str, agent: &str, folder: &str, started: i64) -> SessionView {
    sess(id, agent, folder, SessionStateView::Inactive, started, None)
}

fn ended(id: &str, agent: &str, folder: &str, started: i64, end: i64) -> SessionView {
    sess(
        id,
        agent,
        folder,
        SessionStateView::Ended,
        started,
        Some(end),
    )
}

/// A live model on "shop" at `now_ms`, with the symbol mode and color pinned.
fn fleet(
    lang: Lang,
    trees: &[Tree],
    sessions: Vec<SessionView>,
    now_ms: i64,
    size: (u16, u16),
) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: size.0,
            height: size.1,
        },
    );
    model.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Ascii);
    model.conn = ConnState::Live;
    model.engine.requester = Some(Requester::Unattributed {
        layer: gitraptor_api::catalog::Layer::Cockpit,
    });
    model.engine.global.data = Some(GlobalView {
        repos: Vec::new(),
        engine: EngineStateView::Observing,
        git_version: None,
        autostart: gitraptor_api::scope::AutostartView::Unknown,
        repo_count: 1,
        attention: Vec::new(),
    });
    update(&mut model, snapshot(trees));
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
    update(&mut model, Msg::Tick { now_ms });
    model
}

fn screen(model: &Model) -> String {
    let mut t = Terminal::new(TestBackend::new(model.ui.size.width, model.ui.size.height))
        .expect("test backend");
    t.draw(|f| view(model, f)).expect("draw");
    buffer_text(t.backend().buffer()).join("\n")
}

fn check(
    name: &str,
    trees: &[Tree],
    sessions: &dyn Fn() -> Vec<SessionView>,
    now_ms: i64,
    sizes: &[(u16, u16)],
) {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("snapshots");
    settings.bind(|| {
        for &(w, h) in sizes {
            for (tag, lang) in [("en", Lang::En), ("es", Lang::Es)] {
                let model = fleet(lang, trees, sessions(), now_ms, (w, h));
                let text = screen(&model);
                // Spanish text keeps its accents: ASCII folds the symbols, not the words.
                assert!(lang == Lang::Es || text.is_ascii(), "{text}");
                insta::assert_snapshot!(format!("{name}_{w}x{h}_{tag}"), text);
            }
        }
    });
}

#[test]
fn the_fleet_order_in_english_and_spanish() {
    check(
        "fleet_order",
        ORDER_TREES,
        &|| {
            vec![
                ended("s-done", "claude-4", "b-done", MON9 - 5 * H, MON9 - 2 * H),
                inactive("s-idle", "claude-3", "c-idle", MON9 - 4 * H),
                active("s-d", "claude-1", "d-act", MON9 - 3 * H),
                active("s-m", "claude-2", "m-act", MON9 - 3 * H),
            ]
        },
        MON9,
        &[(80, 24), (120, 40)],
    );
}

#[test]
fn a_shared_worktree_in_english_and_spanish() {
    check(
        "fleet_shared",
        PAGOS_TREES,
        &|| {
            vec![
                active("s1", "claude-1", "wt-pagos", MON9 - 5 * H),
                inactive("s2", "claude-2", "wt-pagos", MON9 - 4 * H),
                ended("s3", "claude-3", "wt-pagos", MON9 - 3 * H, MON9 - H),
            ]
        },
        MON9,
        &[(80, 24)],
    );
}

#[test]
fn the_last_agent_in_english_and_spanish() {
    let end = MON9;
    check(
        "fleet_last_agent",
        OLD_TREES,
        &|| {
            vec![
                ended("s4", "claude-4", "wt-old", end - 3 * H, end),
                active("s1", "claude-1", "wt-pagos", end - 3 * H),
            ]
        },
        end + 24 * H,
        &[(80, 24)],
    );
}
