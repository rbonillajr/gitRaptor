//! US-CKP-025 end to end: `raptor` opened inside a repo the engine does not observe. The
//! developer runs the real binary under a pty (`script`), against the real daemon on a
//! temporary profile and temporary repos built by the "intact repo" harness (INF-GRP-001).
//! Never this repo nor the real profile (NFR-01).
//!
//! The simulated agent is a copy of this test binary named `raptor-fake-agent` (see
//! `fake_agent_entry`), declared to the daemon through the debug-only
//! `GITRAPTOR_AGENT_EXECUTABLES`, as in `repo_state.rs`.
//!
//! No fixed waits: every key is written once its prompt is on the pty's output, with a
//! deadline. The renderer skips cells that did not change, so only words without spaces are
//! looked for.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const DEADLINE: Duration = Duration::from_secs(30);
const LEAVE_ALT: &str = "\u{1b}[?1049l";
/// The pty of `script` starts with no size, and the TUI would paint nothing: give it one
/// first, then become `raptor` (same process, so the ancestry is unchanged).
const SIZED: &str = "stty rows 30 cols 100 && exec \"$0\"";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated agent: when this test binary runs as `raptor-fake-agent`
/// with `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child, with its own stdio, and
/// exits with its status. As a normal test it does nothing.
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// The machine: "notes" not observed (with a linked worktree on `feat-notas`), and the
/// observed repos the scenario asks for.
struct Machine {
    f: Fixture,
    notes: PathBuf,
}

impl Machine {
    fn new(observed: &[&str]) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let notes = repo(&f, "notes", "feat-notas");
        let m = Self { f, notes };
        for name in observed {
            let path = repo(&m.f, name, &format!("feat-{name}"));
            let out = m.developer(&["repo", "add", path.to_str().unwrap()], &m.f.root, "en");
            assert!(out.status.success(), "{}", text(&out));
        }
        m
    }

    fn env(&self, lang: &str) -> Vec<(&'static str, OsString)> {
        let locale = if lang == "es" {
            "es_ES.UTF-8"
        } else {
            "en_US.UTF-8"
        };
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", locale.into()),
            ("TERM", "xterm-256color".into()),
        ]
    }

    /// A one-shot command from the developer's own terminal.
    fn developer(&self, args: &[&str], cwd: &Path, lang: &str) -> Output {
        Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR])
            .args(args)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `raptor` (the TUI) from the developer's own terminal, in `cwd`.
    fn tui(&self, cwd: &Path, lang: &str) -> Pty {
        let child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh", "-c", SIZED, RAPTOR])
            .env_clear()
            .envs(self.env(lang))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Pty::new(child)
    }

    /// `raptor` (the TUI) in a terminal an agent opened: the agent is an ancestor.
    fn tui_as_agent(&self, cwd: &Path) -> Pty {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        let argv = [
            "/usr/bin/script",
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            SIZED,
            RAPTOR,
        ];
        let child = Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env("en"))
            .env(FAKE_AGENT_ARGV, serde_json::to_string(&argv).unwrap())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Pty::new(child)
    }

    /// The folders of the observed repos, from `raptor status --json`.
    fn observed(&self) -> Vec<String> {
        let out = Command::new(RAPTOR)
            .args(["status", "--json"])
            .env_clear()
            .envs(self.env("en"))
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        status["repos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_owned())
            .collect()
    }

    fn notes_observed(&self) -> bool {
        self.observed().iter().any(|p| p.ends_with("/notes"))
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // A daemon started on demand is detached: never leave it behind.
        let dirs = ProfileDirs::under_root(&self.f.profile);
        if let Ok(Some(pid)) = running_pid(&dirs.state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// A repo `<root>/<name>` with one commit on `main` and a linked worktree
/// `<root>/<name>-wt` on `branch`.
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

/// The TUI under a pty: what it paints, read on a thread, and its keyboard.
struct Pty {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
    seen: String,
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
        }
    }

    /// Waits until `what` was painted `count` times in total.
    fn wait(&mut self, what: &str, count: usize) {
        let deadline = Instant::now() + DEADLINE;
        while self.seen.matches(what).count() < count {
            let left = deadline.saturating_duration_since(Instant::now());
            let bytes = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("waiting for {what:?} x{count}: {:?}", self.seen));
            self.seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    }

    fn key(&mut self, bytes: &[u8]) {
        self.stdin.write_all(bytes).unwrap();
        self.stdin.flush().unwrap();
    }

    /// `q`, and everything the TUI painted until it left the alternate screen.
    fn quit(mut self) -> String {
        self.key(b"q");
        self.wait(LEAVE_ALT, 1);
        let status = self.child.wait().unwrap();
        while let Ok(bytes) = self.rx.recv_timeout(Duration::from_millis(0)) {
            self.seen.push_str(&String::from_utf8_lossy(&bytes));
        }
        drop(self.rx);
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        assert!(status.success(), "{status:?}: {:?}", self.seen);
        self.seen
    }
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// Escenario: Responder "s" observa el repo y lo muestra. In English the key is `y`, in
/// Spanish `s`; both are accepted in both languages.
#[test]
fn answering_yes_observes_the_repo_and_shows_its_worktrees() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (lang, question, key) in [("en", "[y/N]", b"y"), ("es", "[s/N]", b"s")] {
        let m = Machine::new(&["shop"]);
        let mut tui = m.tui(&m.notes, lang);
        tui.wait(question, 1);
        assert!(!m.notes_observed(), "{lang}: asking does not observe");
        tui.key(key);
        // Its linked worktree's branch: the fleet of "notes" is on screen.
        tui.wait("feat-notas", 1);
        tui.quit();
        assert!(m.notes_observed(), "{lang}: {:?}", m.observed());
    }
}

/// In a linked worktree (its `.git` is a file) the TUI offers the repo the worktree belongs
/// to, by that repo's name, and observing it observes that repo, not a new one.
#[test]
fn answering_yes_in_a_linked_worktree_observes_its_repo() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(&["shop"]);
    // From a folder inside the worktree: the TUI looks upwards for the `.git` file.
    let sub =
        m.f.root
            .join("notes-wt")
            .canonicalize()
            .unwrap()
            .join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let mut tui = m.tui(&sub, "en");
    tui.wait("[y/N]", 1);
    tui.wait("notes", 1);
    tui.key(b"y");
    tui.wait("feat-notas", 1);
    tui.quit();
    let observed = m.observed();
    assert!(m.notes_observed(), "{observed:?}");
    assert_eq!(observed.len(), 2, "{observed:?}");
    assert!(
        !observed.iter().any(|p| p.contains("notes-wt")),
        "{observed:?}"
    );
}

/// Escenario: En un repo no observado la TUI pregunta y por defecto no observa. Enter and
/// `n` say no; the TUI goes on to the observed repos (here two: it offers to choose).
#[test]
fn answering_no_lists_the_observed_repos() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for key in [&b"\r"[..], &b"n"[..]] {
        let m = Machine::new(&["shop", "api"]);
        let mut tui = m.tui(&m.notes, "en");
        tui.wait("[y/N]", 1);
        tui.key(key);
        tui.wait("choose", 1);
        tui.wait("shop", 1);
        tui.wait("api", 1);
        let seen = tui.quit();
        assert!(!seen.contains("raptor repo add`"), "{seen:?}");
        assert!(!m.notes_observed(), "{key:?}: {:?}", m.observed());
    }
}

/// Escenario: Una TUI lanzada por un agente no ofrece observar. With "shop" as the only
/// observed repo it opens "shop" at once, as outside any repo; a `y` then does nothing.
#[test]
fn an_agent_is_never_asked() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(&["shop"]);
    let mut tui = m.tui_as_agent(&m.notes);
    tui.wait("feat-shop", 1);
    tui.key(b"y");
    let seen = tui.quit();
    assert!(!seen.contains("[y/N]"), "{seen:?}");
    assert!(!m.notes_observed(), "{:?}", m.observed());
}

/// Without a terminal `raptor` does not open the TUI (exit 2, ADR-CKP-003 § 11): nothing is
/// asked and nothing is observed.
#[test]
fn without_a_terminal_nothing_is_asked_nor_observed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(&["shop"]);
    let out = Command::new(RAPTOR)
        .env_clear()
        .envs(m.env("en"))
        .current_dir(&m.notes)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(!text(&out).contains("[y/N]"));
    assert!(!m.notes_observed(), "{:?}", m.observed());
}
