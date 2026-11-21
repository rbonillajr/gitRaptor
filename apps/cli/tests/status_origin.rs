//! US-WTL-005 (T007): `raptor status` shows, for each linked worktree, its
//! origin (certain, probable or unknown) or "foreign, do not touch", in
//! English and Spanish; `--json` carries `origin` only when it exists; and
//! untrusted text keeps coming out sanitized (SEC-12). The real `raptor`
//! binary as daemon and client over a temporary machine (INF-GRP-001):
//! temporary repos and profile, never this repo nor the real profile
//! (NFR-01). The field is read from the JSON on purpose.
//!
//! Git refuses a branch name with a control character, so the untrusted text
//! that reaches the status is a worktree folder with an escape sequence.
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

/// The worktree of `path` in `status --json`, once the engine has read it.
fn read_worktree(m: &Machine, path: &Path) -> Value {
    let start = Instant::now();
    loop {
        let status = m.status();
        if let Some(w) = worktree_in(&status, path)
            && w["main"] == false
            && w["unavailable"].is_null()
        {
            return w;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{}: {status:#}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Shop {
    m: Machine,
    orca: PathBuf,
    trunk: PathBuf,
    manual: PathBuf,
    hand: PathBuf,
    evil: PathBuf,
}

/// Main worktree plus: `wt-orca` (with the `orca/` mark), `repo.feat`
/// (worktrunk's sibling), `manual` (made by hand), `hand` (detached) and a
/// folder with an escape sequence in its name.
fn shop() -> Shop {
    let f = Fixture::with_commit(&git_from_path());
    let mut paths = Vec::new();
    for (path, branch) in [
        (f.root.join("wt-orca"), "orca-b"),
        (f.root.join("repo.feat"), "feat"),
        (f.root.join("manual"), "manual-b"),
        (
            f.root.join("wt-evil\u{1b}[2J\u{1b}]52;c;eA==\u{7}"),
            "evil-b",
        ),
    ] {
        f.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
        ]);
        paths.push(path.canonicalize().unwrap());
    }
    let hand = f.root.join("hand");
    f.git(&["worktree", "add", "-q", "--detach", hand.to_str().unwrap()]);
    std::fs::create_dir_all(f.repo.join(".git/worktrees/wt-orca/orca")).unwrap();
    let m = Machine::new(f);
    m.add(&m.f.repo);
    Shop {
        orca: paths[0].clone(),
        trunk: paths[1].clone(),
        manual: paths[2].clone(),
        evil: paths[3].clone(),
        hand: hand.canonicalize().unwrap(),
        m,
    }
}

fn status_text(s: &Shop, locale: &str) -> String {
    // The engine has read every worktree.
    for p in [&s.orca, &s.trunk, &s.manual, &s.hand] {
        read_worktree(&s.m, p);
    }
    text(&s.m.raptor(&["status"], &[("LANG", locale)]))
}

#[test]
fn status_text_shows_the_origin_in_english() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = shop();
    let shown = status_text(&s, "en_US.UTF-8");
    for expected in [
        "origin: Orca",
        "origin: probably worktrunk (by path)",
        "foreign (unknown origin)",
        "foreign, do not touch: detached HEAD",
    ] {
        assert!(
            shown.lines().any(|l| l.contains(expected)),
            "the English text must show `{expected}`:\n{shown}"
        );
    }
    s.m.stop();
}

#[test]
fn status_text_shows_the_origin_in_spanish() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = shop();
    let shown = status_text(&s, "es_ES.UTF-8");
    for expected in [
        "origen: Orca",
        "origen: probablemente worktrunk (por la ruta)",
        "ajeno (origen desconocido)",
        "ajeno, no tocar: HEAD separado",
    ] {
        assert!(
            shown.lines().any(|l| l.contains(expected)),
            "the Spanish text must show `{expected}`:\n{shown}"
        );
    }
    s.m.stop();
}

#[test]
fn status_json_carries_origin_only_when_it_exists() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = shop();
    let orca = read_worktree(&s.m, &s.orca);
    assert_eq!(
        orca["origin"],
        json!({"tool": "orca", "evidence": "admin-marker"}),
        "status --json must carry origin as the engine sends it: {orca:#}"
    );
    assert_eq!(
        read_worktree(&s.m, &s.trunk)["origin"],
        json!({"tool": "worktrunk", "evidence": "path-pattern"})
    );
    assert_eq!(
        read_worktree(&s.m, &s.manual)["origin"],
        json!({"tool": "unknown", "evidence": "none"})
    );
    assert_eq!(
        read_worktree(&s.m, &s.hand)["origin"],
        json!({"tool": "unknown", "evidence": "none", "hands_off": "detached"})
    );
    let main =
        worktree_in(&s.m.status(), &s.m.f.repo.canonicalize().unwrap()).expect("the main worktree");
    assert!(
        main.get("origin").is_none(),
        "the main worktree carries no origin: {main:#}"
    );
    s.m.stop();
}

#[test]
fn status_keeps_untrusted_text_sanitized() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = shop();
    let shown = status_text(&s, "en_US.UTF-8");
    read_worktree(&s.m, &s.evil);
    let shown = format!("{shown}{}", text(&s.m.raptor(&["status"], &[])));
    assert!(
        shown.contains("foreign (unknown origin)"),
        "the origin line must be there for the check to mean anything:\n{shown}"
    );
    assert!(
        !shown.contains('\u{1b}') && !shown.contains('\u{7}'),
        "the status must paint untrusted text inert (SEC-12): {shown:?}"
    );
    s.m.stop();
}
