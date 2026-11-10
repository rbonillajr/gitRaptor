//! US-CKP-022 end to end: each Gherkin scenario against the real daemon (`raptor`, debug build,
//! temporary profile) over a temporary repo built by the "intact repo" harness. Never this repo
//! nor the real profile (NFR-01). The TUI is the real `App` on `TestBackend`, in Spanish (the
//! texts of the story). The arrange comes from `live_fleet.rs` and `tui_diff.rs`.
//!
//! No fixed waits: every state is awaited on the painted screen with a deadline.
//!
//! macOS only; Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client::engine::EngineConnector;
use gitraptor_cli::client::{self, ClientThread};
use gitraptor_cli::model::{Model, Msg, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue::{self, Outlet};
use gitraptor_cli::tui::app::App;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_core::client::{ClientOptions, Launcher};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const DEADLINE: Duration = Duration::from_secs(30);
/// The fewest rows the list keeps (`layout::LIST_MIN`, DS-US-CKP-005 D2).
const LIST_MIN: usize = 10;

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// "shop" of § 9.3: `main` (checked out in the main worktree) with `feat-pagos` (72 commits from
/// its merge-base, `claude-1`) and `feat-login` (3 commits, `claude-2`). `extra` more worktrees
/// without commits make a long fleet.
struct Shop {
    f: Fixture,
    pagos: PathBuf,
    login: PathBuf,
    /// The commit `main` stays at: the merge-base of both lanes.
    fork: String,
}

fn confirm_base(f: &Fixture, base: &str) {
    let (mut profile, _) = Profile::open(ProfileDirs::under_root(&f.profile)).unwrap();
    let git_dir = std::fs::canonicalize(f.repo.join(".git")).unwrap();
    let (entry, _) = profile.add_repo(&git_dir, None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    store
        .set_confirmed_team_baseline(&Confirmed {
            base_branch: RefName::new(base).unwrap(),
            floor: ConfirmedFloor::Absent,
        })
        .unwrap();
}

impl Shop {
    fn new(base: &str, extra: usize) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("README.md", "shop\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "shop"]);
        let fork = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
        f.git(&["branch", "feat-pagos"]);
        f.git(&["branch", "feat-login"]);
        let pagos = f
            .add_worktree("pagos", "feat-pagos")
            .canonicalize()
            .unwrap();
        let login = f
            .add_worktree("login", "feat-login")
            .canonicalize()
            .unwrap();
        for n in 1..=72 {
            f.git_in(
                &pagos,
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    &format!("pagos {n:02}"),
                ],
            );
        }
        for n in 1..=3 {
            f.git_in(
                &login,
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    &format!("login {n:02}"),
                ],
            );
        }
        for n in 0..extra {
            let name = format!("feat-x{n:02}");
            f.git(&["branch", &name]);
            f.add_worktree(&format!("x{n:02}"), &name);
        }
        confirm_base(&f, base);
        Self {
            f,
            pagos,
            login,
            fork,
        }
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "es_ES.UTF-8".into()),
        ]
    }

    /// The developer, from their own terminal.
    fn developer(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        let out = Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        out
    }

    /// Observed by the engine, with `claude-1` in `feat-pagos` and `claude-2` in `feat-login`.
    fn observed(&self) {
        self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        for (name, worktree) in [("claude-1", &self.pagos), ("claude-2", &self.login)] {
            self.developer(&[
                "agent",
                "register",
                name,
                "--worktree",
                worktree.to_str().unwrap(),
            ]);
        }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    /// The first 7 characters of the commits of `branch` over the fork, newest first.
    fn short_hashes(&self, branch: &str) -> Vec<String> {
        self.f
            .git(&["rev-list", &format!("{}..{branch}", self.fork)])
            .lines()
            .map(|l| l[..7].to_owned())
            .collect()
    }
}

impl Drop for Shop {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// A headless TUI on the running daemon; it never starts one.
struct Tui {
    app: App<TestBackend>,
    input: Outlet,
    channel: Option<ClientThread>,
}

impl Tui {
    fn open(lang: Lang, dirs: ProfileDirs, cwd: &Path, size: Size) -> Self {
        let mut options = ClientOptions::new(dirs, ClientKind::Cli);
        options.launcher = Launcher::Never;
        let (inbox, input, engine) = queue::inbox();
        let model = Model::new(lang, size);
        let mut app = App::new(
            Terminal::new(TestBackend::new(size.width, size.height)).unwrap(),
            model,
            inbox,
        );
        let channel = client::spawn(
            EngineConnector::new(options.connect().unwrap(), Box::new(options.launcher())),
            Some(cwd.to_owned()),
            engine,
        );
        app.attach(channel.cmds.clone());
        Self {
            app,
            input,
            channel: Some(channel),
        }
    }

    fn screen(&self) -> Vec<String> {
        buffer_text(self.app.terminal().backend().buffer())
    }

    fn press(&self, code: KeyCode) {
        self.input
            .send(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
            .unwrap();
    }

    /// Steps the TUI until the painted screen satisfies `ok`; returns it.
    fn until(&mut self, what: &str, ok: impl Fn(&[String]) -> bool) -> Vec<String> {
        let start = Instant::now();
        loop {
            self.app.step(Duration::from_millis(20)).unwrap();
            let screen = self.screen();
            if !self.app.model.dirty && ok(&screen) {
                return screen;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what} ({:?}):\n{}",
                self.app.model.conn,
                screen.join("\n")
            );
        }
    }

    /// The fleet of `shop`, live, with both agents in their rows.
    fn fleet(shop: &Shop, size: Size) -> Self {
        let mut tui = Self::open(Lang::Es, shop.dirs(), &shop.f.repo, size);
        tui.until("claude-1 and claude-2 in their rows, live", |s| {
            live(s)
                && row(s, "feat-pagos").is_some_and(|r| r.contains("claude-"))
                && row(s, "feat-login").is_some_and(|r| r.contains("claude-"))
        });
        tui
    }

    /// The colors painted in the cells of a screen line that are not blank.
    fn colors_of(&self, line: usize) -> HashSet<Color> {
        let buffer = self.app.terminal().backend().buffer();
        let width = buffer.area.width as usize;
        buffer.content()[line * width..(line + 1) * width]
            .iter()
            .filter(|c| !c.symbol().trim().is_empty() && c.fg != Color::Reset)
            .map(|c| c.fg)
            .collect()
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            channel.shutdown();
        }
    }
}

fn big() -> Size {
    // Tall enough to hold the whole view: it does not depend on the size (D1).
    Size {
        width: 120,
        height: 100,
    }
}

fn live(screen: &[String]) -> bool {
    screen
        .first()
        .is_some_and(|h| h.contains("en vivo") || h.contains("live"))
}

fn any(screen: &[String], text: &str) -> bool {
    screen.iter().any(|l| l.contains(text))
}

fn row<'s>(screen: &'s [String], branch: &str) -> Option<&'s str> {
    screen
        .iter()
        .find(|l| l.split_whitespace().any(|w| w == branch))
        .map(String::as_str)
}

/// The header line of the lane of `branch` in the view: the one that names the branch and counts
/// its commits from the merge-base.
fn header(screen: &[String], branch: &str) -> usize {
    screen
        .iter()
        .position(|l| l.contains(branch) && l.contains("commits desde"))
        .unwrap_or_else(|| panic!("no lane header for {branch}:\n{}", screen.join("\n")))
}

/// The view open and answered: `g`, then waits for both lanes.
fn open_view(tui: &mut Tui) -> Vec<String> {
    tui.press(KeyCode::Char('g'));
    tui.until("the graph view with both lanes", |s| {
        s.iter()
            .any(|l| l.contains("feat-pagos") && l.contains("commits desde"))
            && s.iter()
                .any(|l| l.contains("feat-login") && l.contains("commits desde"))
    })
}

// ------------------------------------------------------------ Escenario 1

/// Un carril por worktree, desde su merge-base, con el nombre y el color de su agente.
#[test]
fn one_lane_per_worktree_from_its_merge_base() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new("main", 0);
    shop.observed();
    let mut tui = Tui::fleet(&shop, big());
    // The colors of the fleet rows, before the view covers them.
    let fleet = tui.screen();
    let fleet_pagos = tui.colors_of(
        fleet
            .iter()
            .position(|l| row(std::slice::from_ref(l), "feat-pagos").is_some())
            .unwrap(),
    );
    let fleet_login = tui.colors_of(
        fleet
            .iter()
            .position(|l| row(std::slice::from_ref(l), "feat-login").is_some())
            .unwrap(),
    );

    let screen = open_view(&mut tui);
    let pagos = header(&screen, "feat-pagos");
    let login = header(&screen, "feat-login");
    assert!(
        screen[pagos].contains("claude-1") && screen[pagos].contains("72 commits"),
        "the lane of feat-pagos names its agent and counts 72 commits: {}",
        screen[pagos]
    );
    assert!(
        screen[pagos].contains(&format!("desde {}", &shop.fork[..7])),
        "the lane starts at its merge-base with main ({}): {}",
        &shop.fork[..7],
        screen[pagos]
    );
    assert!(
        screen[login].contains("claude-2") && screen[login].contains("3 commits"),
        "{}",
        screen[login]
    );
    assert!(
        screen[login].contains(&format!("desde {}", &shop.fork[..7])),
        "{}",
        screen[login]
    );
    // The trunk is not a lane.
    assert!(
        !screen
            .iter()
            .any(|l| l.contains("commits desde") && l.split_whitespace().any(|w| w == "main")),
        "main has a lane: {screen:#?}"
    );
    assert_eq!(
        screen
            .iter()
            .filter(|l| l.contains("commits desde"))
            .count(),
        2,
        "one lane per worktree but the trunk"
    );

    // Each lane wears the color of its agent: the one of its fleet row, and not the other's.
    let (head_pagos, head_login) = (tui.colors_of(pagos), tui.colors_of(login));
    let own_pagos: HashSet<_> = head_pagos.difference(&head_login).copied().collect();
    let own_login: HashSet<_> = head_login.difference(&head_pagos).copied().collect();
    assert!(
        !own_pagos.is_empty() && !own_login.is_empty(),
        "the two lanes must be painted in different colors"
    );
    assert!(
        own_pagos.intersection(&fleet_pagos).next().is_some(),
        "the lane of claude-1 does not use the color of its fleet row"
    );
    assert!(
        own_login.intersection(&fleet_login).next().is_some(),
        "the lane of claude-2 does not use the color of its fleet row"
    );
}

// ------------------------------------------------------------ Escenario 2

/// Los 50 más recientes y "22 más" colapsados.
#[test]
fn a_lane_shows_50_commits_and_22_more() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new("main", 0);
    shop.observed();
    let mut tui = Tui::fleet(&shop, big());
    let screen = open_view(&mut tui);
    let start = header(&screen, "feat-pagos");
    let end = screen
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.contains("commits desde"))
        .map_or(screen.len(), |(i, _)| i);
    let lane = &screen[start + 1..end.max(start + 1)];
    // The lane of feat-pagos may come before or after the other one.
    let lane: Vec<&String> = if end > start {
        lane.iter().collect()
    } else {
        screen[start + 1..].iter().collect()
    };

    // 72 commits, newest first: `pagos 72` down to `pagos 23` are the 50 that are shown.
    let hashes = shop.short_hashes("feat-pagos");
    assert_eq!(hashes.len(), 72);
    let mut shown = 0;
    for (i, hash) in hashes.iter().enumerate() {
        let n = 72 - i;
        let subject = format!("pagos {n:02}");
        let line = lane.iter().find(|l| l.contains(&subject));
        if n >= 23 {
            let line = line.unwrap_or_else(|| panic!("commit `{subject}` is not in the lane"));
            assert!(
                line.contains(hash.as_str()),
                "the row of `{subject}` lacks its short hash {hash}: {line}"
            );
            shown += 1;
        } else {
            assert!(
                line.is_none(),
                "`{subject}` is older than the 50 shown: {line:?}"
            );
        }
    }
    assert_eq!(shown, 50, "the window is the 50 most recent commits");
    let more = lane
        .iter()
        .position(|l| l.contains("22 más"))
        .unwrap_or_else(|| panic!("the lane lacks \"22 más\": {lane:#?}"));
    let last_commit = lane
        .iter()
        .rposition(|l| l.contains("pagos 23"))
        .expect("pagos 23");
    assert!(more > last_commit, "\"22 más\" closes the lane");
}

// ------------------------------------------------------------ Escenario 3

/// Cada commit dice "sin atribuir" y el carril conserva el color de su sesión.
#[test]
fn every_commit_is_unattributed_and_keeps_the_lane_color() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new("main", 0);
    shop.observed();
    let mut tui = Tui::fleet(&shop, big());
    let screen = open_view(&mut tui);
    let login = header(&screen, "feat-login");
    let pagos = header(&screen, "feat-pagos");
    let head_login = tui.colors_of(login);
    let head_pagos = tui.colors_of(pagos);

    let commit_lines: Vec<usize> = screen
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("pagos ") || l.contains("login "))
        .filter(|(_, l)| !l.contains("commits desde"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        commit_lines.len(),
        53,
        "50 commits of feat-pagos and 3 of feat-login are listed"
    );
    for i in &commit_lines {
        assert!(
            screen[*i].contains("sin atribuir"),
            "the commit does not say \"sin atribuir\": {}",
            screen[*i]
        );
        // The node of the commit is painted in the color of its lane, never in another's.
        let colors = tui.colors_of(*i);
        let (own, other) = if screen[*i].contains("login ") {
            (&head_login, &head_pagos)
        } else {
            (&head_pagos, &head_login)
        };
        let lane_only: HashSet<_> = own.difference(other).copied().collect();
        assert!(
            lane_only.intersection(&colors).next().is_some(),
            "the node of `{}` does not carry the color of its lane",
            screen[*i]
        );
    }
}

// ------------------------------------------------------------ Escenario 4

/// "develop" como base y sin esa rama: ningún carril y el motivo, en la vista y en el panel.
#[test]
fn missing_base_shows_no_lanes_and_says_why() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new("develop", 0);
    shop.observed();
    let mut tui = Tui::fleet(&shop, big());
    let message = "Rama base develop no encontrada: no calculable";
    // The panel of the fleet, fed by the stream.
    let panel = tui.until("the message in the panel of the fleet", |s| any(s, message));
    assert!(
        !panel.iter().any(|l| l.contains("commits desde")),
        "a lane is drawn without a base"
    );
    // The view, fed by graph.lanes.
    tui.press(KeyCode::Char('g'));
    let view = tui.until("the graph view with the message", |s| {
        any(s, "grafo") && any(s, message) && !any(s, "claude-1")
    });
    assert!(
        !view.iter().any(|l| l.contains("commits desde")),
        "the view draws a lane without a base: {view:#?}"
    );
    assert!(
        !view
            .iter()
            .any(|l| l.contains("pagos ") || l.contains("login ")),
        "no commit is listed against another branch: {view:#?}"
    );
}

// ------------------------------------------------------------ Escenario 5

/// A 80×24 con la flota larga el grafo cede: la lista conserva `LIST_MIN`, `g` está en la barra
/// y abre la vista a pantalla completa.
#[test]
fn the_graph_yields_space_first_at_80x24() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // main + feat-pagos + feat-login + 9 more: 12 rows in the fleet.
    let shop = Shop::new("main", 9);
    shop.observed();
    let size = Size {
        width: 80,
        height: 24,
    };
    let mut tui = Tui::fleet(&shop, size);
    let screen = tui.screen();
    // A fleet row ends in "↑N ↓M". The list region keeps LIST_MIN rows: minus its two borders
    // and its column header, that is LIST_MIN - 3 rows of the fleet.
    let rows = screen
        .iter()
        .filter(|l| {
            let mut chars = l.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '↑' && chars.peek().is_some_and(char::is_ascii_digit) {
                    return true;
                }
            }
            false
        })
        .count();
    assert!(
        rows >= LIST_MIN - 3,
        "the list keeps its minimum ({LIST_MIN} rows) at 80x24 with a long fleet: {rows} fleet rows\n{}",
        screen.join("\n")
    );
    let bar = screen.last().unwrap();
    assert!(
        bar.contains("g grafo"),
        "the key bar names `g`: {bar:?}\n{}",
        screen.join("\n")
    );

    // `g` opens the full-screen view whatever the size.
    tui.press(KeyCode::Char('g'));
    let view = tui.until("the graph view at 80x24", |s| {
        s.iter().any(|l| l.contains("commits desde"))
    });
    assert!(
        view.iter()
            .any(|l| l.contains("grafo de") && l.contains("sobre main")),
        "the view has its title: {view:#?}"
    );
    assert!(
        !view.iter().any(|l| l.contains("feat-x"))
            || view.iter().any(|l| l.contains("commits desde")),
        "the view replaces the fleet"
    );
    // `Esc` goes back to the fleet.
    tui.press(KeyCode::Esc);
    tui.until("the fleet again", |s| {
        live(s) && !s.iter().any(|l| l.contains("commits desde"))
    });
}
