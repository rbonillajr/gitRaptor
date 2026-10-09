//! INF-CKP-001 at process level: `raptor` and `raptor tui` as a user runs
//! them, and the headless `App` against the real daemon. Temporary profile
//! (`GITRAPTOR_PROFILE_DIR`, debug builds only); never this repo nor the
//! real profile (NFR-01).
//!
//! The terminal tests need a pseudo-terminal: macOS `script`. Linux and
//! Windows: Pendiente: etapa de validación multiplataforma.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            ("GITRAPTOR_PROFILE_DIR", self.root().into_os_string()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn raptor(&self, args: &[&str]) -> Output {
        self.raptor_with(args, &[])
    }

    fn raptor_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .envs(extra.iter().map(|(k, v)| (*k, *v)))
            .current_dir(self.tmp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

#[cfg(unix)]
impl Drop for Fixture {
    fn drop(&mut self) {
        // A daemon started on demand is detached: never leave it behind. Ask it to end, and
        // if it is still there after a grace period, kill it.
        let dirs = gitraptor_core::profile::ProfileDirs::under_root(self.root());
        let Ok(Some(pid)) = gitraptor_core::daemon::running_pid(&dirs.state) else {
            return;
        };
        let pid = pid.to_string();
        let alive = || {
            Command::new("/bin/kill")
                .args(["-0", &pid])
                .status()
                .is_ok_and(|s| s.success())
        };
        let _ = Command::new("/bin/kill").arg(&pid).status();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if alive() {
            let _ = Command::new("/bin/kill").args(["-KILL", &pid]).status();
        }
    }
}

/// Without a terminal, `raptor` does not open the TUI: exit code 2 and a
/// hint (ADR-CKP-003 § 11). It never reaches the daemon.
#[test]
fn without_a_terminal_raptor_exits_2_and_suggests_status() {
    let fx = Fixture::new();
    for args in [&[][..], &["tui"][..]] {
        let out = fx.raptor(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("raptor status"), "{stderr}");
        assert!(out.stdout.is_empty());
    }
    assert!(
        !fx.root().join("run").exists(),
        "it must not start a daemon"
    );
}

// Debug builds only: in release the profile override is ignored (SEC-06).
#[cfg(all(target_os = "macos", debug_assertions))]
mod pty {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Child, ChildStdin, ExitStatus};
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    const ENTER_ALT: &str = "\u{1b}[?1049h";
    const LEAVE_ALT: &str = "\u{1b}[?1049l";

    /// Longest any step waits for the child before the test fails (never forever).
    const STEP: Duration = Duration::from_secs(30);

    /// `raptor` under `script` (a pty). Everything it does is bounded by a deadline and the
    /// child is killed when the session ends, however the test ends.
    struct Session {
        child: Child,
        stdin: Option<ChildStdin>,
        rx: Receiver<Vec<u8>>,
        seen: String,
    }

    impl Session {
        fn start(fx: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> Self {
            let mut child = Command::new("/usr/bin/script")
                .args(["-q", "/dev/null", RAPTOR])
                .args(args)
                .env_clear()
                .envs(fx.env())
                .envs(extra.iter().map(|(k, v)| (*k, *v)))
                .current_dir(fx.tmp.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let stdin = child.stdin.take();
            let mut stdout = child.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
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
                seen: String::new(),
            }
        }

        fn send(&mut self, bytes: &[u8]) {
            let stdin = self.stdin.as_mut().expect("stdin open");
            stdin.write_all(bytes).unwrap();
            stdin.flush().unwrap();
        }

        /// Reads until `what` has been seen `count` times; fails with everything read so far.
        fn wait_for(&mut self, what: &str, count: usize) {
            let deadline = Instant::now() + STEP;
            while self.seen.matches(what).count() < count {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.rx.recv_timeout(left) {
                    Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(e) => panic!("waiting for {what:?} x{count} ({e}): {:?}", self.seen),
                }
            }
        }

        /// Waits for the child to end (output closed, then exit status) within the deadline.
        fn finish(&mut self) -> ExitStatus {
            let deadline = Instant::now() + STEP;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.rx.recv_timeout(left) {
                    Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        panic!("the child did not close its output: {:?}", self.seen)
                    }
                }
            }
            while Instant::now() < deadline {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the child did not exit: {:?}", self.seen)
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // `script` leads its own pty session: take its child down first, then it.
            let pid = self.child.id().to_string();
            let _ = Command::new("/usr/bin/pkill")
                .args(["-KILL", "-P", &pid])
                .status();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// `raptor` in a terminal opens the TUI on the alternate screen and
    /// `q` leaves it restored. `q` goes in once the alternate screen is up: raw mode is on
    /// by then, so the key is not lost in the line discipline when the machine is slow.
    #[test]
    fn q_quits_and_restores_the_terminal() {
        let fx = Fixture::new();
        let mut s = Session::start(&fx, &[], &[]);
        s.wait_for(ENTER_ALT, 1);
        s.send(b"q");
        s.wait_for(LEAVE_ALT, 1);
        let status = s.finish();
        let text = &s.seen;
        assert!(status.success(), "{status:?}: {text:?}");
        let enter = text.find(ENTER_ALT).expect("alternate screen entered");
        let leave = text.rfind(LEAVE_ALT).expect("alternate screen left");
        assert!(enter < leave);
        assert!(text.contains("\u{1b}[?25h"), "cursor shown again");
    }

    /// `Ctrl-Z` hands the terminal back (leaves the alternate screen) and takes it again
    /// (enters it again), and the TUI goes on: `q` still quits. Under `script` the TUI leads
    /// an orphaned process group, so the system discards the stop and it resumes at once;
    /// the stop itself is the shell's job control. Waits on the output, never on a clock.
    #[test]
    fn ctrl_z_restores_and_reenters() {
        let fx = Fixture::new();
        let mut s = Session::start(&fx, &["tui"], &[]);
        // Raw mode is on before the alternate screen: from here the byte is a key, kept by
        // the terminal until the input thread reads it (not a SIGTSTP of the line discipline).
        s.wait_for(ENTER_ALT, 1);
        s.send(b"\x1a");
        s.wait_for(LEAVE_ALT, 1);
        s.wait_for(ENTER_ALT, 2);
        // The shell gets its cursor back while the TUI is suspended.
        let handed = s.seen.find(LEAVE_ALT).unwrap();
        let back = handed + s.seen[handed..].find(ENTER_ALT).unwrap();
        assert!(s.seen[handed..back].contains("\u{1b}[?25h"), "{:?}", s.seen);
        s.send(b"q");
        s.wait_for(LEAVE_ALT, 2);
        let status = s.finish();
        assert!(status.success(), "{status:?}: {:?}", s.seen);
    }

    /// A panic in the view leaves the terminal restored: the panic hook of
    /// `ratatui::init()` leaves the alternate screen before the message.
    #[test]
    fn a_panic_in_the_view_restores_the_terminal() {
        let fx = Fixture::new();
        let mut s = Session::start(&fx, &["tui"], &[("GITRAPTOR_TUI_PANIC_IN_VIEW", "1")]);
        let status = s.finish();
        let text = &s.seen;
        assert!(!status.success());
        let enter = text.find(ENTER_ALT).expect("alternate screen entered");
        let leave = text[enter..].find(LEAVE_ALT).expect("restored") + enter;
        let panic = text
            .find("GITRAPTOR_TUI_PANIC_IN_VIEW")
            .expect("panic message");
        assert!(leave < panic, "restored before the panic message: {text:?}");
        // The panic hit the first frame, before the channel thread started.
        assert!(!fx.root().join("run").exists());
    }
}

/// `--lang` beats `GITRAPTOR_LANG`, which beats the locale; an unknown `GITRAPTOR_LANG`
/// falls back to the locale and an unknown `--lang` is refused by the command line
/// (ADR-CKP-003 § 10). The locale of the fixture is English.
#[test]
fn lang_flag_beats_the_locale() {
    const ES: &str = "el cockpit necesita una terminal";
    const EN: &str = "the cockpit needs a terminal";
    let fx = Fixture::new();
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case; 6] = [
        (&["--lang", "es"], &[], ES),
        (&["tui", "--lang", "es"], &[], ES),
        (&["tui"], &[("GITRAPTOR_LANG", "es")], ES),
        (&["--lang", "en", "tui"], &[("GITRAPTOR_LANG", "es")], EN),
        (&["tui"], &[("GITRAPTOR_LANG", "fr")], EN),
        (
            &["tui"],
            &[("GITRAPTOR_LANG", "fr"), ("LANG", "es_ES.UTF-8")],
            ES,
        ),
    ];
    for (args, env, expected) in cases {
        let out = fx.raptor_with(args, env);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?} {env:?}: {stderr}");
        assert!(stderr.contains(expected), "{args:?} {env:?}: {stderr}");
    }
    let out = fx.raptor(&["--lang", "fr", "tui"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("expected `en` or `es`"), "{stderr}");
    assert!(!stderr.contains(EN) && !stderr.contains(ES), "{stderr}");
}

/// Two headless TUIs on the same real daemon reach "live" and see the same
/// state (V11). The daemon runs in the temporary profile.
#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn two_headless_tuis_see_the_same_engine() {
    use std::time::{Duration, Instant};

    use gitraptor_api::messages::ClientKind;
    use gitraptor_cli::client;
    use gitraptor_cli::client::engine::EngineConnector;
    use gitraptor_cli::model::{ConnState, Model, Size};
    use gitraptor_cli::present::i18n::Lang;
    use gitraptor_cli::queue;
    use gitraptor_cli::tui::app::App;
    use gitraptor_core::client::{ClientOptions, Launcher};
    use gitraptor_core::profile::ProfileDirs;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let fx = Fixture::new();
    // Start the daemon in the temporary profile; the TUIs never start one.
    let status = fx.raptor(&["daemon", "status"]);
    assert!(status.status.success(), "{status:?}");

    let mut apps = Vec::new();
    for _ in 0..2 {
        let mut options = ClientOptions::new(ProfileDirs::under_root(fx.root()), ClientKind::Cli);
        options.launcher = Launcher::Never;
        let (inbox, _input, engine) = queue::inbox();
        let model = Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        );
        let mut app = App::new(
            Terminal::new(TestBackend::new(80, 24)).unwrap(),
            model,
            inbox,
        );
        let channel = client::spawn(
            EngineConnector::new(options.connect().unwrap(), Box::new(options.launcher())),
            None,
            engine,
        );
        app.attach(channel.cmds.clone());
        apps.push((app, channel));
    }
    let start = Instant::now();
    while !apps
        .iter()
        .all(|(app, _)| app.model.conn == ConnState::Live && app.model.engine.all_synced())
    {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{:?}",
            apps.iter().map(|(a, _)| a.model.conn).collect::<Vec<_>>()
        );
        for (app, _) in &mut apps {
            app.step(Duration::from_millis(10)).unwrap();
        }
    }
    let first = apps[0].0.model.engine.global.data.clone();
    assert!(first.is_some());
    assert_eq!(apps[1].0.model.engine.global.data, first);
    // Not attributed to an agent: this test runs as the developer.
    assert!(apps[0].0.model.engine.requester.is_some());
    for (_, channel) in apps {
        channel.shutdown();
    }
}
