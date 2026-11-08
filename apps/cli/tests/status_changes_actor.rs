//! US-GRP-008 (T004): `raptor status` asks for `observation.change-attribution`
//! and shows who the uncommitted changes of a worktree are attributed to: in
//! the text (key `status.changes-actor`, with the text of `actor.unattributed`,
//! en and es) and in `--json` (`changes_actor`, absent without changes). The
//! real `raptor` binary as daemon and client over a temporary machine
//! (INF-GRP-001): temporary repos and profile, never this repo nor the real
//! profile (NFR-01). The field is read from the JSON on purpose.
//!
//! A daemon from before the capability cannot be started from here (the
//! daemon serves every capability of its own binary), so that case stays with
//! `crates/core/tests/change_attribution.rs`; `repo.add` is reserved to a
//! controlling terminal and a test client has none.
//!
//! macOS only: the developer runs reserved commands under a pty (`script`).
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

struct Machine {
    f: Fixture,
}

impl Machine {
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
            // Only a simulated agent counts as one, so these tests also run
            // under a real agent.
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn raptor(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// The developer, from their own terminal.
    fn developer(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn add(&self, repo: &Path) {
        let out = self.developer(&["repo", "add", repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// The worktree of `path` in `status --json`, once it lists a change.
    fn dirty_worktree(&self, path: &Path) -> Value {
        let start = Instant::now();
        loop {
            let status = self.status();
            let found = worktree_in(&status, path);
            if found
                .as_ref()
                .is_some_and(|w| w["changes"].as_array().is_some_and(|c| !c.is_empty()))
            {
                return found.unwrap();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{}: {status:#}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(100));
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
            assert!(start.elapsed() < Duration::from_secs(10));
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

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

fn worktree_in(status: &Value, path: &Path) -> Option<Value> {
    status["repos"]
        .as_array()?
        .iter()
        .flat_map(|r| r["worktrees"].as_array().into_iter().flatten())
        .find(|w| w["path"] == path.to_str().unwrap())
        .cloned()
}

/// `demo` repo: `main` with `login.txt`, a linked worktree "feat-login"
/// (clean) and an untracked `pagos.txt` in the main worktree (dirty).
fn demo() -> (Machine, PathBuf, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let linked = f
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();
    f.write("pagos.txt", "x\n");
    let main = f.repo.canonicalize().unwrap();
    (Machine::new(f), main, linked)
}

/// The catalog of the CLI: `key = text` lines of every file of `lang`.
fn catalog(lang: &str, key: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("i18n")
        .join(lang);
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let content = std::fs::read_to_string(entry.path()).unwrap();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once(" = ")
                && k == key
            {
                return v.to_owned();
            }
        }
    }
    panic!("the {lang} catalog has no key {key}");
}

/// The line of changes `raptor status` shows for one untracked file, built
/// from the catalog: `status.changes-actor` filled with `status.changes` and
/// the text of `actor.unattributed`.
fn expected_changes_line(lang: &str) -> String {
    let changes = catalog(lang, "status.changes")
        .replace("{total}", "1")
        .replace("{staged}", "0")
        .replace("{unstaged}", "0")
        .replace("{untracked}", "1");
    catalog(lang, "status.changes-actor")
        .replace("{changes}", &changes)
        .replace("{actor}", &catalog(lang, "actor.unattributed"))
}

/// The text of `raptor status` in `lang`, with the changes line of the
/// catalog (`status.changes-actor`) for the one untracked file.
fn text_shows_the_changes_actor(lang: &str, locale: &str) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, main, _linked) = demo();
    m.add(&m.f.repo);
    m.dirty_worktree(&main);
    let out = m.raptor(&["status"], &[("LANG", locale)]);
    let shown = text(&out);
    let expected = expected_changes_line(lang);
    assert!(
        shown.lines().any(|l| l.contains(&expected)),
        "the {lang} text must show the changes line `{expected}` (status.changes-actor):\n{shown}"
    );
    m.stop();
}

#[test]
fn status_shows_the_changes_actor_in_english_text() {
    text_shows_the_changes_actor("en", "en_US.UTF-8");
}

#[test]
fn status_shows_the_changes_actor_in_spanish_text() {
    text_shows_the_changes_actor("es", "es_ES.UTF-8");
}

/// `status --json`: `changes_actor` with the shape of `raptor events --json`
/// in the worktree with changes, absent in the clean one.
#[test]
fn status_shows_the_changes_actor_in_json_only_with_changes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, main, linked) = demo();
    m.add(&m.f.repo);
    let dirty = m.dirty_worktree(&main);
    assert_eq!(
        dirty["changes_actor"],
        json!({"actor": "unattributed"}),
        "status --json must carry changes_actor as the engine sends it: {dirty:#}"
    );
    let clean = worktree_in(&m.status(), &linked).expect("the linked worktree");
    assert_eq!(clean["changes"], json!([]), "{clean:#}");
    assert!(
        clean.get("changes_actor").is_none(),
        "a clean worktree must not carry changes_actor: {clean:#}"
    );
    m.stop();
}
