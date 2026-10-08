//! US-GRP-020 end to end: the TUI announces a repo discovered in a declared code folder and
//! `s` (or `y`) observes it. The developer runs the real binary under a pty (`script`), against
//! the real daemon on a temporary profile and temporary folders (INF-GRP-001). Never this repo
//! nor the real profile (NFR-01). The daemon lists every root every 100 ms
//! (`GITRAPTOR_TEST_DISCOVERY_POLL_MS`, debug builds only).
//!
//! No fixed waits: every key is written once its prompt is on the pty's output, and the engine
//! is polled with a deadline. The renderer skips cells that did not change, so only words
//! without spaces are looked for.
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
const DEADLINE: Duration = Duration::from_secs(30);
const LEAVE_ALT: &str = "\u{1b}[?1049l";
/// The pty of `script` starts with no size, and the TUI would paint nothing: give it one
/// first, then become `raptor` (same process, so the ancestry is unchanged).
const SIZED: &str = "stty rows 30 cols 100 && exec \"$0\"";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// The machine: "shop" observed (so the TUI opens its fleet), and a `code` folder that the
/// developer declares, where new repos appear.
struct Machine {
    f: Fixture,
    code: PathBuf,
}

impl Machine {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let code = f.root.join("code");
        std::fs::create_dir_all(&code).unwrap();
        let m = Self { f, code };
        let shop = m.f.root.join("shop");
        m.repo(&shop);
        let out = m.developer(&["repo", "add", shop.to_str().unwrap()], "en");
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["repo", "roots", "add", m.code.to_str().unwrap()], "en");
        assert!(out.status.success(), "{}", text(&out));
        m
    }

    /// A repo with one commit at `dir`.
    fn repo(&self, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        self.f.git_in(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("README.md"), "x\n").unwrap();
        self.f.git_in(dir, &["add", "README.md"]);
        self.f.git_in(dir, &["commit", "-q", "-m", "x"]);
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
            // The real agent session that may run these tests is not taken for one.
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            (
                "GITRAPTOR_TEST_DISCOVERY_HOME",
                self.f.home.clone().into_os_string(),
            ),
            ("GITRAPTOR_TEST_DISCOVERY_POLL_MS", "100".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", locale.into()),
            ("TERM", "xterm-256color".into()),
        ]
    }

    /// A one-shot command from the developer's own terminal.
    fn developer(&self, args: &[&str], lang: &str) -> Output {
        Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR])
            .args(args)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `raptor` (the TUI) from the developer's own terminal, outside every repo.
    fn tui(&self, lang: &str) -> Pty {
        let child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh", "-c", SIZED, RAPTOR])
            .env_clear()
            .envs(self.env(lang))
            .current_dir(&self.f.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Pty::new(child)
    }

    fn read(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env("en"))
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The folders of the observed repos, from `raptor status --json`.
    fn observed(&self) -> Vec<String> {
        let out = self.read(&["status", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        status["repos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_owned())
            .collect()
    }

    fn discovered(&self) -> String {
        text(&self.read(&["repo", "discovered"]))
    }

    /// Polls with a deadline until `condition` holds.
    fn eventually(&self, what: &str, mut condition: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !condition(self) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn observes(&self, name: &str) -> bool {
        self.observed()
            .iter()
            .any(|p| p.ends_with(&format!("/{name}")))
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

/// A11: a repo that appears in the declared folder while the TUI is open is asked about under
/// the fleet, which stays on screen; `y` (English) or `s` (Spanish) observes it, without
/// leaving the fleet of "shop".
#[test]
fn a_repo_created_in_the_code_folder_is_offered_and_yes_observes_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (lang, question, key) in [("en", "Observe fresh?", b"y"), ("es", "¿Observar fresh?", b"s")] {
        let m = Machine::new();
        let mut tui = m.tui(lang);
        // The fleet of the only observed repo, outside every repo.
        tui.wait("shop", 1);
        let fresh = m.code.join("fresh");
        m.repo(&fresh);
        tui.wait("fresh?", 1);
        tui.wait(question, 1);
        assert!(!m.observes("fresh"), "{lang}: asking does not observe");
        tui.key(key);
        m.eventually("fresh to be observed", |m| m.observes("fresh"));
        let seen = tui.quit();
        assert!(!seen.contains("raptor repo add`"), "{lang}: {seen:?}");
        assert!(
            !m.discovered().contains("fresh"),
            "{lang}: accepted, no longer a candidate"
        );
    }
}

/// Esc is later: nothing is decided, the repo is still listed, and the next TUI offers it
/// again; `n` then dismisses it for good.
#[test]
fn escape_leaves_it_for_later_and_no_dismisses_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let fresh = m.code.join("fresh");
    m.repo(&fresh);
    m.eventually("fresh to be discovered", |m| {
        m.discovered().contains("fresh")
    });

    let mut tui = m.tui("en");
    tui.wait("fresh?", 1);
    tui.key(b"\x1b");
    // Esc alone is read as a key once the terminal's escape delay passes: the panel goes away
    // and the quit key answers the fleet.
    tui.wait("shop", 1);
    tui.quit();
    assert!(!m.observes("fresh"), "{:?}", m.observed());
    assert!(m.discovered().contains("fresh"), "still pending");

    let mut tui = m.tui("en");
    tui.wait("fresh?", 1);
    tui.key(b"n");
    m.eventually("fresh to be dismissed", |m| {
        !m.discovered().contains("fresh")
    });
    tui.quit();
    assert!(!m.observes("fresh"), "{:?}", m.observed());
}
