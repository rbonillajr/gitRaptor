//! US-CKP-012 end to end: each Gherkin scenario against the real daemon (`raptor`, debug build,
//! temporary profile) over a temporary repo built by the "intact repo" harness. Never this repo
//! nor the real profile (NFR-01). The TUI is the real `App` on `TestBackend`, in Spanish (the
//! texts of the story). The arrange comes from `live_fleet.rs`.
//!
//! No fixed waits: every state is awaited on the painted screen with a deadline.
//!
//! macOS only; Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

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
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint;
use gitraptor_testkit::fixture::git_from_path;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const DEADLINE: Duration = Duration::from_secs(20);

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// What `feat-pagos` changes against `main`.
#[derive(Clone, Copy)]
enum Work {
    /// `pay.txt` added, `logo.png` changed, `notes.txt` added: 3 files that fit on a screen.
    Small,
    /// `pay.txt` added, `logo.png` changed, `big.txt` with its 3.000 lines changed.
    Huge,
    /// `pay.txt` added, `logo.png` changed, and a file whose path and lines carry control
    /// sequences, tabs and CRLF.
    Evil,
}

/// "shop": `main` with `README.md`, `logo.png` and `big.txt`; `feat-pagos` with its worktree,
/// 3 commits from its merge-base with `main` and `README.md` modified without committing.
struct Shop {
    f: Fixture,
    pagos: PathBuf,
}

fn png(version: u8) -> Vec<u8> {
    vec![0x89, b'P', b'N', b'G', 0, 0, version, 0xff]
}

/// OSC 52 (clipboard), BEL, RLO and U+2028 in a path.
const EVIL_PATH: &str = "evil\u{1b}]52;c;aGk=\u{7}\u{202e}gpj\u{2028}.txt";
/// The same in the lines, plus a tab and Windows line ends.
const EVIL_TEXT: &str =
    "col1\tcol2\r\n\u{1b}]52;c;aGk=\u{7}\nrlo \u{202e}abc\nsep\u{2028}end\nesc \u{1b}[2Jgone\n";

impl Shop {
    fn new(work: Work) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("README.md", "shop\n");
        std::fs::write(f.repo.join("logo.png"), png(1)).unwrap();
        let big: String = (1..=3000).map(|i| format!("old line {i}\n")).collect();
        f.write("big.txt", &big);
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "shop"]);
        f.git(&["branch", "feat-pagos"]);
        let pagos = f
            .add_worktree("pagos", "feat-pagos")
            .canonicalize()
            .unwrap();
        let commit = |msg: &str| {
            f.git_in(&pagos, &["add", "."]);
            f.git_in(&pagos, &["commit", "-q", "-m", msg]);
        };
        std::fs::write(pagos.join("pay.txt"), "pay\n").unwrap();
        commit("pay");
        std::fs::write(pagos.join("logo.png"), png(2)).unwrap();
        commit("logo");
        match work {
            Work::Small => std::fs::write(pagos.join("notes.txt"), "notes\n").unwrap(),
            Work::Evil => {
                std::fs::write(pagos.join(EVIL_PATH), EVIL_TEXT).unwrap();
            }
            Work::Huge => {
                let new: String = (1..=3000).map(|i| format!("new line {i}\n")).collect();
                std::fs::write(pagos.join("big.txt"), new).unwrap();
            }
        }
        commit("third");
        std::fs::write(pagos.join("README.md"), "shop, edited\n").unwrap();
        Self { f, pagos }
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

    /// Observed by the engine, with `claude-1` registered in `feat-pagos`.
    fn observed(&self) {
        self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        self.agent("claude-1");
    }

    fn agent(&self, name: &str) {
        self.developer(&[
            "agent",
            "register",
            name,
            "--worktree",
            self.pagos.to_str().unwrap(),
        ]);
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
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
    fn open(dirs: ProfileDirs, cwd: &Path) -> Self {
        Self::open_in(Lang::Es, dirs, cwd)
    }

    fn open_in(lang: Lang, dirs: ProfileDirs, cwd: &Path) -> Self {
        let mut options = ClientOptions::new(dirs, ClientKind::Cli);
        options.launcher = Launcher::Never;
        let (inbox, input, engine) = queue::inbox();
        let size = Size {
            width: 120,
            height: 30,
        };
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

    /// The fleet of `shop`, live, with `claude-1` in `feat-pagos` (main is the first row, so the
    /// cursor, which starts on the first row, goes down once to reach `feat-pagos`).
    fn fleet(shop: &Shop) -> Self {
        Self::fleet_in(Lang::Es, shop)
    }

    fn fleet_in(lang: Lang, shop: &Shop) -> Self {
        let mut tui = Self::open_in(lang, shop.dirs(), &shop.f.repo);
        tui.until("claude-1 in feat-pagos, live", |s| {
            live(s) && row(s, "feat-pagos").is_some_and(|r| r.contains("claude-"))
        });
        tui
    }

    /// `j` then `d` on the fleet: the diff of `feat-pagos`.
    fn ask_for_the_diff(&mut self) {
        self.press(KeyCode::Char('j'));
        self.press(KeyCode::Char('d'));
    }

    fn until_the_diff(&mut self) -> Vec<String> {
        self.until("the diff view (entraría al merge …)", |s| {
            any(s, "entraría al merge")
        })
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            channel.shutdown();
        }
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

// ------------------------------------------------------------ Escenario 1

/// Dos secciones, commiteado y sin commitear.
#[test]
fn two_sections_committed_and_uncommitted() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Small);
    shop.observed();
    let mut tui = Tui::fleet(&shop);
    tui.ask_for_the_diff();
    let screen = tui.until_the_diff();
    let at = |text: &str| {
        screen
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("the view lacks \"{text}\":\n{}", screen.join("\n")))
    };
    let committed = at("entraría al merge (3 archivos)");
    let uncommitted = at("sin commitear (1 archivo)");
    assert!(committed < uncommitted, "{screen:#?}");
    assert!(
        !any(&screen, "claude-1"),
        "the view names an agent: {screen:#?}"
    );
}

// ------------------------------------------------------------ Escenario 2

/// Un binario sin contenido y un archivo enorme truncado con aviso. The view is a scrollable
/// list: `Down` moves it, and the test scrolls until each notice has been painted.
#[test]
fn binary_and_huge_files() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Huge);
    shop.observed();
    let mut tui = Tui::fleet(&shop);
    tui.ask_for_the_diff();
    let mut screen = tui.until_the_diff();
    let (mut truncated, mut binary) = (false, None);
    let start = Instant::now();
    while binary.is_none() {
        truncated |= any(&screen, "truncado");
        if let Some(at) = screen.iter().position(|l| l.contains("logo.png")) {
            binary = Some(screen[at..].to_vec());
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "logo.png never reached the screen:\n{}",
            screen.join("\n")
        );
        for _ in 0..200 {
            tui.press(KeyCode::Down);
        }
        tui.app.step(Duration::from_millis(20)).unwrap();
        screen = tui.screen();
    }
    let binary = binary.unwrap();
    assert!(
        binary[0].contains("binario"),
        "logo.png is not marked \"binario\": {binary:#?}"
    );
    let next = binary.get(1).map(|l| l.trim_start_matches([' ', '┃', '│']));
    assert!(
        next.is_none_or(|l| !l.starts_with(['+', '-', '@', '−'])),
        "a binary file has no lines: {binary:#?}"
    );
    assert!(
        truncated,
        "the 3.000-line big.txt is not marked \"truncado\" with a notice"
    );
}

// ------------------------------------------------------------ Escenario 3

/// Ver el diff no afecta al agente: footprint of the machine (outside the engine's own
/// profile, which the engine writes by itself) before and after, same Active session.
#[test]
fn viewing_the_diff_leaves_repo_session_and_worktree_intact() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Small);
    shop.observed();
    let mut tui = Tui::fleet(&shop);
    tui.until("claude-1 Active", |s| {
        row(s, "feat-pagos").is_some_and(|r| r.contains('●') && r.contains("claude-1"))
    });
    let before = shop.f.fingerprint();
    fingerprint::wait_for_timestamp_tick();
    tui.ask_for_the_diff();
    let screen = tui.until_the_diff();
    assert!(any(&screen, "sin commitear (1 archivo)"), "{screen:#?}");
    tui.press(KeyCode::Esc);
    let screen = tui.until("the fleet again, claude-1 still Active", |s| {
        live(s) && row(s, "feat-pagos").is_some_and(|r| r.contains('●') && r.contains("claude-1"))
    });
    assert!(!any(&screen, "entraría al merge"), "{screen:#?}");
    let changes: Vec<_> = fingerprint::diff(&before, &shop.f.fingerprint())
        .into_iter()
        .filter(|c| c.scope != "profile")
        .collect();
    assert!(
        changes.is_empty(),
        "viewing the diff changed the machine: {changes:#?}"
    );
}

// ------------------------------------------------------------ Escenario 4

/// Worktree compartido, sin atribución por archivo.
#[test]
fn shared_worktree_attributes_no_file() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Small);
    shop.observed();
    shop.agent("claude-2");
    let mut tui = Tui::fleet(&shop);
    tui.until("feat-pagos shared by claude-1 and claude-2", |s| {
        any(s, "claude-2")
    });
    tui.ask_for_the_diff();
    let screen = tui.until_the_diff();
    for agent in ["claude-1", "claude-2"] {
        assert!(
            !any(&screen, agent),
            "the diff view attributes work to {agent}: {screen:#?}"
        );
    }
}

// ------------------------------------------------------------ Escenario 5

/// Worktree no disponible: `d` no pide nada y dice el motivo.
#[test]
fn unavailable_worktree_disables_the_action_with_its_reason() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Small);
    let gone = shop.f.root.join("wt-pagos-moved");
    std::fs::rename(&shop.pagos, &gone).unwrap();
    shop.developer(&["repo", "add", shop.f.repo.to_str().unwrap()]);
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo);
    tui.until("feat-pagos unavailable", |s| {
        live(s) && any(s, "no disponible: falta la carpeta")
    });
    tui.press(KeyCode::Char('j'));
    tui.press(KeyCode::Char('d'));
    let screen = tui.until("the reason \"el worktree no está disponible\"", |s| {
        any(s, "el worktree no está disponible")
    });
    assert!(
        !any(&screen, "calculando") && !any(&screen, "entraría al merge"),
        "d asked for a diff on an unavailable worktree: {screen:#?}"
    );
}

// ------------------------------------------------------------ SEC-12

/// Every cell painted, as one string.
fn painted(tui: &Tui) -> String {
    tui.app
        .terminal()
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

/// Rutas y líneas con secuencias de control se pintan escapadas; tabulaciones y CRLF, legibles.
#[test]
fn untrusted_paths_and_lines_are_painted_inert() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Evil);
    shop.observed();
    let mut tui = Tui::fleet(&shop);
    tui.ask_for_the_diff();
    let screen = tui.until_the_diff();
    let cells = painted(&tui);
    for (name, bad) in [
        ("ESC", '\u{1b}'),
        ("BEL", '\u{7}'),
        ("RLO", '\u{202e}'),
        ("U+2028", '\u{2028}'),
        ("TAB", '\t'),
        ("CR", '\r'),
    ] {
        assert!(
            !cells.contains(bad),
            "the painted view holds a raw {name}: {screen:#?}"
        );
    }
    // The sequence is shown as text, not obeyed.
    assert!(any(&screen, "52;c;aGk="), "{screen:#?}");
    // A tab goes to the next multiple of 4 columns, and the CR of CRLF is dropped.
    assert!(
        any(&screen, "col1    col2"),
        "the tab is not expanded to spaces: {screen:#?}"
    );
    assert!(
        !any(&screen, "\\x09") && !any(&screen, "\\x0d") && !any(&screen, "\\r"),
        "tab or CRLF painted as an escape: {screen:#?}"
    );
}

// ------------------------------------------------------------ en / es

/// Every text of the view in its language, and the hint of `d` on the bar.
#[test]
fn texts_in_both_languages() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Huge);
    shop.observed();
    let spanish = ["entraría al merge", "sin commitear", "binario", "truncado"];
    for (lang, hint, want) in [
        (
            Lang::Es,
            "ver diff",
            ["(3 archivos)", "(1 archivo)", "binario", "truncado"],
        ),
        (
            Lang::En,
            "diff",
            ["(3 files)", "(1 file)", "binary", "truncated"],
        ),
    ] {
        let mut tui = Tui::fleet_in(lang, &shop);
        let bar = tui.screen();
        assert!(
            bar.iter().rev().take(3).any(|l| l.contains(hint)),
            "the bar has no hint for d (\"{hint}\"): {bar:#?}"
        );
        tui.ask_for_the_diff();
        let mut screen = tui.until("the diff view", |s| {
            any(s, "(3 archivos)") || any(s, "(3 files)")
        });
        // Everything painted while the view is scrolled to its last section.
        let mut seen = screen.join("\n");
        let last = want[1];
        let start = Instant::now();
        while !any(&screen, last) {
            assert!(
                start.elapsed() < DEADLINE,
                "the view never reached \"{last}\": {seen}"
            );
            for _ in 0..200 {
                tui.press(KeyCode::Down);
            }
            tui.app.step(Duration::from_millis(20)).unwrap();
            screen = tui.screen();
            seen.push('\n');
            seen.push_str(&screen.join("\n"));
        }
        for text in want {
            assert!(seen.contains(text), "{lang:?}: the view lacks \"{text}\"");
        }
        if matches!(lang, Lang::En) {
            for text in spanish {
                assert!(!seen.contains(text), "en view holds the Spanish \"{text}\"");
            }
        }
    }
}

// ------------------------------------------------------------ D10

/// El cursor guarda la clave del worktree: sobrevive a un reordenamiento y `d` pide ese diff.
#[test]
fn cursor_follows_the_worktree_across_reorders() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new(Work::Small);
    // feat-docs: one commit, nothing uncommitted: its diff says "(1 archivo)", pagos' says 3.
    shop.f.git(&["branch", "feat-docs"]);
    let docs = shop
        .f
        .add_worktree("docs", "feat-docs")
        .canonicalize()
        .unwrap();
    std::fs::write(docs.join("guide.md"), "docs\n").unwrap();
    shop.f.git_in(&docs, &["add", "."]);
    shop.f.git_in(&docs, &["commit", "-q", "-m", "docs"]);
    shop.observed();
    let pagos = shop.pagos.to_str().unwrap().to_owned();
    let mut tui = Tui::fleet(&shop);
    // main, feat-pagos (agent), feat-docs (no agent): the cursor goes down twice.
    let order = |s: &[String]| {
        let at = |b: &str| s.iter().position(|l| l.split_whitespace().any(|w| w == b));
        (at("feat-pagos"), at("feat-docs"))
    };
    let screen = tui.until(
        "pagos before docs",
        |s| matches!(order(s), (Some(p), Some(d)) if p < d),
    );
    assert!(any(&screen, "claude-1"));
    tui.press(KeyCode::Char('j'));
    tui.press(KeyCode::Char('j'));
    tui.app.step(Duration::from_millis(20)).unwrap();
    // docs gets an Active agent and pagos loses its own: docs moves above pagos.
    shop.developer(&[
        "agent",
        "register",
        "claude-2",
        "--worktree",
        docs.to_str().unwrap(),
    ]);
    shop.developer(&["agent", "withdraw", "claude-1", "--worktree", &pagos]);
    tui.until(
        "docs above pagos",
        |s| matches!(order(s), (Some(p), Some(d)) if d < p),
    );
    tui.press(KeyCode::Char('d'));
    let screen = tui.until_the_diff();
    assert!(
        any(&screen, "entraría al merge (1 archivo)"),
        "d asked for another row than the one under the cursor (feat-docs): {screen:#?}"
    );
    assert!(!any(&screen, "(3 archivos)"), "{screen:#?}");
}
