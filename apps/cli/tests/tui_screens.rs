//! US-CKP-005: the whole screen of the TUI in a small terminal, without color, in ASCII, in
//! the user's language and with the keyboard help, on `TestBackend` (no daemon, no profile,
//! no repo). Driven only through the public model: keys, `Msg::Resize`, the theme.

use gitraptor_api::catalog::Layer;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DivergenceView, EngineStateView,
    HeadView, RepoStateView, RepoView as ApiRepo, SessionStateView, SessionView,
    SessionsListResult, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{AutostartView, RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use gitraptor_cli::model::{
    Candidate, ConnEvent, ConnState, EngineMsg, Found, GlobalView, Model, Msg, ObserveFailure,
    RepoChoice, Requester, Size, Stamped,
};
use gitraptor_cli::present::SafeText;
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

fn render(model: &Model, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view(model, f)).unwrap();
    terminal.backend().buffer().clone()
}

fn lines(model: &Model, width: u16, height: u16) -> Vec<String> {
    buffer_text(&render(model, width, height))
}

fn screen(model: &Model, width: u16, height: u16) -> String {
    lines(model, width, height).join("\n")
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

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ready(branch: &str, changes: u32) -> WorktreeStatus {
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
                count: 0,
                exact: true,
            },
            behind: CommitCountView {
                count: 0,
                exact: true,
            },
        },
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

fn model_with(lang: Lang, worktrees: Vec<WorktreeView>, sessions: Vec<SessionView>) -> Model {
    let mut model = Model::new(
        lang,
        Size {
            width: 100,
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

/// "shop": main, feat-pagos with claude-1 active, feat-idle with claude-2 idle, feat-docs
/// without an agent and feat-gone whose folder is missing.
fn shop(lang: Lang) -> Model {
    model_with(
        lang,
        vec![
            worktree("/w/shop", true, ready("main", 0)),
            worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2)),
            worktree("/w/shop/feat-idle", false, ready("feat-idle", 1)),
            worktree("/w/shop/feat-docs", false, ready("feat-docs", 0)),
            worktree(
                "/w/shop/feat-gone",
                false,
                WorktreeStatus::Unavailable {
                    reason: UnavailableReason::Missing,
                },
            ),
        ],
        vec![
            session(
                "s1",
                "/w/shop/feat-pagos",
                "claude-1",
                SessionStateView::Active,
            ),
            session(
                "s2",
                "/w/shop/feat-idle",
                "claude-2",
                SessionStateView::Inactive,
            ),
        ],
    )
}

fn with_theme(mut model: Model, mode: ColorMode, symbols: SymbolSet) -> Model {
    model.ui.theme = Theme::new(mode, Contrast::Normal, symbols);
    model
}

/// Not in an observed repo, with several observed: the picker.
fn picker(lang: Lang) -> Model {
    let mut model = shop(lang);
    model.engine.repo = None;
    model.engine.global.data.as_mut().unwrap().repos = ["shop", "blog", "api"]
        .iter()
        .map(|name| RepoChoice {
            repo_id: format!("id-{name}"),
            name: SafeText::name(name),
            path: SafeText::text(&format!("/w/{name}/.git")),
            tier: None,
        })
        .collect();
    update(&mut model, Msg::Conn(ConnEvent::Unlocated));
    model
}

/// "notes" not observed, asked about as the developer.
fn asking(lang: Lang) -> Model {
    let mut model = picker(lang);
    update(
        &mut model,
        Msg::Conn(ConnEvent::Unobserved(Candidate {
            root: "/w/notes".into(),
            name: SafeText::name("notes"),
            path: SafeText::text("/w/notes"),
        })),
    );
    model
}

/// A repo found in a code folder, asked under the fleet.
fn discovering(lang: Lang) -> Model {
    let mut model = shop(lang);
    update(
        &mut model,
        Msg::Conn(ConnEvent::Discovered(vec![Found {
            path: "/code/notes".into(),
            name: SafeText::name("notes"),
            shown: SafeText::text("/code/notes"),
            root: SafeText::text("/code"),
        }])),
    );
    model
}

/// The header with the answer of a failed `repo.add`.
fn observe_failed(lang: Lang) -> Model {
    let mut model = asking(lang);
    update(&mut model, key(KeyCode::Char('y')));
    update(
        &mut model,
        Msg::Conn(ConnEvent::ObserveFailed(ObserveFailure::NotTrusted)),
    );
    model
}

fn resized(model: &mut Model, width: u16, height: u16) {
    update(model, Msg::Resize(Size { width, height }));
}

fn row<'a>(screen: &'a [String], needle: &str) -> &'a str {
    screen
        .iter()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle}: {screen:#?}"))
}

/// Scenario 1 (D1): under 80×24 only the notice with its action; resizing paints the fleet in
/// the same model.
#[test]
fn too_small_then_resized_without_restart() {
    for (lang, action) in [
        (Lang::En, "Resize it to at least 80×24."),
        (Lang::Es, "Amplía la terminal a 80×24 como mínimo."),
    ] {
        let mut model = shop(lang);
        resized(&mut model, 70, 20);
        let small = flat(&model, 70, 20);
        assert!(small.contains("(70×20)"), "{small}");
        assert!(small.contains(action), "{small}");
        assert!(!small.contains("feat-pagos"), "{small}");
        resized(&mut model, 80, 24);
        let fleet = flat(&model, 80, 24);
        assert!(fleet.contains("feat-pagos"), "{fleet}");
        assert!(!fleet.contains(action), "{fleet}");
    }
}

/// Scenario 3 (D4): no cell carries a color and each state reads by its symbol and its text.
#[test]
fn every_state_reads_without_color() {
    for (symbols, [active, idle, none, gone]) in [
        (SymbolSet::Unicode, ["●", "◐", "○", "⚠"]),
        (SymbolSet::Ascii, ["*", "~", "o", "[!]"]),
    ] {
        let model = with_theme(shop(Lang::En), ColorMode::NoColor, symbols);
        let buffer = render(&model, 100, 30);
        for cell in buffer.content() {
            assert!(
                cell.fg == Color::Reset && cell.bg == Color::Reset,
                "{:?} paints a color ({:?}/{:?}):\n{}",
                cell.symbol(),
                cell.fg,
                cell.bg,
                buffer_text(&buffer).join("\n")
            );
        }
        let shown = buffer_text(&buffer);
        let table = [
            ("claude-1", active),
            ("claude-2", idle),
            ("No agent", none),
            ("feat-gone", gone),
        ];
        for (text, symbol) in table {
            let r = row(&shown, text);
            assert!(
                r.contains(symbol),
                "{symbol} beside {text}: {r}\n{shown:#?}"
            );
        }
        // The bar tells what the symbol of an active agent means.
        let all = shown.join("\n");
        assert!(all.contains(&format!("{active} active agent")), "{all}");
        let gone_row = row(&shown, "feat-gone");
        assert!(gone_row.contains("folder missing"), "{gone_row}");
        assert!(row(&shown, "feat-docs").contains("No agent"), "{shown:#?}");
    }
}

fn is_plain(c: char) -> bool {
    c.is_ascii() || c.is_alphabetic() || c == '¿' || c == '¡'
}

fn assert_plain(what: &str, painted: &str) {
    let odd: Vec<char> = painted.chars().filter(|c| !is_plain(*c)).collect();
    assert!(odd.is_empty(), "{what} paints {odd:?}:\n{painted}");
}

/// Scenario 5 (D3, D6): in ASCII no screen paints a Unicode glyph.
#[test]
fn ascii_screens_paint_only_ascii() {
    for lang in [Lang::En, Lang::Es] {
        let ascii = |m: Model| with_theme(m, ColorMode::NoColor, SymbolSet::Ascii);
        let mut help = ascii(shop(lang));
        update(&mut help, key(KeyCode::Char('?')));
        let mut small = ascii(shop(lang));
        resized(&mut small, 70, 20);
        let screens = [
            ("fleet", ascii(shop(lang)), 100, 30),
            ("picker", ascii(picker(lang)), 100, 30),
            ("question", ascii(asking(lang)), 100, 30),
            ("discovered", ascii(discovering(lang)), 100, 30),
            ("help", help, 100, 30),
            ("small", small, 70, 20),
            ("observe failed", ascii(observe_failed(lang)), 120, 30),
        ];
        for (what, model, w, h) in screens {
            assert_plain(&format!("{what} ({lang:?})"), &screen(&model, w, h));
        }
    }
}

/// Scenario 4: one language on the screen.
#[test]
fn the_screen_speaks_the_language() {
    for (lang, present, absent) in [
        (
            Lang::Es,
            ["Flota", "salir", "ayuda"],
            ["Fleet", "quit", "help"],
        ),
        (
            Lang::En,
            ["Fleet", "quit", "help"],
            ["Flota", "salir", "ayuda"],
        ),
    ] {
        let painted = screen(&shop(lang), 120, 30);
        for word in present {
            assert!(painted.contains(word), "{word}\n{painted}");
        }
        for word in absent {
            assert!(!painted.contains(word), "{word}\n{painted}");
        }
    }
}

/// Scenario 5 (D5): `?` opens and closes the help; Esc closes; `q` quits; `r` is unknown.
#[test]
fn question_mark_opens_and_closes_the_help() {
    let always = "Siempre";
    let mut model = shop(Lang::Es);
    assert!(!screen(&model, 100, 30).contains(always));
    update(&mut model, key(KeyCode::Char('?')));
    assert!(screen(&model, 100, 30).contains(always), "? opens");
    update(&mut model, key(KeyCode::Char('?')));
    assert!(!screen(&model, 100, 30).contains(always), "? closes");
    update(&mut model, key(KeyCode::Char('?')));
    update(&mut model, key(KeyCode::Esc));
    assert!(!screen(&model, 100, 30).contains(always), "Esc closes");
    // `r` with the help open: unknown key, and the help stays.
    update(&mut model, key(KeyCode::Char('?')));
    let cmds = update(&mut model, key(KeyCode::Char('r')));
    let _ = cmds;
    let painted = screen(&model, 100, 30);
    assert!(painted.contains(always), "the help stays open:\n{painted}");
    assert!(
        painted.contains("tecla sin acción"),
        "unknown key notice:\n{painted}"
    );
    assert!(!model.ui.quit);
    // `q` quits with the help open.
    update(&mut model, key(KeyCode::Char('q')));
    assert!(model.ui.quit, "q quits with the help open");
}

/// Scenario 5 (D5): the help shows the keys of the current panel and a symbol legend.
#[test]
fn question_mark_shows_the_keys_of_the_panel() {
    let open = |mut m: Model| {
        resized(&mut m, 80, 24);
        update(&mut m, key(KeyCode::Char('?')));
        flat(&m, 80, 24)
    };
    let fleet = open(shop(Lang::Es));
    for token in ["Siempre", "Ctrl-Z", "q", "r", "?", "Esc"] {
        assert!(fleet.contains(token), "{token}\n{fleet}");
    }
    // The fleet title and its columns keep their own `↑↓` (ahead/behind): what the fleet help
    // must not list is the picker's keys.
    assert!(!fleet.contains("↑ k"), "{fleet}");
    assert!(!fleet.contains("Enter"), "{fleet}");

    let pick = open(picker(Lang::Es));
    for token in ["↑ k", "↓ j", "Enter", "Esc"] {
        assert!(pick.contains(token), "{token}\n{pick}");
    }

    let question = open(asking(Lang::Es));
    for token in ["s y", "n", "Esc"] {
        assert!(question.contains(token), "{token}\n{question}");
    }

    // The symbol legend: the last symbol's meaning is on a 80×24 screen.
    assert!(fleet.contains("aviso"), "{fleet}");
}

/// Scenario 3 (D4): the bar ends with the legend of the states on screen.
#[test]
fn the_bar_explains_the_states_on_screen() {
    let main = || worktree("/w/shop", true, ready("main", 0));
    let pagos = || worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2));
    let idle = || worktree("/w/shop/feat-idle", false, ready("feat-idle", 1));
    let sessions = |idle_too: bool| {
        let mut all = vec![
            session("s0", "/w/shop", "claude-0", SessionStateView::Active),
            session(
                "s1",
                "/w/shop/feat-pagos",
                "claude-1",
                SessionStateView::Active,
            ),
        ];
        if idle_too {
            all.push(session(
                "s2",
                "/w/shop/feat-idle",
                "claude-2",
                SessionStateView::Inactive,
            ));
        }
        all
    };
    for (symbols, active, inactive) in [
        (SymbolSet::Unicode, "● agente activo", "◐ agente inactivo"),
        (SymbolSet::Ascii, "* agente activo", "~ agente inactivo"),
    ] {
        let model = with_theme(
            model_with(Lang::Es, vec![main(), pagos(), idle()], sessions(true)),
            ColorMode::NoColor,
            symbols,
        );
        let shown = lines(&model, 120, 30);
        let bar = shown.iter().rev().find(|l| !l.trim().is_empty()).unwrap();
        let (a, i) = (bar.find(active), bar.find(inactive));
        assert!(a.is_some() && i.is_some(), "{bar}");
        assert!(a < i, "active before idle: {bar}");
        // Without an idle row there is no idle entry.
        let no_idle = with_theme(
            model_with(Lang::Es, vec![main(), pagos()], sessions(false)),
            ColorMode::NoColor,
            symbols,
        );
        let shown = screen(&no_idle, 120, 30);
        assert!(shown.contains(active), "{shown}");
        assert!(!shown.contains("agente inactivo"), "{shown}");
        assert!(!shown.contains('◐'), "{shown}");
    }
}

/// D3/D5: the ASCII bar shows the alternative keys (`k`, `j`) instead of the arrows and gives
/// `? ayuda` its fixed place (`Up`/`Down` are the labels of the help, not of the bar).
#[test]
fn the_ascii_bar_never_shows_arrow_glyphs() {
    let model = with_theme(picker(Lang::Es), ColorMode::NoColor, SymbolSet::Ascii);
    let shown = lines(&model, 100, 30);
    let bar = shown.iter().rev().find(|l| !l.trim().is_empty()).unwrap();
    for glyph in ['↑', '↓'] {
        assert!(!bar.contains(glyph), "{bar}");
    }
    assert!(bar.contains("k ") || bar.contains(" k"), "{bar}");
    assert!(bar.contains('j'), "{bar}");
    assert!(bar.trim_end().ends_with("? ayuda"), "{bar}");
}

/// BR-CKP-EDGE-006: nine agents are told apart by name without color.
#[test]
fn nine_agents_are_told_apart_by_name() {
    let mut worktrees = vec![worktree("/w/shop", true, ready("main", 0))];
    let mut sessions = Vec::new();
    for n in 1..=9 {
        let path = format!("/w/shop/feat-{n}");
        worktrees.push(worktree(&path, false, ready(&format!("feat-{n}"), 1)));
        sessions.push(session(
            &format!("s{n}"),
            &path,
            &format!("claude-{n}"),
            SessionStateView::Active,
        ));
    }
    let mut model = with_theme(
        model_with(Lang::En, worktrees, sessions),
        ColorMode::NoColor,
        SymbolSet::Unicode,
    );
    resized(&mut model, 120, 40);
    let shown = lines(&model, 120, 40);
    let rows: Vec<&str> = (1..=9)
        .map(|n| row(&shown, &format!("claude-{n}")))
        .collect();
    for (i, a) in rows.iter().enumerate() {
        for b in &rows[i + 1..] {
            assert_ne!(a, b);
        }
    }
}
