//! US-CKP-003: what the TUI says about the engine and the connection, on `TestBackend` (no
//! daemon, no profile, no repo), in English and Spanish, without color and in ASCII symbols
//! (pinned in every test: the symbol mode can differ on Windows). Driven only through the public
//! model: connection events, engine messages and the theme. The base-branch title is the one
//! check made with Unicode symbols, pinned too, because its `↑↓` is part of the text.

use gitraptor_api::catalog::Layer;
use gitraptor_api::event::{Event, REPO_TIER};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DaemonView, DivergenceView,
    EngineStateView, EngineView, HeadView, RepoStateView, RepoTier, RepoTierData, ResyncReason,
    UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{
    AttentionCount, AttentionView, AutostartView, GlobalSnapshot, RepoSnapshot, RepoSummaryView,
    Scope, ScopeSnapshot,
};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_cli::model::{
    Candidate, ConnEvent, ConnState, EngineMsg, Model, Msg, Requester, Size, Stamped,
};
use gitraptor_cli::present::SafeText;
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;

fn lines(model: &Model, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view(model, f)).unwrap();
    buffer_text(terminal.backend().buffer())
}

/// The screen with every line trimmed and joined by a space: a message that wraps reads as one.
fn flat(model: &Model, width: u16, height: u16) -> String {
    lines(model, width, height)
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn pinned(mut model: Model, symbols: SymbolSet) -> Model {
    model.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, symbols);
    model
}

fn ready(branch: &str, divergence: DivergenceView) -> WorktreeStatus {
    WorktreeStatus::Ready {
        head: HeadView::Branch {
            name: UntrustedName::new(branch),
        },
        counts: ChangeCounts {
            staged: 0,
            unstaged: 1,
            untracked: 0,
        },
        changes: Vec::new(),
        divergence,
    }
}

fn counted() -> DivergenceView {
    let zero = || CommitCountView {
        count: 0,
        exact: true,
    };
    DivergenceView::Counted {
        ahead: zero(),
        behind: zero(),
    }
}

fn worktree(path: &str, main: bool, status: WorktreeStatus) -> WorktreeView {
    WorktreeView {
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        path: Untrusted::new(path),
        main,
        admin_name: None,
        status,
        operation: None,
        unmerged_paths: 0,
    }
}

fn repo_msg(base: &str, worktrees: Vec<WorktreeView>) -> Msg {
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
                    name: Some(UntrustedName::new(base)),
                    status: BaseStatusView::Confirmed,
                },
                worktrees,
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
            },
        },
    ))))
}

fn live_model(lang: Lang) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: 120,
            height: 30,
        },
    );
    model.conn = ConnState::Live;
    model.engine.requester = Some(Requester::Unattributed {
        layer: Layer::Cockpit,
    });
    model
}

/// "shop" observed with the given worktrees and base, the engine observing.
fn fleet(lang: Lang, base: &str, worktrees: Vec<WorktreeView>) -> Model {
    let mut model = live_model(lang);
    update(
        &mut model,
        global_msg(1, EngineStateView::Observing, Some(None)),
    );
    update(&mut model, repo_msg(base, worktrees));
    model
}

fn shop(lang: Lang) -> Model {
    fleet(
        lang,
        "main",
        vec![
            worktree("/w/shop", true, ready("main", counted())),
            worktree("/w/shop/feat-pagos", false, ready("feat-pagos", counted())),
            worktree(
                "/w/shop/feat-gone",
                false,
                WorktreeStatus::Unavailable {
                    reason: UnavailableReason::Missing,
                },
            ),
        ],
    )
}

/// No repo selected, the engine in `state` with no repos observed.
fn empty_engine(lang: Lang, state: EngineStateView) -> Model {
    let mut model = live_model(lang);
    update(&mut model, global_msg(1, state, None));
    model
}

/// The global scope with "r1" at `tier`, as the engine publishes it.
/// `repos`: `None` for no repos, `Some(tier)` for "r1" at that tier.
fn global_msg(scope_seq: u64, state: EngineStateView, repos: Option<Option<RepoTier>>) -> Msg {
    let none = || AttentionCount::Counted { count: 0 };
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Global(
        GlobalSnapshot {
            run_id: "g".into(),
            scope_seq,
            engine: EngineView {
                state,
                git_version: None,
            },
            daemon: DaemonView {
                pid: 1,
                protocol: gitraptor_api::PROTOCOL_VERSION,
                binary_version: "0.0.0".into(),
                started_wall_ms: 0,
            },
            autostart: AutostartView::Unknown,
            repos: repos
                .into_iter()
                .map(|tier| RepoSummaryView {
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/shop/.git"),
                    attention: AttentionView {
                        conflicts: none(),
                        denials: none(),
                        gaps: none(),
                    },
                    tier,
                })
                .collect(),
        },
    ))))
}

fn tier_event(scope_seq: u64, tier: RepoTier) -> Msg {
    engine(EngineMsg::Event {
        scope: Scope::Global,
        scope_seq,
        event: Box::new(Event {
            seq: 100 + scope_seq,
            kind: REPO_TIER.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: json!(RepoTierData {
                repo_id: "r1".into(),
                tier,
                checked_utc_ms: None,
            }),
        }),
    })
}

/// D2/D4: a connection without an engine replaces the fleet with a panel with its title and
/// its action, stale data included; the other connections make no panel.
#[test]
fn a_connection_without_an_engine_replaces_the_fleet_with_a_panel() {
    for (lang, cases) in [
        (
            Lang::En,
            [
                (
                    ConnState::EngineUnavailable,
                    "Engine unavailable",
                    "raptor daemon enable",
                ),
                (
                    ConnState::Incompatible,
                    "Incompatible engine",
                    "update both to the same version",
                ),
                (
                    ConnState::Rejected,
                    "Engine channel refused",
                    "only your user can open it",
                ),
                (
                    ConnState::Unsupported,
                    "No engine channel",
                    "use `raptor status`",
                ),
            ],
        ),
        (
            Lang::Es,
            [
                (
                    ConnState::EngineUnavailable,
                    "Motor no disponible",
                    "raptor daemon enable",
                ),
                (
                    ConnState::Incompatible,
                    "Motor incompatible",
                    "actualiza los dos a la misma versión",
                ),
                (
                    ConnState::Rejected,
                    "El motor rechazó el canal",
                    "solo tu usuario puede abrirla",
                ),
                (
                    ConnState::Unsupported,
                    "Sin canal con el motor",
                    "usa `raptor status`",
                ),
            ],
        ),
    ] {
        for (conn, title, action) in cases {
            // The fleet below has data (it is old): the panel still takes its place.
            let mut model = pinned(shop(lang), SymbolSet::Ascii);
            model.conn = conn;
            let screen = flat(&model, 120, 30);
            assert!(
                screen.contains(title),
                "{lang:?} {conn:?}: {title}\n{screen}"
            );
            assert!(
                screen.contains(action),
                "{lang:?} {conn:?}: {action}\n{screen}"
            );
            assert!(
                !screen.contains("feat-pagos"),
                "{lang:?} {conn:?}: the old fleet is still there\n{screen}"
            );
        }
        // The other connections make no panel and keep the fleet.
        for conn in [
            ConnState::Connecting,
            ConnState::Starting,
            ConnState::Syncing,
            ConnState::Live,
            ConnState::Resyncing,
            ConnState::Reconnecting { attempt: 2 },
        ] {
            let mut model = pinned(shop(lang), SymbolSet::Ascii);
            model.conn = conn;
            let screen = flat(&model, 120, 30);
            assert!(screen.contains("feat-pagos"), "{lang:?} {conn:?}\n{screen}");
            for title in [
                "Engine unavailable",
                "Motor no disponible",
                "Incompatible engine",
            ] {
                assert!(!screen.contains(title), "{lang:?} {conn:?}\n{screen}");
            }
        }
    }
}

/// D2/D3: waiting for Git says what is missing, wins over "no repos" and never asks to observe
/// the folder's repo.
#[test]
fn waiting_for_git_wins_over_no_repos_and_never_asks_to_observe() {
    for (lang, title, no_repos, question) in [
        (
            Lang::En,
            "Waiting for Git",
            "No repos observed",
            "Observe this repo?",
        ),
        (
            Lang::Es,
            "Esperando Git",
            "Sin repos observados",
            "¿Observar este repo?",
        ),
    ] {
        let mut model = pinned(
            empty_engine(lang, EngineStateView::WaitingForGit),
            SymbolSet::Ascii,
        );
        update(&mut model, Msg::Conn(ConnEvent::Unlocated));
        let screen = flat(&model, 120, 30);
        assert!(screen.contains(title), "{lang:?}\n{screen}");
        assert!(screen.contains("2.38"), "{lang:?}\n{screen}");
        assert!(!screen.contains(no_repos), "{lang:?}\n{screen}");
        assert!(!screen.contains("raptor repo add"), "{lang:?}\n{screen}");

        // The folder is in a repo the engine does not observe: still no question.
        let mut asked = pinned(
            empty_engine(lang, EngineStateView::WaitingForGit),
            SymbolSet::Ascii,
        );
        update(
            &mut asked,
            Msg::Conn(ConnEvent::Unobserved(Candidate {
                root: "/w/notes".into(),
                name: SafeText::name("notes"),
                path: SafeText::text("/w/notes"),
            })),
        );
        let screen = flat(&asked, 120, 30);
        assert!(screen.contains(title), "{lang:?}\n{screen}");
        assert!(!screen.contains(question), "{lang:?}\n{screen}");
    }
}

/// Scenario 4: with no repos observed the panel guides to the first one, with the command.
#[test]
fn no_repos_shows_how_to_add_the_first() {
    for (lang, title, command) in [
        (Lang::En, "No repos observed", "raptor repo add <path>"),
        (Lang::Es, "Sin repos observados", "raptor repo add <ruta>"),
    ] {
        let mut model = pinned(
            empty_engine(lang, EngineStateView::NoRepos),
            SymbolSet::Ascii,
        );
        update(&mut model, Msg::Conn(ConnEvent::Unlocated));
        let screen = flat(&model, 120, 30);
        assert!(screen.contains(title), "{lang:?}\n{screen}");
        assert!(screen.contains(command), "{lang:?}\n{screen}");
    }
}

/// D5/D6: the header says reconciling or asleep by the repo's tier, and why it resyncs while it
/// lasts, also during the reconnection; arriving at live clears it.
#[test]
fn the_header_says_reconciling_dormant_or_why_it_resyncs() {
    for (lang, reconciling, dormant, restarted, replay, live) in [
        (
            Lang::En,
            "reconciling: provisional data",
            "asleep: data may lag",
            "the engine restarted",
            "the engine no longer has the missed events",
            "engine observing",
        ),
        (
            Lang::Es,
            "reconciliando: datos provisionales",
            "dormido: datos con retraso",
            "el motor se reinició",
            "el motor ya no tiene los eventos perdidos",
            "motor observando",
        ),
    ] {
        let mut model = pinned(shop(lang), SymbolSet::Ascii);
        update(
            &mut model,
            global_msg(1, EngineStateView::Observing, Some(Some(RepoTier::Waking))),
        );
        let header = flat(&model, 120, 30);
        assert!(header.contains(reconciling), "{lang:?} waking\n{header}");
        assert!(
            !header.contains(live),
            "{lang:?} waking replaces the engine text\n{header}"
        );

        update(&mut model, tier_event(2, RepoTier::Dormant));
        let header = flat(&model, 120, 30);
        assert!(header.contains(dormant), "{lang:?} dormant\n{header}");
        assert!(!header.contains(reconciling), "{lang:?}\n{header}");

        update(&mut model, tier_event(3, RepoTier::Active));
        let header = flat(&model, 120, 30);
        assert!(header.contains(live), "{lang:?} active\n{header}");
        assert!(
            !header.contains(dormant) && !header.contains(reconciling),
            "{lang:?}\n{header}"
        );

        // `scope.resync` from the daemon: the reason is painted while it resyncs...
        update(
            &mut model,
            engine(EngineMsg::Resync {
                scope: Scope::Global,
                reason: ResyncReason::DaemonRestarted,
            }),
        );
        assert_eq!(model.conn, ConnState::Resyncing);
        let header = flat(&model, 120, 30);
        assert!(header.contains(restarted), "{lang:?} resyncing\n{header}");
        // ...and the reconnection that follows keeps saying it.
        update(
            &mut model,
            Msg::Conn(ConnEvent::State(ConnState::Reconnecting { attempt: 1 })),
        );
        let header = flat(&model, 120, 30);
        assert!(
            header.contains(restarted),
            "{lang:?} reconnecting\n{header}"
        );
        // Arriving at live clears it.
        update(&mut model, Msg::Conn(ConnEvent::State(ConnState::Syncing)));
        update(
            &mut model,
            global_msg(0, EngineStateView::Observing, Some(Some(RepoTier::Active))),
        );
        update(&mut model, Msg::Conn(ConnEvent::State(ConnState::Live)));
        let header = flat(&model, 120, 30);
        assert!(header.contains(live), "{lang:?} live again\n{header}");
        assert!(
            !header.contains(restarted),
            "{lang:?} the reason is gone\n{header}"
        );

        // A resync that ends at the new snapshot (no reconnection) clears it too. The repo scope
        // follows its own stream, so it has its snapshot as well.
        update(&mut model, repo_msg("main", Vec::new()));
        update(
            &mut model,
            engine(EngineMsg::Resync {
                scope: Scope::Global,
                reason: ResyncReason::ReplayUnavailable,
            }),
        );
        let header = flat(&model, 120, 30);
        assert!(header.contains(replay), "{lang:?} resyncing\n{header}");
        update(
            &mut model,
            global_msg(0, EngineStateView::Observing, Some(Some(RepoTier::Active))),
        );
        assert_eq!(model.conn, ConnState::Live);
        let header = flat(&model, 120, 30);
        assert!(!header.contains(replay), "{lang:?}\n{header}");
        assert!(header.contains(live), "{lang:?}\n{header}");
    }
}

/// D7: a worktree whose folder is missing says so, with the warning symbol.
#[test]
fn an_unavailable_worktree_says_not_available() {
    for (lang, text) in [
        (Lang::En, "unavailable: folder missing"),
        (Lang::Es, "no disponible: falta la carpeta"),
    ] {
        let model = pinned(shop(lang), SymbolSet::Ascii);
        let shown = lines(&model, 120, 30);
        let row = shown
            .iter()
            .find(|l| l.contains("[!]"))
            .unwrap_or_else(|| panic!("no row: {shown:#?}"));
        assert!(row.contains(text), "{lang:?}: {row}\n{shown:#?}");
    }
}

/// D8: with the base branch missing the cell says "not computable", never `–`; the title names
/// the published base, and no row counts against another branch.
#[test]
fn a_missing_base_names_the_published_base_and_never_counts() {
    for (lang, cell, title) in [
        (
            Lang::En,
            "not computable",
            "Fleet · ↑↓ not computable: develop not found",
        ),
        (
            Lang::Es,
            "no calculable",
            "Flota · ↑↓ no calculable: develop no encontrada",
        ),
    ] {
        let model = pinned(
            fleet(
                lang,
                "develop",
                vec![
                    worktree("/w/shop", true, ready("trunk", DivergenceView::BaseMissing)),
                    worktree(
                        "/w/shop/feat-a",
                        false,
                        ready("feat-a", DivergenceView::BaseMissing),
                    ),
                    worktree(
                        "/w/shop/feat-b",
                        false,
                        ready("feat-b", DivergenceView::BaseMissing),
                    ),
                ],
            ),
            SymbolSet::Unicode,
        );
        let screen = flat(&model, 120, 40);
        assert!(screen.contains(title), "{lang:?}\n{screen}");
        let shown = lines(&model, 120, 40);
        for branch in ["trunk", "feat-a", "feat-b"] {
            let row = shown
                .iter()
                .find(|l| l.contains(branch))
                .unwrap_or_else(|| panic!("no row {branch}: {shown:#?}"));
            assert!(row.contains(cell), "{lang:?} {branch}: {row}");
            for glyph in ['–', '↑', '↓'] {
                assert!(
                    !row.contains(glyph),
                    "{lang:?} {branch} shows {glyph}: {row}"
                );
            }
        }
    }
}
