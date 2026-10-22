//! US-CKP-003 end to end: what `raptor` says about the engine and what to do, with the real
//! binary under a pty (`script`) against the real daemon on a temporary profile and temporary
//! repos (INF-GRP-001). Never this repo nor the real profile (NFR-01).
//!
//! No fixed waits: every check waits on the screen the pty painted, with a deadline.
//!
//! macOS only: `script` and `lsof` are the macOS ones. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client::engine::EngineConnector;
use gitraptor_cli::client::{self, ClientThread};
use gitraptor_cli::model::{Model, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue;
use gitraptor_cli::tui::app::App;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::keymap::Action;
use gitraptor_core::client::{ClientOptions, Launcher};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const DEADLINE: Duration = Duration::from_secs(20);
const LEAVE_ALT: &str = "\u{1b}[?1049l";
/// Set for the child process that runs the TUI alone: the profile root.
const PROBE: &str = "RAPTOR_ENGINE_STATES_PROBE";
const READY: &str = "<<engine-states-ready>>";
/// The pty of `script` starts with no size: give it one, then become `raptor`.
const SIZED: &str = "stty rows 30 cols 100 && exec \"$0\"";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// The temporary machine of one scenario.
struct Machine {
    f: Fixture,
}

impl Machine {
    fn new() -> Self {
        Self::with(Fixture::new(&git_from_path()))
    }

    fn with(f: Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { f }
    }

    fn env(&self, lang: &str, extra: &[(&'static str, OsString)]) -> Vec<(&'static str, OsString)> {
        let locale = if lang == "es" {
            "es_ES.UTF-8"
        } else {
            "en_US.UTF-8"
        };
        let mut env = vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", locale.into()),
            ("TERM", "xterm-256color".into()),
        ];
        env.extend(extra.iter().cloned());
        env
    }

    /// `raptor repo add <path>` from the developer's own terminal.
    fn add(&self, path: &Path) {
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "repo", "add"])
            .arg(path)
            .env_clear()
            .envs(self.env("en", &[]))
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }

    /// `raptor` (the TUI) from the developer's own terminal, in `cwd`.
    fn tui(&self, cwd: &Path, lang: &str, extra: &[(&'static str, OsString)]) -> Pty {
        let child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh", "-c", SIZED, RAPTOR])
            .env_clear()
            .envs(self.env(lang, extra))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Pty::new(child)
    }

    fn engine_pid(&self) -> Option<u32> {
        let dirs = ProfileDirs::under_root(&self.f.profile);
        running_pid(&dirs.state).ok().flatten()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // A daemon started on demand is detached: never leave it behind.
        if let Some(pid) = self.engine_pid() {
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

/// A repo `<root>/<name>` with one commit on `main` and a linked worktree `<root>/<name>-wt` on
/// `branch`.
fn repo(f: &Fixture, name: &str, branch: &str) -> PathBuf {
    let dir = f.root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    f.git_in(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README.md"), format!("{name}\n")).unwrap();
    f.git_in(&dir, &["add", "README.md"]);
    f.git_in(&dir, &["commit", "-q", "-m", name]);
    let wt = f.root.join(format!("{name}-wt"));
    f.git_in(
        &dir,
        &["worktree", "add", "-q", "-b", branch, wt.to_str().unwrap()],
    );
    dir.canonicalize().unwrap()
}

/// The text on the screen after replaying what the TUI painted. ratatui repaints only the cells
/// that changed, so a word already on screen is not sent again: a wait must look at the screen,
/// not at the bytes. Only cursor positioning, printable text and erasures are modeled.
struct Screen {
    rows: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// Bytes of an escape sequence cut by the end of a read.
    pending: String,
}

impl Screen {
    const ROWS: usize = 30;
    const COLS: usize = 100;

    fn new() -> Self {
        Self {
            rows: vec![vec![' '; Self::COLS]; Self::ROWS],
            row: 0,
            col: 0,
            pending: String::new(),
        }
    }

    fn put(&mut self, c: char) {
        if self.row < Self::ROWS && self.col < Self::COLS {
            self.rows[self.row][self.col] = c;
        }
        self.col += 1;
    }

    fn feed(&mut self, bytes: &str) {
        let input = std::mem::take(&mut self.pending) + bytes;
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' => {
                    let mut seq = String::from(c);
                    let Some(&kind) = chars.peek() else {
                        self.pending = seq;
                        return;
                    };
                    match kind {
                        '[' => {
                            seq.extend(chars.next());
                            loop {
                                let Some(b) = chars.next() else {
                                    self.pending = seq;
                                    return;
                                };
                                seq.push(b);
                                if ('@'..='~').contains(&b) {
                                    break;
                                }
                            }
                            self.csi(&seq[2..]);
                        }
                        ']' => {
                            seq.extend(chars.next());
                            loop {
                                let Some(b) = chars.next() else {
                                    self.pending = seq;
                                    return;
                                };
                                seq.push(b);
                                if b == '\u{7}' || seq.ends_with("\u{1b}\\") {
                                    break;
                                }
                            }
                        }
                        _ => {
                            chars.next();
                        }
                    }
                }
                '\r' => self.col = 0,
                '\n' => self.row += 1,
                c if !c.is_control() => self.put(c),
                _ => {}
            }
        }
    }

    fn csi(&mut self, seq: &str) {
        let (params, last) = seq.split_at(seq.len() - 1);
        let nums: Vec<usize> = params.split(';').map(|n| n.parse().unwrap_or(0)).collect();
        let n = |i: usize| nums.get(i).copied().filter(|v| *v > 0).unwrap_or(1);
        match last {
            "H" | "f" => {
                self.row = n(0) - 1;
                self.col = n(1) - 1;
            }
            "J" if nums[0] == 2 => self.rows.iter_mut().for_each(|r| r.fill(' ')),
            "K" if self.row < Self::ROWS => {
                let from = if nums[0] == 0 { self.col } else { 0 };
                let to = if nums[0] == 1 {
                    self.col + 1
                } else {
                    Self::COLS
                };
                for c in from.min(Self::COLS)..to.min(Self::COLS) {
                    self.rows[self.row][c] = ' ';
                }
            }
            "X" if self.row < Self::ROWS => {
                for c in self.col..(self.col + n(0)).min(Self::COLS) {
                    self.rows[self.row][c] = ' ';
                }
            }
            "C" => self.col += n(0),
            _ => {}
        }
    }

    fn lines(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.iter().collect()).collect()
    }

    fn contains(&self, what: &str) -> bool {
        self.lines().iter().any(|l| l.contains(what))
    }
}

/// The TUI under a pty: what it paints, read on a thread, and its keyboard.
struct Pty {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
    seen: String,
    screen: Screen,
}

impl Pty {
    fn new(mut child: Child) -> Self {
        let stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 || tx.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            rx,
            reader: Some(reader),
            seen: String::new(),
            screen: Screen::new(),
        }
    }

    /// Waits until `what` is on the screen.
    fn wait(&mut self, what: &str) {
        let deadline = Instant::now() + DEADLINE;
        while !self.screen.contains(what) {
            let left = deadline.saturating_duration_since(Instant::now());
            let bytes = self.rx.recv_timeout(left).unwrap_or_else(|_| {
                panic!(
                    "waiting for {what:?}; the screen:\n{}",
                    self.screen.lines().join("\n")
                )
            });
            let bytes = String::from_utf8_lossy(&bytes);
            self.screen.feed(&bytes);
            self.seen.push_str(&bytes);
        }
    }

    /// Takes what the TUI painted up to now, without waiting for more.
    fn settle(&mut self) {
        while let Ok(bytes) = self.rx.recv_timeout(Duration::from_millis(300)) {
            let bytes = String::from_utf8_lossy(&bytes);
            self.screen.feed(&bytes);
            self.seen.push_str(&bytes);
        }
    }

    /// `q`, and everything the TUI painted until it left the alternate screen.
    fn quit(mut self) -> String {
        self.stdin.write_all(b"q").unwrap();
        self.stdin.flush().unwrap();
        self.wait_bytes(LEAVE_ALT);
        let status = self.child.wait().unwrap();
        drop(self.rx);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        assert!(status.success(), "{status:?}: {:?}", self.seen);
        self.seen
    }

    fn wait_bytes(&mut self, what: &str) {
        let deadline = Instant::now() + DEADLINE;
        while !self.seen.contains(what) {
            let left = deadline.saturating_duration_since(Instant::now());
            let bytes = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("waiting for {what:?}: {:?}", self.seen));
            self.seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
}

// ------------------------------------------------------------ Escenario 1

/// Without a daemon the TUI starts it and goes live (regression guard: it passes today).
#[test]
fn without_a_daemon_the_tui_starts_it_and_goes_live() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    assert!(m.engine_pid().is_none());
    let mut tui = m.tui(&m.f.root, "en", &[]);
    tui.wait("live");
    tui.quit();
    assert!(m.engine_pid().is_some());
}

// ------------------------------------------------------------ Escenario 2

/// The child process of scenario 2: the TUI alone, with `git` a trap on its `PATH` and an engine
/// it cannot start. It prints the screen once the panel is there, then [`READY`], and waits for
/// its input to close so the parent can look at its open files.
#[test]
fn tui_probe_entry() {
    let Some(profile) = std::env::var_os(PROBE) else {
        return;
    };
    let profile = PathBuf::from(profile);
    let cwd = std::env::current_dir().unwrap();
    let mut options = ClientOptions::new(ProfileDirs::under_root(&profile), ClientKind::Cli);
    options.launcher = Launcher::Installed(profile.parent().unwrap().join("no-raptor"));
    let (inbox, _input, engine) = queue::inbox();
    let size = Size {
        width: 120,
        height: 30,
    };
    let mut app = App::new(
        Terminal::new(TestBackend::new(size.width, size.height)).unwrap(),
        Model::new(Lang::En, size),
        inbox,
    );
    let channel: ClientThread = client::spawn(
        EngineConnector::new(options.connect().unwrap(), Box::new(options.launcher())),
        Some(cwd),
        engine,
    );
    app.attach(channel.cmds.clone());
    let start = Instant::now();
    let screen = loop {
        app.step(Duration::from_millis(20)).unwrap();
        let screen = buffer_text(app.terminal().backend().buffer());
        let all = screen.join("\n");
        if !app.model.dirty && all.contains("Engine unavailable") && all.contains("raptor daemon") {
            break screen;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for the panel ({:?}):\n{all}",
            app.model.conn
        );
    };
    println!("{}", screen.join("\n"));
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    channel.shutdown();
    std::process::exit(0);
}

/// Without an engine it can start, the TUI says how to start it, launches no Git and opens
/// nothing of the profile (V5, V6 of ADR-CKP-003), and creates no data.
#[test]
fn when_the_engine_cannot_start_the_tui_says_how_and_observes_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    // The trap: any `git` the TUI process looked up by name leaves a mark.
    let trap = m.f.root.join("trap");
    std::fs::create_dir(&trap).unwrap();
    let fired = m.f.root.join("git-fired");
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
        .env(PROBE, &m.f.profile)
        .env("PATH", &trap)
        .env("HOME", &m.f.home)
        .current_dir(&m.f.repo)
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
    let all = screen.join("\n");
    assert!(all.contains("Engine unavailable"), "{all}");
    assert!(all.contains("`raptor daemon`"), "{all}");
    // What the TUI process has open while it shows the panel: nothing of the profile.
    let lsof = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-Fn", "-p", &child.id().to_string()])
        .output()
        .unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let open = String::from_utf8_lossy(&lsof.stdout);
    assert!(open.lines().any(|l| l.starts_with('n')), "lsof: {open}");
    let profile = m.f.profile.to_string_lossy().into_owned();
    for name in open.lines().filter_map(|l| l.strip_prefix('n')) {
        assert!(!name.starts_with(&profile), "the TUI opened {name}");
    }
    assert!(
        !fired.exists(),
        "the TUI launched git: {:?}",
        std::fs::read_to_string(&fired)
    );
    let data = std::fs::read_dir(m.f.profile.join("data")).unwrap().count();
    assert_eq!(data, 0, "the profile's data folder is not empty");
}

// ------------------------------------------------------------ Escenario 3

/// Without Git and without repos the screen waits for Git and does not guide to the first repo.
#[test]
fn without_git_and_without_repos_it_waits_for_git_not_the_first_repo_guide() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (lang, title) in [("en", "Waiting for Git"), ("es", "Esperando Git")] {
        let m = Machine::new();
        let no_git: OsString = m.f.root.join("no-git").into_os_string();
        let mut tui = m.tui(&m.f.root, lang, &[("GITRAPTOR_TEST_GIT", no_git)]);
        tui.wait(title);
        tui.wait("2.38");
        tui.settle();
        let all = tui.screen.lines().join("\n");
        assert!(!all.contains("raptor repo add"), "{lang}\n{all}");
        tui.quit();
    }
}

// ------------------------------------------------------------ Escenario 4

/// With Git and without repos the screen guides to the first one, with the command.
#[test]
fn without_repos_it_shows_how_to_add_the_first() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (lang, command) in [
        ("en", "raptor repo add <path>"),
        ("es", "raptor repo add <ruta>"),
    ] {
        let m = Machine::new();
        let mut tui = m.tui(&m.f.root, lang, &[]);
        tui.wait(command);
        tui.quit();
    }
}

// ------------------------------------------------------------ Escenario 6

/// A worktree whose folder was deleted says it is unavailable, and no key acts on it.
#[test]
fn a_deleted_worktree_says_not_available_and_offers_no_action() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let shop = repo(&m.f, "shop", "feat-x");
    m.add(&shop);
    std::fs::remove_dir_all(m.f.root.join("shop-wt")).unwrap();
    let mut tui = m.tui(&m.f.root, "en", &[]);
    tui.wait("unavailable: folder missing");
    tui.quit();
}

// ------------------------------------------------------------ Escenario 7

/// The repo of `base_branch.rs::a_missing_base_branch_is_reported_and_no_other_is_used`: no
/// `main`, with `origin/main` and a tag `main` as traps. Every row says "not computable", the
/// title names the base, and neither names another branch.
#[test]
fn a_missing_base_branch_is_not_computable_and_no_other_is_used() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new(&git_from_path());
    let otro = f.other_repo.clone();
    f.git_in(&otro, &["checkout", "-q", "-b", "trunk"]);
    for i in 0..2 {
        f.git_in(
            &otro,
            &["commit", "-q", "--allow-empty", "-m", &format!("trunk {i}")],
        );
    }
    f.git_in(&otro, &["branch", "feat"]);
    let feat = f.root.join("wt-otro-feat");
    f.git_in(
        &otro,
        &["worktree", "add", "-q", feat.to_str().unwrap(), "feat"],
    );
    f.git_in(&otro, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
    f.git_in(&otro, &["tag", "main", "HEAD~1"]);
    let m = Machine::with(f);
    m.add(&otro);
    let mut tui = m.tui(&m.f.root, "en", &[]);
    tui.wait("not computable: main not found");
    tui.wait("trunk");
    tui.settle();
    let screen = tui.screen.lines();
    let title = screen.iter().find(|l| l.contains("Fleet")).unwrap();
    for other in ["trunk", "origin"] {
        assert!(!title.contains(other), "{title}");
    }
    let rows: Vec<&String> = screen
        .iter()
        .filter(|l| {
            l.split_whitespace()
                .any(|w| w == "trunk" || w == "feat" || w == "feat-x")
        })
        .collect();
    assert_eq!(rows.len(), 2, "{screen:#?}");
    for row in rows {
        assert!(row.contains("not computable"), "{row}");
        // The activity column paints `–` for a worktree with no activity: only the ↑↓ cell
        // must carry no count (a dash there would not say "not computable" either).
        for glyph in ['↑', '↓'] {
            assert!(!row.contains(glyph), "{row}");
        }
    }
    tui.quit();
}

/// Every key of the TUI is global: none acts on a row. The first action per row (US-CKP-012)
/// has to be disabled with a reason on an unavailable worktree (BR-CKP-EDGE-003) before being
/// classified here: a new `Action` stops this from compiling until someone decides (D7).
#[test]
fn no_action_targets_a_row() {
    fn targets_a_row(action: Action) -> bool {
        match action {
            Action::Quit
            | Action::Retry
            | Action::Up
            | Action::Down
            | Action::Open
            | Action::Suspend
            | Action::Yes
            | Action::No
            | Action::Later
            | Action::Help => false,
        }
    }
    for action in [
        Action::Quit,
        Action::Retry,
        Action::Up,
        Action::Down,
        Action::Open,
        Action::Suspend,
        Action::Yes,
        Action::No,
        Action::Later,
        Action::Help,
    ] {
        assert!(!targets_a_row(action), "{action:?}");
    }
}
