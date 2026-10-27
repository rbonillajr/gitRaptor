//! US-CKP-002: the fleet ordered by what asks for attention, with the ended sessions, on
//! `TestBackend` (no daemon, no profile, no repo), in English and Spanish. Driven only through the
//! public model: `update` with engine snapshots, session lists, `session.state` events,
//! `Lagged`, connection events, `Msg::Tick` (the fixed clock of the view) and keys. The symbol
//! mode is pinned in every test (Unicode, because the Spanish texts have accents that ASCII
//! folds away); color is `NoColor` unless the test is about color (then `TrueColor`).

use gitraptor_api::event::{Event, SESSION_STATE};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, DivergenceView, EngineStateView,
    HeadView, RepoStateView, ResyncReason, SessionEndCauseView, SessionStateView, SessionView,
    SessionsListResult, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{RepoSnapshot, Scope, ScopeSnapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
use gitraptor_cli::model::{
    ConnEvent, ConnState, EngineMsg, GlobalView, Model, Msg, Requester, Size, Stamped,
};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

/// Monday 9:00 in UTC milliseconds, fixed: the tests never read the system clock.
const MON9: i64 = 1_760_000_000_000;
const H: i64 = 3_600_000;
const DAY: i64 = 24 * H;

// ------------------------------------------------------------------ harness

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn pinned(model: &mut Model, mode: ColorMode) {
    model.ui.theme = Theme::new(mode, Contrast::Normal, SymbolSet::Unicode);
}

fn ready(branch: &str) -> WorktreeStatus {
    let zero = || CommitCountView {
        count: 0,
        exact: true,
    };
    WorktreeStatus::Ready {
        head: HeadView::Branch {
            name: UntrustedName::new(branch),
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
    }
}

/// `(folder, branch)`; the folder `""` is the main worktree at `/w/shop`.
type Tree = (&'static str, &'static str);

fn path(folder: &str) -> String {
    if folder.is_empty() {
        "/w/shop".into()
    } else {
        format!("/w/shop/{folder}")
    }
}

fn worktrees(trees: &[Tree]) -> Vec<WorktreeView> {
    trees
        .iter()
        .map(|(folder, branch)| WorktreeView {
            last_activity_utc_ms: None,
            last_activity_in_gap: false,
            detached_at: None,
            path: Untrusted::new(path(folder)),
            main: folder.is_empty(),
            admin_name: None,
            status: ready(branch),
            operation: None,
            unmerged_paths: 0,
        })
        .collect()
}

fn snapshot(run: &str, trees: &[Tree]) -> Msg {
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: run.into(),
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

/// A session of the engine: `(id, agent, folder, state)` with its start; the end and its cause
/// are set with the builders below.
fn sess(id: &str, agent: &str, folder: &str, state: SessionStateView, started: i64) -> SessionView {
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
        state_since_utc_ms: started,
        utc_offset_s: 0,
        ended_utc_ms: None,
        end_cause: None,
    }
}

fn active(id: &str, agent: &str, folder: &str, started: i64) -> SessionView {
    sess(id, agent, folder, SessionStateView::Active, started)
}

fn inactive(id: &str, agent: &str, folder: &str, started: i64) -> SessionView {
    sess(id, agent, folder, SessionStateView::Inactive, started)
}

/// Ended at `ended` (published), having started at `started`.
fn ended(id: &str, agent: &str, folder: &str, started: i64, ended: i64) -> SessionView {
    let mut s = sess(id, agent, folder, SessionStateView::Ended, started);
    s.state_since_utc_ms = ended;
    s.ended_utc_ms = Some(ended);
    s.end_cause = Some(SessionEndCauseView::RegistrationWithdrawn);
    s
}

/// Ended without a published end time; the engine published the state at `since`.
fn ended_no_time(
    id: &str,
    agent: &str,
    folder: &str,
    started: i64,
    since: i64,
    cause: SessionEndCauseView,
) -> SessionView {
    let mut s = sess(id, agent, folder, SessionStateView::Ended, started);
    s.state_since_utc_ms = since;
    s.end_cause = Some(cause);
    s
}

fn list(sessions: Vec<SessionView>) -> Msg {
    engine(EngineMsg::Sessions {
        repo_id: "r1".into(),
        result: Box::new(SessionsListResult {
            detection_available: true,
            sessions,
        }),
    })
}

fn session_event(seq: u64, view: &SessionView) -> Msg {
    engine(EngineMsg::Event {
        scope: Scope::Repo {
            repo_id: "r1".into(),
        },
        scope_seq: seq,
        event: Box::new(Event {
            seq,
            kind: SESSION_STATE.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: serde_json::to_value(view).unwrap(),
        }),
    })
}

fn tick(model: &mut Model, now_ms: i64) {
    update(model, Msg::Tick { now_ms });
}

fn press(model: &mut Model, c: char) {
    update(
        model,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    );
}

/// A live model on "shop" with `trees`, then the session list, at `now_ms`.
fn fleet(
    lang: Lang,
    mode: ColorMode,
    trees: &[Tree],
    sessions: Vec<SessionView>,
    now_ms: i64,
    height: u16,
) -> Model {
    let mut model = Model::new(lang, Size { width: 140, height });
    pinned(&mut model, mode);
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
    update(&mut model, snapshot("run", trees));
    update(&mut model, list(sessions));
    tick(&mut model, now_ms);
    model
}

fn terminal(model: &Model) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(model.ui.size.width, model.ui.size.height)).unwrap();
    t.draw(|f| view(model, f)).unwrap();
    t
}

fn screen(model: &Model) -> Vec<String> {
    buffer_text(terminal(model).backend().buffer())
}

fn text(model: &Model) -> String {
    screen(model).join("\n")
}

/// The line of the row of the worktree on `branch` (a whole word).
fn row_at(lines: &[String], branch: &str) -> Option<usize> {
    lines
        .iter()
        .position(|l| l.split_whitespace().any(|w| w == branch))
}

/// The row of `branch` and the line under it (its sessions line, when it has one).
fn block(model: &Model, branch: &str) -> String {
    let lines = screen(model);
    let i = row_at(&lines, branch).unwrap_or_else(|| panic!("no row for {branch}:\n{lines:#?}"));
    format!(
        "{}\n{}",
        lines[i],
        lines.get(i + 1).map_or("", |l| l.as_str())
    )
}

/// The foreground of the first cell of `word`, searched cell by cell on the painted buffer.
fn fg_of(model: &Model, word: &str) -> Color {
    let t = terminal(model);
    let buf = t.backend().buffer();
    let width = buf.area.width;
    let n = word.chars().count() as u16;
    for y in 0..buf.area.height {
        for x in 0..width.saturating_sub(n) {
            let s: String = (0..n)
                .map(|d| buf[(x + d, y)].symbol().to_string())
                .collect();
            if s == word {
                return buf[(x, y)].fg;
            }
        }
    }
    panic!("`{word}` is not on the screen:\n{:#?}", buffer_text(buf));
}

// -------------------------------------------------------------- CKP002-ORDER

const ORDER_TREES: &[Tree] = &[
    ("", "main"),
    ("a-none", "br-none"),
    ("b-done", "br-done"),
    ("c-idle", "br-idle"),
    ("d-act", "br-d"),
    ("m-act", "br-m"),
];

fn order_sessions() -> Vec<SessionView> {
    vec![
        ended("s-done", "claude-4", "b-done", MON9 - 5 * H, MON9 - 2 * H),
        inactive("s-idle", "claude-3", "c-idle", MON9 - 4 * H),
        active("s-d", "claude-1", "d-act", MON9 - 3 * H),
        active("s-m", "claude-2", "m-act", MON9 - 3 * H),
    ]
}

#[test]
fn the_fleet_goes_main_active_inactive_ended_no_agent() {
    for lang in [Lang::En, Lang::Es] {
        let mut model = fleet(
            lang,
            ColorMode::NoColor,
            ORDER_TREES,
            order_sessions(),
            MON9,
            30,
        );
        let order = |model: &Model| -> Vec<String> {
            let lines = screen(model);
            let mut rows: Vec<(usize, &str)> = ORDER_TREES
                .iter()
                .map(|(_, b)| {
                    let at =
                        row_at(&lines, b).unwrap_or_else(|| panic!("no row for {b}:\n{lines:#?}"));
                    (at, *b)
                })
                .collect();
            rows.sort();
            rows.into_iter().map(|(_, b)| b.to_string()).collect()
        };
        let expected = ["main", "br-d", "br-m", "br-idle", "br-done", "br-none"];
        assert_eq!(order(&model), expected, "{lang:?}\n{}", text(&model));
        // A change of activity never moves a row inside its group: the later folder gets the
        // newest state, and still goes after the earlier one.
        let mut newer = active("s-m", "claude-2", "m-act", MON9 - 3 * H);
        newer.state_since_utc_ms = MON9 - 1_000;
        update(&mut model, session_event(2, &newer));
        tick(&mut model, MON9 + 1_000);
        assert_eq!(order(&model), expected, "{lang:?}\n{}", text(&model));
    }
}

// ------------------------------------------------------------- CKP002-ENDED

const OLD_TREES: &[Tree] = &[
    ("", "main"),
    ("wt-old", "feat-old"),
    ("wt-pagos", "feat-pagos"),
];

#[test]
fn an_ended_session_shows_when_it_ended_then_its_last_agent() {
    let end = MON9;
    let sessions = || {
        vec![
            ended("s4", "claude-4", "wt-old", end - 3 * H, end),
            active("s1", "claude-1", "wt-pagos", end - 3 * H),
        ]
    };
    let cases = [
        (
            Lang::En,
            "ended 2 h ago",
            "No agent",
            "last agent: claude-4 (ended 1 d ago)",
        ),
        (
            Lang::Es,
            "terminó hace 2 h",
            "Sin agente",
            "último agente: claude-4 (terminó hace 1 d)",
        ),
    ];
    for (lang, two_hours, none, last) in cases {
        let mut model = fleet(
            lang,
            ColorMode::NoColor,
            OLD_TREES,
            sessions(),
            end + 2 * H,
            30,
        );
        let old = block(&model, "feat-old");
        assert!(old.contains("claude-4 · wt-old"), "{lang:?}: {old}");
        assert!(old.contains(two_hours), "{lang:?}: {old}");
        // 24 h minus 1 ms: still visible.
        tick(&mut model, end + DAY - 1);
        assert!(
            block(&model, "feat-old").contains("claude-4 · wt-old"),
            "{lang:?}"
        );
        // 24 h exactly: hidden, the row says who was the last agent and how long ago.
        tick(&mut model, end + DAY);
        let old = block(&model, "feat-old");
        assert!(old.contains(none), "{lang:?}: {old}");
        assert!(old.contains(last), "{lang:?}: {old}");
        assert!(!old.contains("claude-4 · wt-old"), "{lang:?}: {old}");
        // `f` brings it back.
        press(&mut model, 'f');
        let old = block(&model, "feat-old");
        assert!(old.contains("claude-4 · wt-old"), "{lang:?}: {old}");
        assert!(
            old.contains(
                &last
                    .replace("last agent: claude-4 (", "")
                    .replace("último agente: claude-4 (", "")
                    .replace(')', "")
            ),
            "{lang:?}: {old}"
        );
    }
    // The name of an ended session is dim: not the color of a present agent.
    let model = fleet(
        Lang::En,
        ColorMode::TrueColor,
        OLD_TREES,
        sessions(),
        end + 2 * H,
        30,
    );
    assert_ne!(fg_of(&model, "claude-4"), fg_of(&model, "claude-1"));
}

// -------------------------------------------------------- CKP002-UNOBSERVED

const GAP_TREES: &[Tree] = &[
    ("", "main"),
    ("wt-gap", "feat-gap"),
    ("wt-other", "feat-other"),
];

#[test]
fn a_session_ended_unobserved_says_so() {
    let sessions = || {
        vec![
            ended_no_time(
                "s4",
                "claude-4",
                "wt-gap",
                MON9 - 6 * H,
                MON9 - 2 * H,
                SessionEndCauseView::EndedDuringGap,
            ),
            ended_no_time(
                "s5",
                "claude-5",
                "wt-other",
                MON9 - 6 * H,
                MON9 - 2 * H,
                SessionEndCauseView::ProcessGone,
            ),
        ]
    };
    let cases = [
        (
            Lang::En,
            "ended unobserved; detected 2 h ago",
            "end time not available",
            "last agent: claude-4 (ended unobserved; detected 1 d ago)",
        ),
        (
            Lang::Es,
            "terminó sin observar; detectado hace 2 h",
            "hora de fin no disponible",
            "último agente: claude-4 (terminó sin observar; detectado hace 1 d)",
        ),
    ];
    for (lang, unobserved, unavailable, last) in cases {
        let mut model = fleet(lang, ColorMode::NoColor, GAP_TREES, sessions(), MON9, 30);
        let gap = block(&model, "feat-gap");
        assert!(gap.contains(unobserved), "{lang:?}: {gap}");
        let other = block(&model, "feat-other");
        assert!(other.contains(unavailable), "{lang:?}: {other}");
        assert!(!other.contains(unobserved), "{lang:?}: {other}");
        // The window counts from when the engine published it: 24 h later it is hidden.
        tick(&mut model, MON9 - 2 * H + DAY);
        let gap = block(&model, "feat-gap");
        assert!(gap.contains(last), "{lang:?}: {gap}");
    }
}

// ----------------------------------------------------------- CKP002-REPLACE

#[test]
fn a_new_session_replaces_the_ended_one() {
    for lang in [Lang::En, Lang::Es] {
        let (two_hours, last) = match lang {
            Lang::En => ("ended 2 h ago", "last agent"),
            Lang::Es => ("terminó hace 2 h", "último agente"),
        };
        let mut model = fleet(
            lang,
            ColorMode::NoColor,
            OLD_TREES,
            vec![ended(
                "s4",
                "claude-4",
                "wt-old",
                MON9 - 5 * H,
                MON9 - 2 * H,
            )],
            MON9,
            30,
        );
        assert!(
            text(&model).contains(two_hours),
            "{lang:?}\n{}",
            text(&model)
        );
        // The same agent works again: a new session, started after the end.
        let again = active("s5", "claude-4", "wt-old", MON9 - H);
        update(&mut model, session_event(2, &again));
        tick(&mut model, MON9 + 1_000);
        let old = block(&model, "feat-old");
        assert!(old.contains("● claude-4 · wt-old"), "{lang:?}: {old}");
        let all = text(&model);
        assert!(!all.contains(two_hours), "{lang:?}\n{all}");
        assert!(!all.contains(last), "{lang:?}\n{all}");
    }
}

// ------------------------------------------------------------ CKP002-SHARED

const PAGOS_TREES: &[Tree] = &[("", "main"), ("wt-pagos", "feat-pagos")];

#[test]
fn a_shared_worktree_shows_all_its_sessions() {
    // claude-3 ended one hour ago, after claude-1 and claude-2 started.
    let sessions = vec![
        active("s1", "claude-1", "wt-pagos", MON9 - 5 * H),
        inactive("s2", "claude-2", "wt-pagos", MON9 - 4 * H),
        ended("s3", "claude-3", "wt-pagos", MON9 - 3 * H, MON9 - H),
    ];
    for (lang, shared) in [(Lang::En, "shared with"), (Lang::Es, "compartido con")] {
        let model = fleet(
            lang,
            ColorMode::NoColor,
            PAGOS_TREES,
            sessions.clone(),
            MON9,
            30,
        );
        let pagos = block(&model, "feat-pagos");
        assert!(pagos.contains(shared), "{lang:?}: {pagos}");
        for agent in ["claude-1", "claude-2", "claude-3"] {
            assert!(pagos.contains(agent), "{lang:?}: {agent} in {pagos}");
        }
    }
}

// --------------------------------------------------------- CKP002-NOTSHARED

#[test]
fn a_present_and_an_ended_session_are_not_shared() {
    // claude-2 ended one hour ago, after claude-1 started.
    let sessions = vec![
        inactive("s1", "claude-1", "wt-pagos", MON9 - 5 * H),
        ended("s2", "claude-2", "wt-pagos", MON9 - 4 * H, MON9 - H),
    ];
    for (lang, shared, ago) in [
        (Lang::En, "shared", "ended 1 h ago"),
        (Lang::Es, "compartido", "terminó hace 1 h"),
    ] {
        let model = fleet(
            lang,
            ColorMode::NoColor,
            PAGOS_TREES,
            sessions.clone(),
            MON9,
            30,
        );
        let pagos = block(&model, "feat-pagos");
        assert!(pagos.contains("claude-1"), "{lang:?}: {pagos}");
        assert!(pagos.contains("claude-2"), "{lang:?}: {pagos}");
        assert!(pagos.contains(ago), "{lang:?}: {pagos}");
        assert!(!pagos.contains(shared), "{lang:?}: {pagos}");
    }
}

// ------------------------------------------------------------ CKP002-NINE

/// The folder of each agent. Sorted by name, claude-9's folder is the fifth: by row position it
/// would not share claude-1's color; by start order (a slot per session) it does.
const NINE_FOLDERS: [(&str, u8); 9] = [
    ("wt-a", 1),
    ("wt-b", 2),
    ("wt-c", 3),
    ("wt-d", 4),
    ("wt-e", 9),
    ("wt-f", 5),
    ("wt-g", 6),
    ("wt-h", 7),
    ("wt-i", 8),
];

fn nine_trees() -> Vec<Tree> {
    let mut trees: Vec<Tree> = vec![("", "main")];
    let branches = [
        "br-1", "br-2", "br-3", "br-4", "br-9", "br-5", "br-6", "br-7", "br-8",
    ];
    for ((folder, _), branch) in NINE_FOLDERS.iter().zip(branches) {
        trees.push((folder, branch));
    }
    trees
}

fn nine_sessions() -> Vec<SessionView> {
    NINE_FOLDERS
        .iter()
        .map(|(folder, i)| {
            active(
                &format!("s{i}"),
                &format!("claude-{i}"),
                folder,
                MON9 - 100 * H + i64::from(*i) * 1_000,
            )
        })
        .collect()
}

#[test]
fn nine_agents_the_ninth_shares_the_first_color_and_keeps_its_name() {
    let model = fleet(
        Lang::En,
        ColorMode::TrueColor,
        &nine_trees(),
        nine_sessions(),
        MON9,
        40,
    );
    assert_eq!(fg_of(&model, "claude-9"), fg_of(&model, "claude-1"));
    assert_ne!(fg_of(&model, "claude-1"), fg_of(&model, "claude-2"));
    // Its name and its state symbol are on its row.
    let ninth = block(&model, "br-9");
    assert!(ninth.contains("● claude-9"), "{ninth}");
}

// ----------------------------------------------------------- CKP002-LAGGED

const THREE: &[Tree] = &[
    ("", "main"),
    ("wt-1", "br-1"),
    ("wt-2", "br-2"),
    ("wt-3", "br-3"),
];
const THREE_REVERSED: &[Tree] = &[
    ("", "main"),
    ("wt-3", "br-3"),
    ("wt-2", "br-2"),
    ("wt-1", "br-1"),
];

const FOUR_REVERSED: &[Tree] = &[
    ("", "main"),
    ("wt-4", "br-4"),
    ("wt-3", "br-3"),
    ("wt-2", "br-2"),
    ("wt-1", "br-1"),
];

fn three_sessions() -> Vec<SessionView> {
    (1..=3)
        .map(|i| {
            active(
                &format!("s{i}"),
                &format!("claude-{i}"),
                &format!("wt-{i}"),
                MON9 - 10 * H + i64::from(i) * 1_000,
            )
        })
        .collect()
}

/// `claude-2` and `claude-3` present, in a different order, and `claude-1` ended an hour ago.
fn oldest_ended() -> Vec<SessionView> {
    let mut sessions = three_sessions();
    sessions[0] = ended("s1", "claude-1", "wt-1", MON9 - 10 * H + 1_000, MON9 - H);
    sessions.reverse();
    sessions
}

fn colors(model: &Model) -> Vec<Color> {
    ["claude-2", "claude-3"]
        .iter()
        .map(|a| fg_of(model, a))
        .collect()
}

#[test]
fn each_agent_keeps_its_color_after_lagged_and_a_reconnection() {
    let mut model = fleet(
        Lang::En,
        ColorMode::TrueColor,
        THREE,
        three_sessions(),
        MON9,
        30,
    );
    let before = colors(&model);
    assert_ne!(before[0], before[1]);
    assert_ne!(fg_of(&model, "claude-1"), before[0]);
    update(
        &mut model,
        engine(EngineMsg::Lagged {
            reason: ResyncReason::SlowConsumer,
        }),
    );
    update(
        &mut model,
        Msg::Conn(ConnEvent::State(ConnState::Reconnecting { attempt: 1 })),
    );
    update(&mut model, Msg::Conn(ConnEvent::State(ConnState::Live)));
    // The new snapshot publishes the worktrees in another order; the list brings the sessions
    // in another order too.
    update(&mut model, snapshot("run-2", THREE_REVERSED));
    // In that list the oldest session has already ended: the others keep their color.
    update(&mut model, list(oldest_ended()));
    tick(&mut model, MON9 + 1_000);
    assert_eq!(colors(&model), before);
}

// --------------------------------------------------------- CKP002-FAILEDLIST

#[test]
fn a_failed_sessions_list_never_frees_a_color() {
    let mut model = fleet(
        Lang::En,
        ColorMode::TrueColor,
        &[THREE, &[("wt-4", "br-4")]].concat(),
        three_sessions(),
        MON9,
        30,
    );
    let before = colors(&model);
    // A new snapshot, a loose session.state of claude-2 and a new agent, and no list: the list
    // failed (it never reaches `update`).
    update(&mut model, snapshot("run-2", FOUR_REVERSED));
    let mut loose = active("s2", "claude-2", "wt-2", MON9 - 10 * H + 2_000);
    loose.state_since_utc_ms = MON9;
    update(&mut model, session_event(2, &loose));
    let newcomer = active("s4", "claude-4", "wt-4", MON9 - H);
    update(&mut model, session_event(3, &newcomer));
    // The list arrives later.
    update(&mut model, list(oldest_ended()));
    tick(&mut model, MON9 + 1_000);
    assert_eq!(colors(&model), before);
    // The newcomer took a free slot: none of the others' colors.
    let fourth = fg_of(&model, "claude-4");
    assert!(!before.contains(&fourth), "{fourth:?} in {before:?}");
}

// ------------------------------------------------------------- CKP002-HINT

const HINT_TREES: &[Tree] = &[
    ("", "main"),
    ("wt-1", "br-1"),
    ("wt-2", "br-2"),
    ("wt-3", "br-3"),
    ("wt-4", "br-4"),
];

/// Three worktrees whose only session ended 30 hours ago (hidden), and one with a present agent.
fn hidden_sessions() -> Vec<SessionView> {
    let end = MON9 - 30 * H;
    vec![
        ended("e1", "claude-1", "wt-1", end - H, end),
        ended("e2", "claude-2", "wt-2", end - H, end),
        ended("e3", "claude-3", "wt-3", end - H, end),
        active("a4", "claude-4", "wt-4", MON9 - H),
    ]
}

/// The lines of the help panel between the title of the fleet group and the next group title.
fn fleet_group(model: &Model, title: &str, next: &str) -> Vec<String> {
    let lines = screen(model);
    let from = lines
        .iter()
        .position(|l| l.contains(title))
        .unwrap_or_else(|| panic!("no `{title}` group:\n{lines:#?}"));
    lines[from + 1..]
        .iter()
        .take_while(|l| !l.contains(next))
        .cloned()
        .collect()
}

#[test]
fn the_show_ended_hint_counts_what_is_hidden() {
    let cases = [
        (
            Lang::En,
            "f show ended (3)",
            "f hide ended",
            "show ended",
            "In the fleet",
            "Always",
        ),
        (
            Lang::Es,
            "f ver terminadas (3)",
            "f ocultar terminadas",
            "ver terminadas",
            "En la flota",
            "Siempre",
        ),
    ];
    for (lang, show, hide, label, group, next) in cases {
        let mut model = fleet(
            lang,
            ColorMode::NoColor,
            HINT_TREES,
            hidden_sessions(),
            MON9,
            30,
        );
        assert!(text(&model).contains(show), "{lang:?}\n{}", text(&model));
        // The filter on: no longer hidden, the hint offers to hide them again.
        press(&mut model, 'f');
        let on = text(&model);
        assert!(on.contains(hide), "{lang:?}\n{on}");
        assert!(!on.contains(show), "{lang:?}\n{on}");
        // Off again: the count is back.
        press(&mut model, 'f');
        assert!(text(&model).contains(show), "{lang:?}\n{}", text(&model));

        // Nothing hidden and the filter off: no hint on the bar.
        let none = fleet(
            lang,
            ColorMode::NoColor,
            HINT_TREES,
            vec![active("a4", "claude-4", "wt-4", MON9 - H)],
            MON9,
            30,
        );
        let bar = text(&none);
        assert!(!bar.contains(label), "{lang:?}\n{bar}");
        // The `?` help lists `f` in the fleet group, always (here with nothing hidden).
        let mut help = none;
        press(&mut help, '?');
        let lines = fleet_group(&help, group, next);
        assert!(
            lines.iter().any(|l| l.contains(label) && l.contains('f')),
            "{lang:?}: no `f {label}` in the fleet group: {lines:#?}"
        );
    }
}
