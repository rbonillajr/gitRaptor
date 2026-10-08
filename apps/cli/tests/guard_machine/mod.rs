//! The temporary machine of the Guardrails end-to-end suites of US-GRD-002 and US-GRD-003: the
//! repo "demo" with `main` and `feat-x`, both pushed to a bare remote, observed by a daemon of a
//! temporary profile (INF-GRP-001, INF-GRD-001; never this repo nor the real profile, NFR-01).
//! The developer answers from their own terminal (a pty through `script`); an agent runs the
//! same command as a child of the simulated agent. No fixed waits: every step waits on an
//! explicit signal (a process exit, a text on the terminal, the daemon's pid file).
#![allow(dead_code)]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use gitraptor_api::guard::GuardStatus;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture};

pub const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
pub const FAKE_AGENT: &str = "raptor-fake-agent";
pub const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";

/// Entry point of the simulated agent: when the test binary runs as `raptor-fake-agent` with
/// `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child and exits with its status. Each
/// suite calls it from a `#[test] fn fake_agent_entry`.
pub fn fake_agent_entry() {
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

pub fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// The argv of `raptor <args>` from a terminal: a pty through `script`.
fn script_argv(args: &[OsString]) -> Vec<OsString> {
    if cfg!(target_os = "macos") {
        let mut v: Vec<OsString> = vec!["/usr/bin/script".into(), "-q".into(), "/dev/null".into()];
        v.push(RAPTOR.into());
        v.extend(args.iter().cloned());
        v
    } else {
        // util-linux: the command is one shell word list.
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let line = std::iter::once(RAPTOR.to_owned())
            .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
            .map(|a| quote(&a))
            .collect::<Vec<_>>()
            .join(" ");
        vec![
            "script".into(),
            "-q".into(),
            "-e".into(),
            "-c".into(),
            line.into(),
            "/dev/null".into(),
        ]
    }
}

/// A command running in the developer's terminal, read as it writes.
pub struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
    pub seen: Vec<u8>,
}

impl Running {
    /// Waits until the terminal shows `marker` (the explicit signal).
    pub fn wait_for(&mut self, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !String::from_utf8_lossy(&self.seen).contains(marker) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(_) => panic!(
                    "no {marker:?} on the terminal: {}",
                    String::from_utf8_lossy(&self.seen)
                ),
            }
        }
    }

    pub fn type_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    pub fn finish(mut self) -> Output {
        let status: ExitStatus = self.child.wait().unwrap();
        drop(self.stdin.take());
        if let Some(r) = self.reader.take() {
            r.join().unwrap();
        }
        self.seen.extend(self.rx.try_iter().flatten());
        let mut stderr = Vec::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_end(&mut stderr);
        }
        Output {
            status,
            stdout: self.seen,
            stderr,
        }
    }
}

pub struct Machine {
    pub f: Fixture,
    pub remote: PathBuf,
    /// Length of the cancellable window of the daemon (debug builds only).
    pub window_ms: u64,
    /// Outside the fixture root, so the simulated agent is not part of the fingerprint.
    agent_dir: tempfile::TempDir,
    /// Extra variables for the next commands (a cut point of the `chaos` feature).
    pub extra_env: std::cell::RefCell<Vec<(&'static str, OsString)>>,
}

impl Machine {
    pub fn new() -> Self {
        Self::with_window(200)
    }

    pub fn with_window(window_ms: u64) -> Self {
        // Without debug assertions `raptor` ignores GITRAPTOR_PROFILE_DIR and would use the
        // real profile (NFR-01): refuse to run.
        #[cfg(not(debug_assertions))]
        panic!(
            "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
        );
        let f = Fixture::with_commit(&git_from_path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.write("x.txt", "x\n");
        f.git(&["add", "x.txt"]);
        f.git(&["commit", "-q", "-m", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        f.git(&["switch", "-q", "main"]);
        Self {
            f,
            remote,
            window_ms,
            agent_dir: tempfile::tempdir().unwrap(),
            extra_env: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    pub fn common(&self) -> PathBuf {
        self.f.repo.join(".git")
    }

    pub fn env(&self) -> Vec<(&'static str, OsString)> {
        let mut env = self.base_env();
        env.extend(self.extra_env.borrow().iter().cloned());
        env
    }

    fn base_env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            // Debug builds only: the agents the daemon knows; none of this test's ancestors.
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            // Debug builds only: the daemon uses the Git the repos are created with.
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            // Debug builds only: the cancellable window of the reserved actions that relax.
            (
                "GITRAPTOR_TEST_GUARD_WINDOW_MS",
                self.window_ms.to_string().into(),
            ),
            // Debug builds only: how often the daemon checks that the protection is still in
            // place (US-GRD-004), instead of every minute.
            ("GITRAPTOR_TEST_GUARD_HEALTH_MS", "100".into()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// Starts `raptor <args>` in the developer's terminal.
    pub fn developer_spawn(&self, args: &[&str]) -> Running {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let argv = script_argv(&args);
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Running {
            child,
            stdin,
            rx,
            reader: Some(reader),
            seen: Vec::new(),
        }
    }

    /// `raptor <args>` from the developer's terminal, typing `answer` once `prompt` shows.
    pub fn developer_answering(&self, args: &[&str], prompt: &str, answer: &str) -> Output {
        let mut run = self.developer_spawn(args);
        run.wait_for(prompt);
        run.type_line(answer);
        run.finish()
    }

    /// `raptor <args>` from the developer's terminal, with nothing typed.
    pub fn developer(&self, args: &[&str]) -> Output {
        self.developer_spawn(args).finish()
    }

    /// `raptor <args>` without a terminal.
    pub fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `raptor <args>` in a terminal, as a child of the simulated agent.
    pub fn as_agent(&self, args: &[&str]) -> Output {
        let agent = self.agent_dir.path().join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let argv: Vec<String> = script_argv(&args)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_AGENT_ARGV, serde_json::to_string(&argv).unwrap())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }

    pub fn add(&self, path: &Path) {
        let out = self.developer(&["repo", "add", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    pub fn protect(&self, path: &Path) -> Output {
        self.developer(&["guard", "install", "--yes", path.to_str().unwrap()])
    }

    pub fn uninstall(&self, path: &Path) -> Output {
        self.developer(&["guard", "uninstall", "--yes", path.to_str().unwrap()])
    }

    pub fn status(&self, path: &Path) -> GuardStatus {
        let out = self.raptor(&["guard", "status", "--json", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// The `guard.status` JSON as is (fields of newer capabilities included).
    pub fn status_json(&self, path: &Path) -> serde_json::Value {
        let out = self.raptor(&["guard", "status", "--json", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Plain Git in `dir`, with the hooks active, as an agent would run it.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Output {
        self.f.git_command(dir, args).output().unwrap()
    }

    pub fn git_ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git(dir, args);
        assert!(out.status.success(), "git {args:?}: {}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    pub fn remote_ref(&self, name: &str) -> Option<String> {
        let out = self.git(&self.remote, &["rev-parse", "--verify", "-q", name]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Rewrites `feat-x` locally so pushing it needs a force.
    pub fn rewrite_feat_x(&self) {
        self.git_ok(&self.f.repo, &["switch", "-q", "feat-x"]);
        self.git_ok(
            &self.f.repo,
            &["commit", "-q", "--amend", "-m", "feat-x rewritten"],
        );
    }

    /// An executable script at `path` (a hook of the developer).
    pub fn script(&self, path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "daemon did not stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// What an install may change: the key and the folder in the repo, and the profile.
pub fn install_exceptions() -> Exceptions {
    Exceptions::guardrails_install("repo", Path::new(".git"))
        .and(Exceptions::engine_profile("profile"))
}

/// After an uninstall, against the snapshot before the install: only the times of the common
/// dir and the identity of the rewritten `config` (whose content is compared byte by byte by
/// the suite), plus the profile and the channel socket of the daemon.
pub fn uninstalled_exceptions() -> Exceptions {
    Exceptions::guardrails_uninstalled("repo", Path::new(".git"))
        .and(Exceptions::engine_profile("profile"))
        .with(gitraptor_testkit::Exception::Subtree {
            scope: "profile".into(),
            prefix: "run".into(),
        })
}
