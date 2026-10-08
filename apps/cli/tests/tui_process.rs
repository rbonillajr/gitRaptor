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
        // A daemon started on demand is detached: never leave it behind.
        let dirs = gitraptor_core::profile::ProfileDirs::under_root(self.root());
        if let Ok(Some(pid)) = gitraptor_core::daemon::running_pid(&dirs.state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
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

    const ENTER_ALT: &str = "\u{1b}[?1049h";
    const LEAVE_ALT: &str = "\u{1b}[?1049l";

    fn script(fx: &Fixture, args: &[&str], extra: &[(&str, &str)], input: &str) -> Output {
        let mut child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR])
            .args(args)
            .env_clear()
            .envs(fx.env())
            .envs(extra.iter().map(|(k, v)| (*k, *v)))
            .current_dir(fx.tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_owned();
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            // Let the TUI enter raw mode first.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = stdin.write_all(input.as_bytes());
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        out
    }

    /// `raptor` in a terminal opens the TUI on the alternate screen and
    /// `q` leaves it restored.
    #[test]
    fn q_quits_and_restores_the_terminal() {
        let fx = Fixture::new();
        let out = script(&fx, &[], &[], "q");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}");
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
        use std::io::{Read, Write};
        use std::sync::mpsc;

        let fx = Fixture::new();
        let mut child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "tui"])
            .env_clear()
            .envs(fx.env())
            .current_dir(fx.tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
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
        let mut seen = String::new();
        let wait_for = |seen: &mut String, what: &str, count: usize| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while seen.matches(what).count() < count {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                let bytes = rx
                    .recv_timeout(left)
                    .unwrap_or_else(|_| panic!("waiting for {what:?} x{count}: {seen:?}"));
                seen.push_str(&String::from_utf8_lossy(&bytes));
            }
        };
        // Raw mode is on before the alternate screen: from here the byte is a key, kept by
        // the terminal until the input thread reads it (not a SIGTSTP of the line discipline).
        wait_for(&mut seen, ENTER_ALT, 1);
        stdin.write_all(b"\x1a").unwrap();
        wait_for(&mut seen, LEAVE_ALT, 1);
        wait_for(&mut seen, ENTER_ALT, 2);
        // The shell gets its cursor back while the TUI is suspended.
        let handed = seen.find(LEAVE_ALT).unwrap();
        let back = handed + seen[handed..].find(ENTER_ALT).unwrap();
        assert!(seen[handed..back].contains("\u{1b}[?25h"), "{seen:?}");
        stdin.write_all(b"q").unwrap();
        wait_for(&mut seen, LEAVE_ALT, 2);
        let status = child.wait().unwrap();
        drop(rx);
        reader.join().unwrap();
        assert!(status.success(), "{status:?}: {seen:?}");
    }

    /// A panic in the view leaves the terminal restored: the panic hook of
    /// `ratatui::init()` leaves the alternate screen before the message.
    #[test]
    fn a_panic_in_the_view_restores_the_terminal() {
        let fx = Fixture::new();
        let out = script(&fx, &["tui"], &[("GITRAPTOR_TUI_PANIC_IN_VIEW", "1")], "");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!out.status.success());
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
