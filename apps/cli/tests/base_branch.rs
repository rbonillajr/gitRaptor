//! US-GRP-012 end to end: each Gherkin scenario with the real `raptor`
//! binary as daemon and as client, over a temporary machine built by the
//! "intact repo" harness (INF-GRP-001): temporary repos, remote and
//! profile, never this repo nor the real profile (NFR-01).
//!
//! The developer adds repos from their own terminal (a pty, `script`).
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma. The same logic runs on
//! every OS in `crates/core/tests/base_branch.rs`.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// The temporary machine of one scenario.
struct Machine {
    f: Fixture,
}

impl Machine {
    /// The profile folders private, as the engine requires (SEC-06).
    fn new(f: Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { f }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            // The agents the daemon knows, debug builds only: none of this
            // test's ancestors (it may itself run under an agent).
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
        ]
    }

    /// `raptor <args>` without a terminal.
    fn raptor(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .envs(self.env())
            .envs(extra.iter().copied())
            .current_dir(&self.f.root)
            .stdin(Stdio::null());
        cmd.output().unwrap()
    }

    /// `raptor repo add <path>` by the developer, from their own terminal.
    fn add(&self, path: &Path) {
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "repo", "add"])
            .arg(path)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }

    /// `raptor status --json`, parsed.
    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// `raptor status` in `lang`.
    fn status_text(&self, lang: &str) -> String {
        let out = self.raptor(&["status"], &[("LANG", lang)]);
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "daemon", "stop", "--yes"])
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // A failed test must not leave a daemon behind (it is detached).
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

/// `n` empty commits in `dir`, each with its own message.
fn commit(f: &Fixture, dir: &Path, n: usize, tag: &str) {
    for i in 0..n {
        f.git_in(
            dir,
            &["commit", "-q", "--allow-empty", "-m", &format!("{tag} {i}")],
        );
    }
}

/// "demo" observed with no team settings: "feat-login" has 3 commits that
/// are not in `main`, and `main` 1 that "feat-login" lacks.
fn demo() -> (Machine, PathBuf) {
    let f = Fixture::with_commit(&git_from_path());
    f.git(&["branch", "feat-login"]);
    let wt = f.add_worktree("feat-login", "feat-login");
    commit(&f, &wt, 3, "login");
    commit(&f, &f.repo, 1, "main");
    (Machine::new(f), wt)
}

/// The `ahead_behind` of each worktree of the first repo.
fn ahead_behind(status: &Value) -> Vec<Value> {
    status["repos"][0]["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["ahead_behind"].clone())
        .collect()
}

fn counted(ahead: u64, behind: u64) -> Value {
    json!({"state": "counted", "ahead": ahead, "behind": behind, "exact": true})
}

/// The engine's own data, the only difference a scenario may show outside
/// the repos: its profile data and state, and the socket folder.
fn engine_profile() -> Exceptions {
    Exceptions::engine_profile("profile")
        .with(Exception::Subtree {
            scope: "profile".into(),
            prefix: "run".into(),
        })
        .with(Exception::DirTimes {
            scope: "profile".into(),
            path: PathBuf::new(),
        })
}

// ------------------------------------------------------------ Escenario 1

/// Sin configuración del equipo, la rama base es main.
#[test]
fn without_team_settings_the_base_branch_is_main() {
    let (m, _wt) = demo();
    m.add(&m.f.repo);

    let status = m.status();
    let repo = &status["repos"][0];
    assert_eq!(repo["base_branch"], "main", "{status}");
    // Nothing confirmed it: adding the repo never does (ADR-GRD-004 § 3.5).
    assert_eq!(repo["base_confirmed"], false, "{status}");
    assert_eq!(ahead_behind(&status), [counted(0, 0), counted(3, 1)]);

    let en = m.status_text("en_US.UTF-8");
    assert!(en.contains("base branch: main (unconfirmed)"), "{en}");
    assert!(en.contains("3 ahead and 1 behind main"), "{en}");
    let es = m.status_text("es_ES.UTF-8");
    assert!(es.contains("rama base: main (no confirmada)"), "{es}");
    assert!(es.contains("3 por delante y 1 por detrás de main"), "{es}");
    m.stop();
}

// ------------------------------------------------------------ Escenario 2

/// El ahead/behind se recalcula cuando avanza la rama base.
#[test]
fn ahead_behind_follows_the_base_branch() {
    let (m, _wt) = demo();
    m.add(&m.f.repo);
    assert_eq!(ahead_behind(&m.status()), [counted(0, 0), counted(3, 1)]);

    // A new commit on `main`, without adding the repo again.
    commit(&m.f, &m.f.repo, 1, "main again");
    let status = m.status();
    assert_eq!(
        ahead_behind(&status),
        [counted(0, 0), counted(3, 2)],
        "{status}"
    );
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// El motor no trae novedades del remoto.
#[test]
fn repo_intact_the_engine_does_not_fetch() {
    let (m, _wt) = demo();
    let f = &m.f;
    // A remote that has 2 commits on `main` the repo does not know yet.
    let origin = f.root.join("origin.git");
    f.git(&["init", "-q", "--bare", origin.to_str().unwrap()]);
    f.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    f.git(&["push", "-q", "origin", "main"]);
    f.git(&["fetch", "-q", "origin"]);
    let elsewhere = f.root.join("elsewhere");
    f.git(&[
        "clone",
        "-q",
        origin.to_str().unwrap(),
        elsewhere.to_str().unwrap(),
    ]);
    commit(f, &elsewhere, 2, "remote");
    f.git_in(&elsewhere, &["push", "-q", "origin", "main"]);
    let known = f.git(&["rev-parse", "refs/remotes/origin/main"]);
    let remote_main = f.git_in(&origin, &["rev-parse", "main"]);
    assert_ne!(known, remote_main);

    // The remote and the other clone move during the scenario: by this
    // test, not the engine. Everything else must stay as it was.
    let mut allowed = engine_profile();
    for scope in ["origin.git", "elsewhere"] {
        allowed = allowed.with(Exception::Subtree {
            scope: scope.into(),
            prefix: PathBuf::new(),
        });
    }
    let report = check("US-GRP-012 no fetch", f, &allowed, || {
        m.add(&f.repo);
        // Counted with what the repo already knows.
        assert_eq!(ahead_behind(&m.status()), [counted(0, 0), counted(3, 1)]);
        // The remote advancing again does not change it either.
        commit(f, &elsewhere, 1, "remote again");
        f.git_in(&elsewhere, &["push", "-q", "origin", "main"]);
        assert_eq!(ahead_behind(&m.status()), [counted(0, 0), counted(3, 1)]);
        m.stop();
    });
    report.assert_intact();
    // The known remote branches still point where they did.
    assert_eq!(f.git(&["rev-parse", "refs/remotes/origin/main"]), known);
}

// ------------------------------------------------------------ Escenario 4

/// Si la rama base no existe, el motor lo indica y no elige otra.
#[test]
fn a_missing_base_branch_is_reported_and_no_other_is_used() {
    let f = Fixture::new(&git_from_path());
    // "otro": its branches are `trunk` and "feat"; a known remote copy of
    // `main` and a tag `main` are other refs, never the base (Q42).
    let otro = f.other_repo.clone();
    f.git_in(&otro, &["checkout", "-q", "-b", "trunk"]);
    commit(&f, &otro, 2, "trunk");
    f.git_in(&otro, &["branch", "feat"]);
    let feat = f.root.join("wt-otro-feat");
    f.git_in(
        &otro,
        &["worktree", "add", "-q", feat.to_str().unwrap(), "feat"],
    );
    f.git_in(&otro, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
    f.git_in(&otro, &["tag", "main", "HEAD~1"]);
    let m = Machine::new(f);
    m.add(&otro);

    let status = m.status();
    assert_eq!(status["repos"][0]["base_branch"], "main", "{status}");
    let missing = json!({"state": "base-missing"});
    assert_eq!(
        ahead_behind(&status),
        [missing.clone(), missing],
        "{status}"
    );

    let en = m.status_text("en_US.UTF-8");
    assert_eq!(
        en.matches("no ahead/behind: base branch \"main\" does not exist in the repo")
            .count(),
        2,
        "{en}"
    );
    assert!(!en.contains("ahead and"), "{en}");
    let es = m.status_text("es_ES.UTF-8");
    assert!(
        es.contains("sin ahead/behind: la rama base \"main\" no existe en el repo"),
        "{es}"
    );
    m.stop();
}
