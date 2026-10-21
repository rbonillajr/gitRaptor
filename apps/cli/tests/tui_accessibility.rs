//! US-CKP-005 end to end: the real `raptor` under a pty (`script`), against the real daemon on a
//! temporary profile and a temporary repo `shop` with a linked worktree `feat-pagos` (INF-GRP-001).
//! Never this repo nor the real profile (NFR-01).
//!
//! No fixed waits: each key is written once what it needs is on the pty's output, with a
//! deadline. The renderer skips cells that did not change, so only words without spaces are
//! looked for.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const DEADLINE: Duration = Duration::from_secs(30);
const LEAVE_ALT: &str = "\u{1b}[?1049l";
/// The pty of `script` starts with no size, and the TUI would paint nothing: give it one
/// first, then become `raptor` with the arguments after it.
const SIZED: &str = "stty rows 30 cols 100 && exec \"$0\" \"$@\"";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// The machine: "shop" observed, with a linked worktree on `feat-pagos`.
struct Machine {
    f: Fixture,
    shop: PathBuf,
}

impl Machine {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let shop = repo(&f, "shop", "feat-pagos");
        let m = Self { f, shop };
        let out = Command::new("/usr/bin/script")
            .args([
                "-q",
                "/dev/null",
                RAPTOR,
                "repo",
                "add",
                m.shop.to_str().unwrap(),
            ])
            .env_clear()
            .envs(m.env("en"))
            .current_dir(&m.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
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
            // No process of this machine counts as an agent: the developer is the requester
            // even when the tests run from inside an agent's terminal.
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-no-such-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", locale.into()),
            ("TERM", "xterm-256color".into()),
        ]
    }

    /// `raptor <args>` (the TUI) from the developer's own terminal, inside "shop".
    fn tui(&self, lang: &str, args: &[&str]) -> Pty {
        let child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh", "-c", SIZED, RAPTOR])
            .args(args)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(&self.shop)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Pty::new(child)
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

/// The text on the screen after replaying what the TUI painted. ratatui repaints only the
/// cells that changed, so a word already on screen (the `n` of `feat-notas` after a `no`)
/// is not sent again and never appears whole in the byte stream: a wait for it must look at
/// the screen, not at the bytes. Only what those waits need is modeled: cursor positioning,
/// printable text, and the erasures; colors and every other sequence are dropped.
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
                        // OSC: up to BEL or ST (`ESC \`).
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

    fn contains(&self, what: &str) -> bool {
        self.rows
            .iter()
            .any(|r| r.iter().collect::<String>().contains(what))
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

    /// Waits until `what` was painted `count` times in total; a single one is also met by it
    /// being on the screen, though it was painted in pieces.
    fn wait(&mut self, what: &str, count: usize) {
        let deadline = Instant::now() + DEADLINE;
        while self.seen.matches(what).count() < count && !(count == 1 && self.screen.contains(what))
        {
            let left = deadline.saturating_duration_since(Instant::now());
            let bytes = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("waiting for {what:?} x{count}: {:?}", self.seen));
            let bytes = String::from_utf8_lossy(&bytes);
            self.screen.feed(&bytes);
            self.seen.push_str(&bytes);
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

/// Scenario 5 (D5): `?` paints the help ("Siempre"); Esc closes it, and `?` paints it again;
/// `?` and `?` open and close it, and paint it a third time.
#[test]
fn question_mark_opens_and_closes_the_help() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let mut tui = m.tui("es", &[]);
    tui.wait("feat-pagos", 1);
    tui.key(b"?");
    tui.wait("Siempre", 1);
    // Esc closes it (so `?` opens it again: a second painting).
    tui.key(b"\x1b");
    tui.key(b"?");
    tui.wait("Siempre", 2);
    // `?` closes it and `?` opens it again: a third one.
    tui.key(b"?");
    tui.key(b"?");
    tui.wait("Siempre", 3);
    tui.quit();
}

/// Scenario 5 (D3, D6): with `--ascii` every character of the output is ASCII, a letter or
/// `¿`/`¡`, in English and in Spanish.
#[test]
fn ascii_paints_only_ascii() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for lang in ["en", "es"] {
        let m = Machine::new();
        let mut tui = m.tui(lang, &["--ascii"]);
        tui.wait("feat-pagos", 1);
        let seen = tui.quit();
        let odd: Vec<char> = seen
            .chars()
            .filter(|c| !(c.is_ascii() || c.is_alphabetic() || *c == '¿' || *c == '¡'))
            .collect();
        assert!(odd.is_empty(), "{lang}: {odd:?} in {seen:?}");
    }
}
