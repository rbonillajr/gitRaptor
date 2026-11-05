//! US-CKP-006 end to end: the six scenarios of the story against the real daemon (`raptor`,
//! debug build, on a temporary profile) over temporary repos. Never this repo nor the real
//! profile (NFR-01). The TUI is the real `App` (queues, channel thread, `update`, `view`) on
//! `TestBackend`, same harness as `live_fleet.rs`.
//!
//! No fixed waits: every state is awaited on the painted screen with a deadline of 30 s (the
//! 5 s p95 of S-CKP-1 is measured by TS-CKP-001's T012).
//!
//! macOS only; Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client::engine::EngineConnector;
use gitraptor_cli::client::{self, ClientThread};
use gitraptor_cli::model::{Model, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue;
use gitraptor_cli::tui::app::App;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_core::client::{ClientOptions, Launcher};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Snapshot, diff};
use gitraptor_testkit::fixture::git_from_path;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
/// The only agent executable the debug daemon knows: no process of this test is an agent.
const FAKE_AGENT: &str = "raptor-fake-agent";
const DEADLINE: Duration = Duration::from_secs(30);
const ES_LIMITS: &str = "⚠ con lo sin commitear · ⚡ solo commits · sin drivers ni .gitattributes";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// "shop": `main` with `src/api.rs` (30 lines) and `README.md`, and the worktrees each scenario
/// adds.
struct Shop {
    f: Fixture,
}

/// `src/api.rs` with lines 10 to 12 replaced by `<tag>-10..12`.
fn api_rs(tag: &str) -> String {
    (1..=30)
        .map(|n| {
            if (10..=12).contains(&n) {
                format!("{tag}-{n}\n")
            } else {
                format!("line {n}\n")
            }
        })
        .collect()
}

impl Shop {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("src/api.rs", &api_rs("base"));
        f.write("README.md", "shop\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "shop"]);
        Self { f }
    }

    /// A linked worktree `wt-<name>` on a new branch `<branch>` cut from `main`.
    fn worktree(&self, name: &str, branch: &str) -> PathBuf {
        self.f.git(&["branch", branch]);
        self.f.add_worktree(name, branch).canonicalize().unwrap()
    }

    fn commit(&self, wt: &Path, file: &str, content: &str) {
        std::fs::create_dir_all(wt.join(file).parent().unwrap()).unwrap();
        std::fs::write(wt.join(file), content).unwrap();
        self.f.git_in(wt, &["add", "."]);
        self.f
            .git_in(wt, &["commit", "-q", "-m", &format!("change {file}")]);
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
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
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    /// Observed by the engine, with each `(agent, worktree)` registered.
    fn observed(&self, agents: &[(&str, &Path)]) {
        self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        for (name, wt) in agents {
            self.developer(&[
                "agent",
                "register",
                name,
                "--worktree",
                wt.to_str().unwrap(),
            ]);
        }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    /// The repo and its worktrees, never the profile the engine writes (NFR-01).
    fn fingerprint(&self) -> Snapshot {
        let scopes: Vec<_> = self
            .f
            .scopes()
            .into_iter()
            .filter(|s| s.label != "profile")
            .collect();
        Snapshot::take(&scopes, &BTreeSet::new())
    }

    /// The four agents of the story: claude-1 and claude-2 commit different changes to the
    /// same lines of `src/api.rs`, claude-3 and claude-4 work on files of their own.
    fn four_agents(&self) -> [PathBuf; 4] {
        let wts = [
            self.worktree("c1", "feat-1"),
            self.worktree("c2", "feat-2"),
            self.worktree("c3", "feat-3"),
            self.worktree("c4", "feat-4"),
        ];
        self.commit(&wts[0], "src/api.rs", &api_rs("one"));
        self.commit(&wts[1], "src/api.rs", &api_rs("two"));
        self.commit(&wts[2], "docs/c3.md", "c3\n");
        self.commit(&wts[3], "docs/c4.md", "c4\n");
        self.observed(&[
            ("claude-1", &wts[0]),
            ("claude-2", &wts[1]),
            ("claude-3", &wts[2]),
            ("claude-4", &wts[3]),
        ]);
        wts
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
    channel: Option<ClientThread>,
}

impl Tui {
    fn open(dirs: ProfileDirs, cwd: &Path, lang: Lang) -> Self {
        let mut options = ClientOptions::new(dirs, ClientKind::Cli);
        options.launcher = Launcher::Never;
        let (inbox, _input, engine) = queue::inbox();
        let size = Size {
            width: 120,
            height: 40,
        };
        let mut app = App::new(
            Terminal::new(TestBackend::new(size.width, size.height)).unwrap(),
            Model::new(lang, size),
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
            channel: Some(channel),
        }
    }

    fn screen(&self) -> Vec<String> {
        buffer_text(self.app.terminal().backend().buffer())
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
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            channel.shutdown();
        }
    }
}

/// A pair line: the ⚡/⚠ symbol, the sides and the "versus" glyph.
fn pair_line<'s>(screen: &'s [String], symbol: char, names: &[&str]) -> Option<&'s String> {
    screen
        .iter()
        .find(|l| l.contains(symbol) && l.contains('↔') && names.iter().all(|n| l.contains(n)))
}

fn any(screen: &[String], needle: &str) -> bool {
    screen.iter().any(|l| l.contains(needle))
}

/// ⚡ with `src/api.rs` and the hunk's range, between the two agents.
fn demo_visible(screen: &[String], a: &str, b: &str) -> bool {
    pair_line(screen, '⚡', &[a, b]).is_some()
        && any(screen, "src/api.rs")
        && any(screen, "10–12 ↔ 10–12")
}

// ------------------------------------------------------------ Escenario 1

/// Demo del BRD § 13: el conflicto se ve antes del merge y luego ocurre.
#[test]
fn demo_conflict_is_seen_before_the_merge_and_the_merge_produces_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.four_agents();
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::En);
    let screen = tui.until(
        "⚡ claude-1 ↔ claude-2 with src/api.rs and its range",
        |s| demo_visible(s, "claude-1", "claude-2"),
    );
    // claude-3 and claude-4 work on files of their own: no ⚡ for them.
    assert!(
        pair_line(&screen, '⚡', &["claude-3", "claude-4"]).is_none(),
        "{screen:#?}"
    );
    // Then the real merge, in a disposable clone of the fixture.
    let clone = shop.f.root.join("clone");
    shop.f.git_in(
        &shop.f.root,
        &[
            "clone",
            "-q",
            "--no-hardlinks",
            shop.f.repo.to_str().unwrap(),
            "clone",
        ],
    );
    shop.f
        .git_in(&clone, &["checkout", "-q", "-b", "m1", "origin/feat-1"]);
    let merge = shop
        .f
        .git_command(&clone, &["merge", "--no-edit", "origin/feat-2"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the real merge must conflict");
    let status = shop.f.git_in(&clone, &["status", "--porcelain"]);
    assert!(
        status
            .lines()
            .any(|l| l.starts_with("UU") && l.ends_with("src/api.rs")),
        "Git leaves src/api.rs in conflict: {status}"
    );
}

// ------------------------------------------------------------ Escenario 2

/// Lo sin commitear solo cuenta como solape.
#[test]
fn uncommitted_work_only_overlaps() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    let c3 = shop.worktree("c3", "feat-3");
    let c4 = shop.worktree("c4", "feat-4");
    std::fs::write(c3.join("README.md"), "claude-3 edits, not committed\n").unwrap();
    shop.commit(&c4, "README.md", "claude-4 edits, committed\n");
    shop.observed(&[("claude-3", &c3), ("claude-4", &c4)]);
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::En);
    let screen = tui.until("⚠ claude-3 ↔ claude-4 with README.md", |s| {
        pair_line(s, '⚠', &["claude-3", "claude-4"]).is_some() && any(s, "README.md")
    });
    assert!(
        !screen.iter().any(|l| l.contains('⚡') && l.contains('↔')),
        "no ⚡ pair: committed work only overlaps with uncommitted work:\n{}",
        screen.join("\n")
    );
}

// ------------------------------------------------------------ Escenario 3

/// Cada agente contra la base confirmada.
#[test]
#[ignore = "G-B: confirming the base at runtime does not replan (follow-up of US-CKP-006)"]
fn a_worktree_against_the_confirmed_base_shows_the_conflict() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    let pagos = shop.worktree("pagos", "feat-pagos");
    shop.commit(&pagos, "src/api.rs", &api_rs("pagos"));
    shop.f.write("src/api.rs", &api_rs("main"));
    shop.f.git(&["add", "."]);
    shop.f
        .git(&["commit", "-q", "-m", "main moves the same lines"]);
    shop.observed(&[]);
    // `main` is the confirmed base once the developer confirms it.
    shop.developer(&["guard", "install", "--yes", shop.f.repo.to_str().unwrap()]);
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::En);
    tui.until("⚡ wt-pagos ↔ main with src/api.rs and its range", |s| {
        pair_line(s, '⚡', &["wt-pagos", "main"]).is_some()
            && any(s, "src/api.rs")
            && any(s, "10–12 ↔ 10–12")
    });
}

// ------------------------------------------------------------ Escenario 4

/// La vista declara sus límites (in Spanish, the texts the DS fixes).
#[test]
fn the_panel_declares_its_limits() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.four_agents();
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::Es);
    let screen = tui.until("a visible prediction", |s| {
        pair_line(s, '⚡', &["claude-1", "claude-2"]).is_some()
    });
    assert!(
        any(&screen, ES_LIMITS),
        "the panel declares that ⚠ includes uncommitted work, ⚡ uses commits only and no merge drivers or .gitattributes apply:\n{}",
        screen.join("\n")
    );
}

// ------------------------------------------------------------ Escenario 5

/// Predecir no deja rastro en el repo (NFR-01).
#[test]
fn predicting_with_the_tui_open_leaves_the_repo_intact() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.four_agents();
    let before = shop.fingerprint();
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::En);
    tui.until("the published pairs", |s| {
        demo_visible(s, "claude-1", "claude-2")
    });
    let changes = diff(&before, &shop.fingerprint());
    assert!(
        changes.is_empty(),
        "predicting changed the repo: {:#?}",
        changes.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
}

// ------------------------------------------------------------ Escenario 6

/// Un worktree sin trabajo propio no forma pares entre worktrees.
#[test]
fn a_worktree_without_own_work_forms_no_worktree_pairs() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    let c1 = shop.worktree("c1", "feat-1");
    let c2 = shop.worktree("c2", "feat-2");
    shop.commit(&c1, "src/api.rs", &api_rs("one"));
    shop.commit(&c2, "src/api.rs", &api_rs("two"));
    // Just created from the base: no commits, no changes.
    shop.worktree("nuevo", "feat-nuevo");
    shop.observed(&[("claude-1", &c1), ("claude-2", &c2)]);
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo, Lang::En);
    // The panel is published (a real pair shows) before looking for what must not be there.
    let screen = tui.until("the published pairs", |s| {
        pair_line(s, '⚡', &["claude-1", "claude-2"]).is_some()
    });
    assert!(
        !screen
            .iter()
            .any(|l| l.contains('↔') && l.contains("wt-nuevo") && l.contains("claude-")),
        "feat-nuevo has no own work, so no pair with another worktree:\n{}",
        screen.join("\n")
    );
}
