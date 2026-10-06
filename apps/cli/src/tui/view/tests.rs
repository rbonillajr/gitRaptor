//! The screen as US-CKP-001 shows it: the fleet composed from the widget library, fed only by
//! what the engine published, plus snapshots of the whole screen on a dark and a light
//! terminal.

use super::*;
use crate::model::{EngineMsg, GlobalView, Msg, Size, Stamped};
use crate::tui::gallery::buffer_text;
use crate::tui::update::update;
use crate::tui::widgets::tests::style_runs;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, EngineStateView, HeadView,
    RepoStateView, RepoView as ApiRepo, SessionView, SessionsListResult, WorktreeStatus,
    WorktreeView,
};
use gitraptor_api::scope::{AutostartView, RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use gitraptor_theme::{Background, ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

fn render(model: &Model, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view(model, f)).unwrap();
    terminal.backend().buffer().clone()
}

fn lines(buffer: &Buffer) -> Vec<String> {
    buffer_text(buffer)
}

fn screen(model: &Model, width: u16, height: u16) -> String {
    lines(&render(model, width, height)).join("\n")
}

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn ready(branch: &str, changes: u32, ahead: u64, behind: u64) -> WorktreeStatus {
    WorktreeStatus::Ready {
        head: HeadView::Branch {
            name: UntrustedName::new(branch),
        },
        counts: ChangeCounts {
            staged: 0,
            unstaged: changes,
            untracked: 0,
        },
        changes: Vec::new(),
        divergence: DivergenceView::Counted {
            ahead: CommitCountView {
                count: ahead,
                exact: true,
            },
            behind: CommitCountView {
                count: behind,
                exact: true,
            },
        },
    }
}

fn worktree(path: &str, main: bool, status: WorktreeStatus) -> WorktreeView {
    WorktreeView {
        path: Untrusted::new(path),
        main,
        admin_name: None,
        status,
    }
}

/// The repo "shop" of the story: main, feat-pagos with claude-1 Active, 2 files, ↑3 ↓1, and
/// feat-docs without an agent.
fn shop() -> Vec<WorktreeView> {
    vec![
        worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2, 3, 1)),
        worktree("/w/shop", true, ready("main", 0, 0, 0)),
        worktree("/w/shop/feat-docs", false, ready("feat-docs", 0, 1, 0)),
    ]
}

fn session(id: &str, worktree: &str, name: &str, state: SessionStateView) -> SessionView {
    SessionView {
        repo_id: "r1".into(),
        session_id: id.into(),
        worktree: Untrusted::new(worktree),
        actor: Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new(name)),
            origin: AgentOrigin::Registered,
        },
        state,
        started_utc_ms: 0,
        state_since_utc_ms: 1,
        utc_offset_s: 0,
        ended_utc_ms: None,
        end_cause: None,
    }
}

fn model_with(
    lang: Lang,
    worktrees: Vec<WorktreeView>,
    sessions: Option<Vec<SessionView>>,
) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: 100,
            height: 30,
        },
    );
    model.conn = ConnState::Live;
    model.engine.requester = Some(Requester::Unattributed {
        layer: gitraptor_api::catalog::Layer::Cockpit,
    });
    model.engine.global.data = Some(GlobalView {
        engine: EngineStateView::Observing,
        git_version: None,
        autostart: AutostartView::Unknown,
        repo_count: 1,
        attention: Vec::new(),
    });
    update(
        &mut model,
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
            RepoSnapshot {
                run_id: "run".into(),
                scope_seq: 1,
                repo: ApiRepo {
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/shop/.git"),
                    base: BaseBranchView {
                        name: Some(UntrustedName::new("main")),
                        status: BaseStatusView::Confirmed,
                    },
                    worktrees,
                },
            },
        )))),
    );
    if let Some(sessions) = sessions {
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
    }
    model
}

fn shop_model(lang: Lang) -> Model {
    model_with(
        lang,
        shop(),
        Some(vec![session(
            "s1",
            "/w/shop/feat-pagos",
            "claude-1",
            SessionStateView::Active,
        )]),
    )
}

fn row<'a>(screen: &'a [String], needle: &str) -> &'a str {
    screen
        .iter()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle}: {screen:#?}"))
}

/// Scenario 1: one row per worktree, the main one first and the agent first in the row.
#[test]
fn one_row_per_worktree_with_the_agent_first() {
    let screen = lines(&render(&shop_model(Lang::En), 100, 30));
    let rows: Vec<&String> = screen
        .iter()
        .filter(|l| l.contains("feat-") || l.contains(" main "))
        .collect();
    assert!(rows[0].contains("main"), "main first: {rows:#?}");
    let pagos = row(&screen, "feat-pagos");
    let agent = pagos.find("claude-1").unwrap();
    assert!(agent < pagos.find("feat-pagos").unwrap(), "{pagos}");
    // Active is the ● of DSYS § 2.2, before the name.
    assert!(pagos.contains("●  claude-1"), "{pagos}");
    assert!(pagos.contains("~2"), "{pagos}");
    assert!(pagos.contains("↑3 ↓1"), "{pagos}");
    assert!(
        screen[1].contains("↑↓ vs main (local copy, fetch age not available)"),
        "{}",
        screen[1]
    );
}

/// Scenario 3: a worktree without an agent session is "unattributed", never "human".
#[test]
fn a_worktree_without_an_agent_is_unattributed_never_human() {
    for (lang, text) in [
        (Lang::En, "Unattributed (you/other)"),
        (Lang::Es, "Tú u otro (sin atribuir)"),
    ] {
        let all = screen(&shop_model(lang), 100, 30);
        let screen = lines(&render(&shop_model(lang), 100, 30));
        assert!(row(&screen, "feat-docs").contains(text), "{all}");
        assert!(!all.to_lowercase().contains("human"), "{all}");
        assert!(!all.to_lowercase().contains("humano"), "{all}");
    }
}

/// Without the session list the TUI cannot tell: "agent not available", not "no agent".
#[test]
fn unknown_detection_is_not_presented_as_no_agent() {
    let model = model_with(Lang::En, shop(), None);
    let screen = lines(&render(&model, 100, 30));
    assert!(row(&screen, "feat-pagos").contains("agent not available"));
    assert!(!screen.join("\n").contains("Unattributed"));
}

/// Scenario 4: the last activity is not published, so every row says so.
#[test]
fn unpublished_activity_is_not_available() {
    for (lang, text) in [(Lang::En, "not available"), (Lang::Es, "no disponible")] {
        let screen = lines(&render(&shop_model(lang), 100, 30));
        for name in ["feat-pagos", "feat-docs"] {
            assert!(
                row(&screen, name)
                    .trim_end_matches('┃')
                    .trim_end()
                    .ends_with(text),
                "{name}: {screen:#?}"
            );
        }
    }
}

/// Scenario 5: a branch with an escape sequence is painted inert and visible.
#[test]
fn a_malicious_branch_is_painted_inert() {
    let model = model_with(
        Lang::En,
        vec![worktree("/w/x", true, ready("evil\u{1b}[2J", 0, 0, 0))],
        Some(Vec::new()),
    );
    let buffer = render(&model, 100, 30);
    let screen = lines(&buffer);
    assert!(row(&screen, "evil").contains("evil\\x1b[2J"), "{screen:#?}");
    let painted: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(!painted.contains('\u{1b}'));
}

/// The ⚡ and ⛔ counts the engine does not publish are "–", never 0 (BR-CKP-CALC-001).
#[test]
fn unpublished_attention_counts_are_not_zero() {
    let model = shop_model(Lang::En);
    let header = &lines(&render(&model, 100, 30))[0];
    assert!(header.contains("shop"), "{header}");
    assert!(header.contains("live · engine observing"), "{header}");
    assert!(header.contains("acting as you"), "{header}");
    assert!(header.ends_with("⚡ –  ⛔ –"), "{header}");
}

#[test]
fn a_worktree_the_engine_cannot_read_says_why() {
    let model = model_with(
        Lang::En,
        vec![worktree(
            "/w/gone",
            false,
            WorktreeStatus::Unavailable {
                reason: gitraptor_api::messages::UnavailableReason::Missing,
            },
        )],
        Some(Vec::new()),
    );
    let screen = lines(&render(&model, 100, 30));
    assert!(row(&screen, "folder missing").contains('⚠'), "{screen:#?}");
}

#[test]
fn a_bounded_count_is_at_least_and_an_uncounted_one_says_why() {
    let mut pagos = worktree("/w/a", true, ready("a", 0, 0, 0));
    if let WorktreeStatus::Ready { divergence, .. } = &mut pagos.status {
        *divergence = DivergenceView::Counted {
            ahead: CommitCountView {
                count: 10_000,
                exact: false,
            },
            behind: CommitCountView {
                count: 2,
                exact: true,
            },
        };
    }
    let mut lost = worktree("/w/b", false, ready("b", 0, 0, 0));
    if let WorktreeStatus::Ready { divergence, .. } = &mut lost.status {
        *divergence = DivergenceView::BaseMissing;
    }
    let screen = lines(&render(
        &model_with(Lang::En, vec![pagos, lost], Some(Vec::new())),
        100,
        30,
    ));
    assert!(row(&screen, " a ").contains("↑10000+ ↓2"), "{screen:#?}");
    assert!(row(&screen, " b ").contains("base absent"), "{screen:#?}");
}

#[test]
fn two_present_sessions_show_the_active_one_and_how_many_more() {
    let mut idle = session(
        "s0",
        "/w/shop/feat-pagos",
        "claude-0",
        SessionStateView::Inactive,
    );
    idle.state_since_utc_ms = 9;
    let model = model_with(
        Lang::En,
        shop(),
        Some(vec![
            idle,
            session(
                "s1",
                "/w/shop/feat-pagos",
                "claude-1",
                SessionStateView::Active,
            ),
        ]),
    );
    let screen = lines(&render(&model, 100, 30));
    assert!(
        row(&screen, "feat-pagos").contains("●  claude-1 +1"),
        "{screen:#?}"
    );
}

#[test]
fn below_80_by_24_only_the_size_message_is_painted() {
    let screen = screen(&shop_model(Lang::En), 79, 24);
    assert!(screen.contains("Terminal too small (79×24)"), "{screen}");
    assert!(!screen.contains("feat-pagos"));
    assert!(screen.contains("q quit"), "{screen}");
}

#[test]
fn without_a_repo_the_fleet_says_how_to_add_one() {
    let mut model = shop_model(Lang::Es);
    model.engine.repo = None;
    let screen = screen(&model, 100, 30);
    assert!(screen.contains("sin repo seleccionado"), "{screen}");
    assert!(screen.contains("raptor repo add"), "{screen}");
}

/// The whole screen at 80×24 and 120×40 on a dark and a light terminal: the text is the same,
/// only the colors change (DSYS-GRP-001, TS-CKP-004 amendment).
#[test]
fn the_fleet_on_a_dark_and_a_light_terminal() {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("snapshots");
    settings.bind(|| {
        for (w, h) in [(80, 24), (120, 40)] {
            let mut text = None;
            for (name, background) in [("dark", Background::Dark), ("light", Background::Light)] {
                let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode)
                    .with_background(background);
                let model = shop_model(Lang::En).with_theme(theme);
                let buffer = render(&model, w, h);
                let painted = lines(&buffer).join("\n");
                if let Some(text) = &text {
                    assert_eq!(text, &painted, "the background changed the text");
                } else {
                    text = Some(painted.clone());
                }
                let snap = format!(
                    "{painted}\n=== styles ({name}) ===\n{}",
                    style_runs(&buffer)
                );
                insta::assert_snapshot!(format!("fleet_{w}x{h}_{name}"), snap);
            }
        }
    });
}
