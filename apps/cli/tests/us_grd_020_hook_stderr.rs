//! US-GRD-020 end to end: what the hook writes to stderr when it denies an operation because the
//! Time Machine could not save its prior point and GitRaptor is not running. The denial names the
//! operation that was denied (E2 and E3 of the story) and asks to repeat that same one, and the
//! stderr carries nothing else of GitRaptor: no push hint and no minimum-set notice for a branch
//! deletion or a rebase.
//!
//! Same harness as `us_grd_017_hook.rs`: the real `raptor` as daemon and hook client, the real
//! dispatcher and plain Git, over a temporary repo, a bare remote and a temporary profile
//! (NFR-01); the agent is `raptor-fake-agent`. The language is set with `LANG`, the only
//! language variable the hook dispatcher forwards. No fixed waits.
//!
//! Unix only (Windows: pending cross-platform validation stage).
#![cfg(unix)]

use std::ffi::OsString;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, Snapshot};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
const DEADLINE: Duration = Duration::from_secs(60);
const API_RS: &str = "pub fn api() -> u32 { 42 }\n";

/// Entry point of the simulated Claude Code: when this binary runs as `raptor-fake-agent` with
/// [`AGENT_CMD`], it runs that command and exits with its status. As a normal test, nothing.
#[test]
fn fake_agent_entry() {
    let Some(cmd) = std::env::var_os(AGENT_CMD) else {
        return;
    };
    let status = Command::new("/bin/sh").arg("-c").arg(cmd).status().unwrap();
    std::process::exit(status.code().unwrap_or(2));
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// `raptor <args>` from the developer's own terminal: a pty through `script`.
fn developer_command(args: &[&str]) -> Command {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null", RAPTOR]).args(args);
        c
    } else {
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let line = std::iter::once(RAPTOR)
            .chain(args.iter().copied())
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new("script");
        c.args(["-q", "-e", "-c", &line, "/dev/null"]);
        c
    }
}

/// "demo" with `main` (newer than the branches); `feat-x` with 3 commits that are on no other
/// branch (the first adds `api.rs`); `feat-y` with 2 commits of its own, behind `main`; `ff`, a
/// commit ahead of the first `main`; a bare remote with `main`. Observed and protected by
/// `raptor guard install`, with the Time Machine ready.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    extra: Vec<(&'static str, OsString)>,
}

impl Machine {
    fn new(extra: &[(&'static str, &str)]) -> Self {
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
        let commit = |file: &str, content: &str, msg: &str| {
            f.write(file, content);
            f.git(&["add", file]);
            f.git(&["commit", "-q", "-m", msg]);
        };
        f.git(&["switch", "-q", "-c", "feat-x"]);
        commit("api.rs", API_RS, "add api");
        commit("x2.txt", "2\n", "x2");
        commit("x3.txt", "3\n", "x3");
        f.git(&["switch", "-q", "main"]);
        f.git(&["switch", "-q", "-c", "feat-y"]);
        commit("y1.txt", "1\n", "y1");
        commit("y2.txt", "2\n", "y2");
        f.git(&["switch", "-q", "main"]);
        commit("m.txt", "m\n", "m");
        f.git(&["switch", "-q", "-c", "ff"]);
        commit("ff.txt", "ff\n", "ff");
        f.git(&["switch", "-q", "main"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);

        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self {
            f,
            outside,
            extra: extra.iter().map(|(k, v)| (*k, OsString::from(v))).collect(),
        };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["guard", "install", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m.wait_tm_ready();
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        let mut env = vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "es_ES.UTF-8".into()),
        ];
        env.extend(self.extra.iter().cloned());
        env
    }

    fn developer(&self, args: &[&str]) -> Output {
        developer_command(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn git_line(&self, args: &str) -> String {
        format!("'{}' {args}", self.f.git.to_str().unwrap())
    }

    /// `git <args>` by the developer in the repo: no agent in the ancestry.
    fn human(&self, args: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(self.git_line(args))
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `git <args>` run by the simulated Claude Code in the repo.
    fn agent(&self, args: &str) -> Output {
        let line = self.git_line(args);
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            copy_executable(&std::env::current_exe().unwrap(), &agent);
        }
        Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_CMD, line)
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The tip of the local branch, `None` if it does not exist.
    fn branch(&self, name: &str) -> Option<String> {
        let out = self.human(&format!("rev-parse --verify -q refs/heads/{name}"));
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: Snapshot = self
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        snapshot.repos[0].repo_id.clone()
    }

    /// The Time Machine took its first point of the repo (its store is seeded).
    fn wait_tm_ready(&self) {
        self.f.write("notes.txt", "ready\n");
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id)
                && snapshot_refs(&store).is_ok_and(|r| !r.is_empty())
            {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "the Time Machine took no point");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&self) {
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

/// The lines of GitRaptor in `stderr`: Git's own `error:`, `fatal:` and `hint:` lines about the
/// refused hook are not the hook's text.
fn gitraptor_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(|l| l.trim_end().to_owned())
        .filter(|l| !l.is_empty())
        .filter(|l| {
            !["error:", "fatal:", "hint:", "warning:"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect()
}

const EN: [(&str, &str); 1] = [("LANG", "en_US.UTF-8")];

/// GRD020-DENY-DELETE: with GitRaptor stopped, the deletion of `feat-x` is refused with E2 and
/// only E2, in English and in Spanish.
#[test]
fn delete_denied_without_the_service_prints_only_e2() {
    for (extra, expected) in [
        (
            &EN[..],
            [
                "GitRaptor: branch feat-x was not deleted: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).",
                "To go on: start GitRaptor (`raptor daemon status` starts it) and delete the branch again; if it is already running, observe the repo again with raptor repo add.",
            ],
        ),
        (
            &[][..],
            [
                "GitRaptor: no se borró la rama feat-x: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).",
                "Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a borrar la rama; si ya está en marcha, vuelve a observar el repo con raptor repo add.",
            ],
        ),
    ] {
        let m = Machine::new(extra);
        let tip = m.branch("feat-x").unwrap();
        m.stop();

        let out = m.agent("branch -D feat-x");

        assert!(!out.status.success(), "the deletion ran: {}", text(&out));
        assert_eq!(gitraptor_lines(&out), expected, "{}", text(&out));
        assert_eq!(m.branch("feat-x").as_deref(), Some(tip.as_str()));
    }
}

/// GRD020-DENY-REBASE: with GitRaptor stopped, the rebase is refused with E3 and only E3.
#[test]
fn rebase_denied_without_the_service_prints_only_e3() {
    for (extra, expected) in [
        (
            &EN[..],
            [
                "GitRaptor: the rebase of feat-y did not run: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).",
                "To go on: start GitRaptor (`raptor daemon status` starts it) and run the rebase again; if it is already running, observe the repo again with raptor repo add.",
            ],
        ),
        (
            &[][..],
            [
                "GitRaptor: no se ejecutó el rebase de feat-y: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).",
                "Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a lanzar el rebase; si ya está en marcha, vuelve a observar el repo con raptor repo add.",
            ],
        ),
    ] {
        let m = Machine::new(extra);
        let tip = m.branch("feat-y").unwrap();
        m.stop();

        let out = m.agent("rebase -q main feat-y");

        assert!(!out.status.success(), "the rebase ran: {}", text(&out));
        assert_eq!(gitraptor_lines(&out), expected, "{}", text(&out));
        assert_eq!(m.branch("feat-y").as_deref(), Some(tip.as_str()));
    }
}

/// GRD020-DENY-PUSH: a force-push to `main` with GitRaptor stopped is still denied and its hint
/// still asks to push again, with the new wording about "a person's operation".
#[test]
fn push_denied_without_the_service_asks_to_push_again() {
    let m = Machine::new(&[]);
    let main = m.branch("main").unwrap();
    m.stop();

    let out = m.agent("push -q --force origin feat-x:main");

    assert!(!out.status.success(), "the push ran: {}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("vuelve a hacer el push"), "{shown}");
    assert!(
        shown.contains("la operación de una persona se distingue de la de un agente"),
        "{shown}"
    );
    assert!(!shown.contains("el push de una persona"), "{shown}");
    assert_eq!(m.branch("main").as_deref(), Some(main.as_str()));
}
