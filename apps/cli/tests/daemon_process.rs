//! TS-GRP-003 at process level: the real `raptor daemon` binary with a
//! temporary profile (`GITRAPTOR_PROFILE_DIR`, honored only in debug builds)
//! and temporary repos. Never this repo nor the real profile (NFR-01).
//!
//! Unix only: signals and `kill -9`. Windows is not verified from macOS.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::{EXIT_ALREADY_RUNNING, LOG_FILE};
use gitraptor_core::profile::{Agent, AgentKind, Origin, Profile, ProfileDirs, WriteOp};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// Executable name of the simulated agent (`GITRAPTOR_AGENT_EXECUTABLES`).
const FAKE_AGENT: &str = "raptor-fake-agent";

/// `argv` run inside a pseudo-terminal with `script`.
fn in_pty(argv: &[&str]) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("/usr/bin/script");
        cmd.arg("-q").arg("/dev/null").args(argv);
        cmd
    }
    // Pendiente: etapa de validación multiplataforma (util-linux syntax).
    #[cfg(not(target_os = "macos"))]
    {
        let mut cmd = Command::new("script");
        cmd.args(["-qec", &argv.join(" "), "/dev/null"]);
        cmd
    }
}

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn profile_root(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(self.profile_root())
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// A repo whose files hold `content`, added to the profile.
    fn add_repo(&self, name: &str, content: &str) -> PathBuf {
        let repo = self.tmp.path().join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join(".env"), content).unwrap();
        std::fs::write(repo.join("README"), content).unwrap();
        git(&repo, &["add", "README"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let common = PathBuf::from(git(
            &repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ));
        let (mut profile, _) = Profile::open(self.dirs()).unwrap();
        profile.add_repo(&common, None, 1).unwrap();
        repo
    }

    /// Starts an active agent session in the first observed repo.
    fn start_session(&self, worktree: &Path) {
        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let entry = profile.repos().unwrap().remove(0);
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let worktree = gitraptor_core::observe::canonical(worktree);
        store
            .write_batch(&[
                WriteOp::UpsertWorktree {
                    path: worktree.clone(),
                    admin_name: None,
                    seen_ms: 1,
                },
                WriteOp::StartSession {
                    session_id: "s1".into(),
                    worktree,
                    agent: Agent {
                        kind: AgentKind::ClaudeCode,
                        name: None,
                    },
                    origin: Origin::Detected,
                    detection_key: None,
                    started_ms: 1,
                },
            ])
            .unwrap();
    }

    /// `raptor <args>` with exactly `env` plus the profile override and the
    /// agent classifier narrowed to a simulated agent (debug builds only),
    /// so the Claude Code session that may run these tests is not taken for
    /// an agent.
    fn command(&self, args: &[&str], env: &[(&str, OsString)]) -> Command {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", self.profile_root())
            .env("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    fn spawn_daemon(&self, env: &[(&str, OsString)]) -> Child {
        self.command(&["daemon"], env).spawn().unwrap()
    }

    /// `raptor daemon stop --yes` as the developer: in a pseudo-terminal, so
    /// it has a controlling terminal (ADR-GRP-005 § 6), and not under an
    /// agent. Goes through the channel as a reserved command.
    fn stop(&self) -> Output {
        let mut cmd = in_pty(&[RAPTOR, "daemon", "stop", "--yes"]);
        cmd.env_clear()
            .env("GITRAPTOR_PROFILE_DIR", self.profile_root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.output().unwrap()
    }

    /// Waits until the log has `n` lines containing `needle`.
    fn wait_log(&self, needle: &str, n: usize) -> String {
        let start = Instant::now();
        loop {
            let log = self.log();
            if log.matches(needle).count() >= n {
                return log;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "timed out waiting for {needle:?}; log:\n{log}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn normal_path() -> Vec<(&'static str, OsString)> {
    vec![("PATH", std::env::var_os("PATH").unwrap_or_default())]
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git must be installed");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn wait_exit(child: &mut Child) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "daemon did not exit"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Every file under `dir` with its content and modification time.
fn fingerprint(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let content = if meta.is_dir() {
                stack.push(path.clone());
                Vec::new()
            } else {
                std::fs::read(&path).unwrap()
            };
            out.insert(path, (content, meta.modified().unwrap()));
        }
    }
    out
}

fn observed_until(fx: &Fixture) -> Option<i64> {
    let (profile, _) = Profile::open(fx.dirs()).unwrap();
    let entry = profile.repos().unwrap().remove(0);
    let (store, _) = profile.open_store(&entry.repo_id).unwrap();
    store.observed_until().unwrap()
}

#[test]
fn single_instance_two_simultaneous_daemons() {
    let fx = Fixture::new();
    fx.add_repo("r", "hello\n");
    let mut a = fx.spawn_daemon(&normal_path());
    let mut b = fx.spawn_daemon(&normal_path());

    // Exactly one of them exits as "already running".
    let start = Instant::now();
    let (mut winner, loser_status) = loop {
        if let Some(status) = a.try_wait().unwrap() {
            break (b, status);
        }
        if let Some(status) = b.try_wait().unwrap() {
            break (a, status);
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no daemon gave up"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(loser_status.code(), Some(EXIT_ALREADY_RUNNING));
    let log = fx.wait_log("daemon_started", 1);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        fx.log().matches("daemon_started").count(),
        1,
        "only one daemon observes:\n{log}"
    );
    let winner_pid = winner.id().to_string();
    assert!(log.contains(&["pid=", &winner_pid].concat()), "{log}");

    assert!(fx.stop().status.success());
    assert!(wait_exit(&mut winner).success());
}

#[test]
fn daemon_stop_is_orderly() {
    let fx = Fixture::new();
    fx.add_repo("r", "hello\n");
    let mut daemon = fx.spawn_daemon(&normal_path());
    fx.wait_log("daemon_started state=observing", 1);

    let out = fx.stop();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(wait_exit(&mut daemon).success());

    // TS-GRP-004: the stop goes through the channel as a reserved command,
    // so it is an attributed stop with the client that asked for it.
    let log = fx.log();
    assert!(
        log.contains("reserved_command op=daemon.stop outcome=accepted"),
        "{log}"
    );
    assert!(
        log.contains("daemon_stopped cause=stop-command recorded=true"),
        "{log}"
    );
    let (profile, _) = Profile::open(fx.dirs()).unwrap();
    match profile.daemon_run().unwrap() {
        gitraptor_core::profile::DaemonRun::Stopped {
            cause,
            requested_by,
            ..
        } => {
            assert_eq!(cause, "stop-command");
            assert!(requested_by.unwrap().starts_with("client:"));
        }
        other => panic!("{other:?}"),
    }
    drop(profile);
    assert!(
        observed_until(&fx).is_some(),
        "observed-until not persisted"
    );
    // The lock is free: a new daemon starts at once.
    let mut again = fx.spawn_daemon(&normal_path());
    fx.wait_log("daemon_started", 2);
    assert!(fx.stop().status.success());
    assert!(wait_exit(&mut again).success());
}

#[test]
fn termination_signal_is_orderly() {
    let fx = Fixture::new();
    fx.add_repo("r", "hello\n");
    let mut daemon = fx.spawn_daemon(&normal_path());
    fx.wait_log("daemon_started", 1);
    let status = Command::new("/bin/kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(wait_exit(&mut daemon).success());
    assert!(
        fx.log()
            .contains("daemon_stopped cause=signal recorded=true signal=TERM")
    );
    assert!(observed_until(&fx).is_some());
}

#[test]
fn kill_9_during_active_session_is_marked_and_lock_recovers() {
    let fx = Fixture::new();
    let repo = fx.add_repo("r", "hello\n");
    fx.start_session(&repo);
    let mut daemon = fx.spawn_daemon(&normal_path());
    fx.wait_log("daemon_started", 1);
    daemon.kill().unwrap(); // SIGKILL
    daemon.wait().unwrap();

    let mut next = fx.spawn_daemon(&normal_path());
    let log = fx.wait_log("daemon_started", 2);
    assert!(
        log.contains("repo_pending_gap") && log.contains("cause=daemon-down-during-session"),
        "{log}"
    );
    assert!(log.contains("previous=crashed"), "{log}");
    assert!(fx.stop().status.success());
    assert!(wait_exit(&mut next).success());
}

#[test]
fn starts_with_an_empty_path() {
    let fx = Fixture::new();
    fx.add_repo("r", "hello\n");
    let mut daemon = fx.spawn_daemon(&[("PATH", OsString::new())]);
    let log = fx.wait_log("daemon_started", 1);
    assert!(
        log.contains("state=observing"),
        "Git must be found in well-known locations with an empty PATH:\n{log}"
    );
    assert!(fx.stop().status.success());
    assert!(wait_exit(&mut daemon).success());
}

#[test]
fn hostile_environment_does_not_change_behavior() {
    // Baseline.
    let base = Fixture::new();
    base.add_repo("r", "hello\n");
    let mut daemon = base.spawn_daemon(&normal_path());
    let baseline = base.wait_log("daemon_started", 1);
    assert!(base.stop().status.success());
    wait_exit(&mut daemon);
    let git_field = |log: &str| {
        log.lines()
            .find(|l| l.contains("daemon_started"))
            .and_then(|l| l.split(' ').find(|w| w.starts_with("git=")))
            .map(str::to_owned)
    };

    let fx = Fixture::new();
    fx.add_repo("r", "hello\n");
    // A fake `git` in a relative PATH entry leaves a marker if it ever runs.
    let cwd = fx.tmp.path().join("cwd");
    let marker = fx.tmp.path().join("fake-git-ran");
    std::fs::create_dir_all(&cwd).unwrap();
    let fake = cwd.join("git");
    std::fs::write(&fake, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let evil = fx.tmp.path().join("evil");
    std::fs::create_dir_all(&evil).unwrap();
    let path = std::env::join_paths(
        [PathBuf::from("."), PathBuf::from("bin")]
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let hostile: Vec<(&str, OsString)> = vec![
        ("PATH", path),
        ("GIT_EXEC_PATH", evil.clone().into()),
        ("GIT_DIR", evil.clone().into()),
        (
            "GIT_CONFIG_PARAMETERS",
            "'core.fsmonitor'='touch /tmp/x'".into(),
        ),
        ("LD_PRELOAD", evil.join("evil.so").into()),
        // A missing library would make dyld abort before `main`: that is the
        // start of the process itself, out of scope (TS-GRP-003). A harmless
        // system library checks the daemon still behaves the same.
        ("DYLD_INSERT_LIBRARIES", "/usr/lib/libz.1.dylib".into()),
        ("XDG_CONFIG_HOME", evil.clone().into()),
    ];
    let mut daemon = fx
        .command(&["daemon"], &hostile)
        .current_dir(&cwd)
        .spawn()
        .unwrap();
    let log = fx.wait_log("daemon_started", 1);
    assert!(fx.stop().status.success());
    assert!(wait_exit(&mut daemon).success());

    assert!(log.contains("state=observing"), "{log}");
    assert_eq!(
        git_field(&log),
        git_field(&baseline),
        "same Git as without the hostile env"
    );
    assert!(!marker.exists(), "a git from a relative PATH entry ran");
    assert_eq!(
        std::fs::read_dir(&evil).unwrap().count(),
        0,
        "something wrote to the hostile dirs"
    );
}

#[test]
fn logs_never_contain_content_secrets_or_paths() {
    let fx = Fixture::new();
    let content = "CONTENT_MARK_7f3a API_KEY=sk-planted\n";
    let repo = fx.add_repo("repo-PATHMARK", content);
    let mut env = normal_path();
    env.push(("AWS_SECRET_ACCESS_KEY", "ENV_MARK_91c2".into()));
    env.push(("GITHUB_TOKEN", "ghp_ENV_MARK_55d1".into()));
    let daemon = fx.spawn_daemon(&env);
    fx.wait_log("daemon_started", 1);
    assert!(fx.stop().status.success());
    let out = daemon.wait_with_output().unwrap();
    assert!(out.status.success());

    let mut haystacks = vec![("stdout", out.stdout), ("stderr", out.stderr)];
    for entry in std::fs::read_dir(fx.dirs().state).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(LOG_FILE)
        {
            haystacks.push(("log", std::fs::read(&path).unwrap()));
        }
    }
    for (what, bytes) in haystacks {
        let text = String::from_utf8_lossy(&bytes);
        for mark in ["CONTENT_MARK", "sk-planted", "ENV_MARK", "PATHMARK"] {
            assert!(!text.contains(mark), "{what} contains {mark}:\n{text}");
        }
        assert!(
            !text.contains(repo.to_str().unwrap()),
            "{what} has the repo path"
        );
    }
}

#[test]
fn repo_and_outside_stay_intact() {
    let fx = Fixture::new();
    let repo = fx.add_repo("r", "hello\n");
    let before = fingerprint(&repo);
    let siblings_before: Vec<_> = std::fs::read_dir(fx.tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut daemon = fx.spawn_daemon(&normal_path());
    fx.wait_log("daemon_started", 1);
    assert!(fx.stop().status.success());
    wait_exit(&mut daemon);

    assert_eq!(fingerprint(&repo), before, "the observed repo changed");
    let siblings_after: Vec<_> = std::fs::read_dir(fx.tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        siblings_after, siblings_before,
        "something was written outside the profile"
    );
    // Lock, log and state only in the state folder of the profile.
    let state = fx.dirs().state;
    for name in ["daemon.lock", LOG_FILE] {
        assert!(state.join(name).is_file(), "{name} missing in state folder");
    }
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["daemon.lock", LOG_FILE] {
            let mode = std::fs::metadata(state.join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}

#[test]
fn stop_without_a_daemon_says_not_running() {
    let fx = Fixture::new();
    let out = fx.stop();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("not running"));
}
