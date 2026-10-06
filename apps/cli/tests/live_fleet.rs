//! US-CKP-001 end to end: each Gherkin scenario against the real daemon (`raptor`, debug build,
//! on a temporary profile) over a temporary repo built by the "intact repo" harness
//! (INF-GRP-001). Never this repo nor the real profile (NFR-01). The TUI is the real `App`
//! (queues, channel thread, `update`, `view`) on `TestBackend`, in this process or, for the
//! process audit, in a child of its own.
//!
//! No fixed waits: every state is awaited on the painted screen with a deadline.
//!
//! macOS only: the developer runs under `script` with the macOS options. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma. The 500 ms p95 of scenario 2 is measured by
//! the engine bench (INF-GRP-002, scenario `tui-modify`), not here.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client::{self, ClientThread};
use gitraptor_cli::link::EngineConnector;
use gitraptor_cli::model::{ConnState, Model, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue;
use gitraptor_cli::tui::app::App;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_core::client::{ClientOptions, Launcher};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
/// The only agent executable the debug daemon knows: no process of this test is an agent.
const FAKE_AGENT: &str = "raptor-fake-agent";
/// Set for the child process that runs the TUI alone (scenario 4): the profile root.
const PROBE: &str = "RAPTOR_LIVE_FLEET_PROBE";
const READY: &str = "<<live-fleet-ready>>";
const DEADLINE: Duration = Duration::from_secs(20);

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// "shop": `main`, `feat-pagos` 3 commits ahead and 1 behind `main` with its worktree, and
/// `feat-docs` with its worktree.
struct Shop {
    f: Fixture,
    pagos: PathBuf,
    docs: PathBuf,
}

impl Shop {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("README.md", "shop\n");
        f.git(&["add", "README.md"]);
        f.git(&["commit", "-q", "-m", "shop"]);
        f.git(&["branch", "feat-pagos"]);
        f.git(&["branch", "feat-docs"]);
        let pagos = f
            .add_worktree("pagos", "feat-pagos")
            .canonicalize()
            .unwrap();
        let docs = f.add_worktree("docs", "feat-docs").canonicalize().unwrap();
        for i in 1..=3 {
            std::fs::write(pagos.join(format!("pay-{i}.txt")), format!("{i}\n")).unwrap();
            f.git_in(&pagos, &["add", "."]);
            f.git_in(&pagos, &["commit", "-q", "-m", &format!("pay {i}")]);
        }
        f.write("CHANGELOG.md", "main moved\n");
        f.git(&["add", "CHANGELOG.md"]);
        f.git(&["commit", "-q", "-m", "main moved"]);
        Self { f, pagos, docs }
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
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    /// Observed by the engine, with `claude-1` registered in `feat-pagos` and two new files
    /// there (its activity).
    fn observed(&self) {
        self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        self.developer(&[
            "agent",
            "register",
            "claude-1",
            "--worktree",
            self.pagos.to_str().unwrap(),
        ]);
        for name in ["checkout.rs", "refund.rs"] {
            std::fs::write(self.pagos.join(name), "todo\n").unwrap();
        }
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

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// A headless TUI on the running daemon; it never starts one.
struct Tui {
    app: App<TestBackend>,
    channel: Option<ClientThread>,
}

impl Tui {
    fn open(dirs: ProfileDirs, cwd: &Path) -> Self {
        let mut options = ClientOptions::new(dirs, ClientKind::Cli);
        options.launcher = Launcher::Never;
        let (inbox, _input, engine) = queue::inbox();
        let size = Size {
            width: 120,
            height: 30,
        };
        let model = Model::new(Lang::En, size);
        let mut app = App::new(
            Terminal::new(TestBackend::new(size.width, size.height)).unwrap(),
            model,
            inbox,
        );
        let channel = client::spawn(EngineConnector::new(options), Some(cwd.to_owned()), engine);
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

/// The row of the worktree on `branch`.
fn row<'s>(screen: &'s [String], branch: &str) -> Option<&'s str> {
    screen
        .iter()
        .find(|l| l.split_whitespace().any(|w| w == branch))
        .map(String::as_str)
}

fn has(screen: &[String], branch: &str, parts: &[&str]) -> bool {
    row(screen, branch).is_some_and(|r| parts.iter().all(|p| r.contains(p)))
}

/// Rows of the fleet, in order: the lines between the column header and the bottom border.
fn rows(screen: &[String]) -> Vec<&str> {
    screen
        .iter()
        .skip_while(|l| !l.contains("Agent"))
        .skip(1)
        .take_while(|l| !l.starts_with('┗') && !l.starts_with('└'))
        .map(String::as_str)
        .filter(|l| {
            !l.trim_matches(|c: char| c == '┃' || c == '│' || c == ' ')
                .is_empty()
        })
        .collect()
}

// ------------------------------------------------------------ Escenario 1

/// Una fila por worktree, con el agente primero.
#[test]
fn one_row_per_worktree_with_the_agent_first() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.observed();
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo);
    let screen = tui.until(
        "claude-1 Active in feat-pagos with 2 files and ↑3 ↓1",
        |s| has(s, "feat-pagos", &["●", "claude-1", "~2", "↑3 ↓1"]),
    );
    let rows = rows(&screen);
    assert_eq!(rows.len(), 3, "{screen:#?}");
    // The main worktree first.
    assert!(rows[0].split_whitespace().any(|w| w == "main"), "{rows:#?}");
    // The agent is the first thing of its row, with the Active symbol.
    let pagos = row(&screen, "feat-pagos").unwrap();
    assert!(pagos.find('●').unwrap() < pagos.find("claude-1").unwrap());
    assert!(pagos.find("claude-1").unwrap() < pagos.find("feat-pagos").unwrap());
    // Each agent names the folder of its worktree (dogfooding 2026-10-06).
    assert!(pagos.contains("claude-1 · wt-pagos"), "{pagos}");
    // ahead/behind names its reference and the age of the local copy: never fetched here.
    assert!(
        screen[1].contains("↑↓ vs main (local copy, never fetched)"),
        "{}",
        screen[1]
    );
    assert_eq!(tui.app.model.conn, ConnState::Live);
}

// ------------------------------------------------------------ Escenario 2

/// Un cambio se ve en vivo (el p95 de 500 ms lo mide el banco INF-GRP-002).
#[test]
fn a_change_reaches_the_row_live() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.observed();
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo);
    tui.until("2 files in feat-pagos", |s| {
        has(s, "feat-pagos", &["claude-1", "~2"])
    });
    let frames = tui.app.metrics.frames;
    std::fs::write(shop.pagos.join("invoice.rs"), "todo\n").unwrap();
    tui.until("3 files in feat-pagos", |s| {
        has(s, "feat-pagos", &["claude-1", "~3"])
    });
    // It came through the stream: new frames, and the TUI's own budget was measured.
    assert!(tui.app.metrics.frames > frames);
    assert!(tui.app.metrics.total.p95().is_some());
}

// ------------------------------------------------------------ Escenario 3

/// Lo no atribuido nunca se presenta como "humano".
#[test]
fn a_commit_without_an_agent_is_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.observed();
    std::fs::write(shop.docs.join("guide.md"), "docs\n").unwrap();
    shop.f.git_in(&shop.docs, &["add", "guide.md"]);
    shop.f.git_in(&shop.docs, &["commit", "-q", "-m", "docs"]);
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo);
    let screen = tui.until("feat-docs one commit ahead", |s| {
        has(s, "feat-docs", &["Unattributed (you/other)", "↑1 ↓1"])
    });
    let all = screen.join("\n").to_lowercase();
    assert!(!all.contains("human"), "{all}");
}

// ------------------------------------------------------------ Escenario 4

/// The child process of scenario 4: the TUI alone, with a `PATH` where `git` is a trap. It
/// prints the screen once the fleet is live, then [`READY`], and waits for its input to close
/// so the parent can look at its open files.
#[test]
fn tui_probe_entry() {
    let Some(profile) = std::env::var_os(PROBE) else {
        return;
    };
    let cwd = std::env::current_dir().unwrap();
    let mut tui = Tui::open(ProfileDirs::under_root(PathBuf::from(profile)), &cwd);
    let screen = tui.until("the fleet", |s| has(s, "feat-pagos", &["claude-1", "~2"]));
    println!("{}", screen.join("\n"));
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    drop(tui);
    std::process::exit(0);
}

/// La TUI no calcula ningún campo: la última actividad se pinta como la publica el motor
/// (su antigüedad, o "no disponible" mientras no la conoce), y el proceso de la TUI no lanza Git
/// ni abre el perfil (V5 y V6 de ADR-CKP-003, auditados en su propio PID).
#[test]
fn an_unpublished_field_is_not_computed_by_the_tui() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.observed();
    // The trap: any `git` the TUI process looked up by name leaves a mark.
    let trap = shop.f.root.join("trap");
    std::fs::create_dir(&trap).unwrap();
    let fired = shop.f.root.join("git-fired");
    let script = trap.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexit 127\n",
            fired.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "tui_probe_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(PROBE, &shop.f.profile)
        .env("PATH", &trap)
        .env("HOME", &shop.f.home)
        .current_dir(&shop.f.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut screen = Vec::new();
    loop {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "probe died: {screen:#?}"
        );
        if line.contains(READY) {
            break;
        }
        screen.push(line.trim_end().to_owned());
    }
    // What the TUI process has open while it shows the fleet: nothing of the profile but
    // the channel's socket.
    let lsof = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-Fn", "-p", &child.id().to_string()])
        .output()
        .unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let open = String::from_utf8_lossy(&lsof.stdout);
    // lsof saw the process: at least its working folder.
    assert!(open.lines().any(|l| l.starts_with('n')), "lsof: {open}");
    let profile = shop.f.profile.to_string_lossy().into_owned();
    for name in open.lines().filter_map(|l| l.strip_prefix('n')) {
        assert!(
            !name.starts_with(&profile) || name.ends_with(gitraptor_api::SOCKET_FILE),
            "the TUI opened {name}"
        );
    }
    assert!(
        !fired.exists(),
        "the TUI launched git: {:?}",
        std::fs::read_to_string(&fired)
    );
    // Every row shows the last activity as the engine published it (`scope.activity`): its
    // age, or "not available" while the engine has not seen the worktree change.
    let rows = rows(&screen);
    assert_eq!(rows.len(), 3, "{screen:#?}");
    for r in rows {
        let end = r.trim_end_matches(['┃', '│']).trim_end();
        assert!(
            ["not available", "just now", " ago"]
                .iter()
                .any(|t| end.ends_with(t)),
            "{r}"
        );
    }
}

// ------------------------------------------------------------ Escenario 5

/// Texto no confiable no altera la terminal. Git refuses a branch name with a control character
/// (`check-ref-format`) and the engine refuses one in an agent's name, so the untrusted text
/// that does reach the TUI here is a worktree whose folder has an escape sequence. The branch
/// of the scenario is covered on the view (`a_malicious_branch_is_painted_inert`).
#[test]
fn untrusted_text_is_painted_inert() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let shop = Shop::new();
    shop.f.git(&["branch", "feat-evil"]);
    let evil = shop.f.root.join("wt-evil\u{1b}[2J\u{1b}]52;c;eA==\u{7}");
    shop.f
        .git(&["worktree", "add", "-q", evil.to_str().unwrap(), "feat-evil"]);
    shop.developer(&["repo", "add", shop.f.repo.to_str().unwrap()]);
    let refused = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            RAPTOR,
            "agent",
            "register",
            "evil\u{1b}[2J",
            "--worktree",
        ])
        .arg(&shop.docs)
        .env_clear()
        .envs(shop.env())
        .current_dir(&shop.f.root)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!refused.status.success(), "{}", text(&refused));
    let mut tui = Tui::open(shop.dirs(), &shop.f.repo);
    let screen = tui.until("the worktree with the escape in its folder", |s| {
        has(s, "feat-evil", &["Unattributed"])
    });
    let painted: String = tui
        .app
        .terminal()
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        !painted.contains('\u{1b}') && !painted.contains('\u{7}'),
        "{screen:#?}"
    );
    assert_eq!(tui.app.model.conn, ConnState::Live);
}
