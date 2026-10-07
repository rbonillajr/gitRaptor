//! US-GRP-002 end to end: each Gherkin scenario, and each example of the
//! outline, with the real `raptor` binary as daemon and as client, over a
//! temporary machine built by the "intact repo" harness (INF-GRP-001):
//! temporary repo and profile, never this repo nor the real profile
//! (NFR-01). The developer runs reserved commands under a pty (`script`).
//!
//! Freshness is checked as ADR-GRP-011 § 2 measures it: `t0` when the write
//! or the Git command ends, `t_client_recv` when the first event that shows
//! it arrives, on the common monotonic clock; the engine's part is 300 ms.
//! One sample per scenario: the p95 gate of 200 samples is INF-GRP-002's.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::event::{GIT_EVENT, WORKTREE_STATE};
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, HeadView, Snapshot,
    SubscribeResult, WorktreeStateData, WorktreeStatus,
};
use gitraptor_api::{Actor, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// The engine's part of the NFR-04 budget (ADR-GRP-011 § 2).
const ENGINE_BUDGET_MS: u64 = 300;

/// One scenario at a time: each runs its own engine, and timing scenarios
/// must not share the machine with nine others.
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
            // under a real agent (as in repo_state.rs, debug builds only).
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
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

    fn add(&self) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
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

    /// The repo's history, oldest first.
    fn history(&self) -> Vec<GitEventView> {
        let page: EventsHistoryResult = self
            .client()
            .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
            .unwrap();
        page.events
    }

    /// Waits until the history has an event of `kind` in `worktree`.
    fn event(&self, kind: GitEventKind, worktree: &Path) -> GitEventView {
        let start = Instant::now();
        loop {
            let history = self.history();
            if let Some(e) = history
                .iter()
                .find(|e| e.kind == kind && Path::new(e.worktree.raw()) == worktree)
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no {kind:?} in {}: {history:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The worktree at `path` in `status --json`, once `ok` holds on it.
    fn worktree_when(&self, path: &Path, ok: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let status = self.status();
            let wt = status["repos"][0]["worktrees"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["path"] == path.to_str().unwrap())
                .cloned()
                .unwrap_or(Value::Null);
            if ok(&wt) {
                return wt;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{}: {status}",
                path.display()
            );
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

fn canonical(path: &Path) -> PathBuf {
    // The engine's own canonical form (the drive form on Windows).
    gitraptor_core::observe::canonical(path)
}

/// "demo": `main` with `login.txt`, branch `feat-login` and, if `linked`,
/// its worktree "feat-login" without changes.
fn demo(linked: bool) -> (Machine, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let wt = f.root.join("wt-feat-login");
    if linked {
        f.add_worktree("feat-login", "feat-login");
    }
    let m = Machine::new(f);
    let wt = if linked { canonical(&wt) } else { wt };
    (m, wt)
}

/// `demo` observed, with the observer settled.
fn observed(linked: bool) -> (Machine, PathBuf) {
    let (m, wt) = demo(linked);
    m.add();
    std::thread::sleep(Duration::from_millis(300));
    (m, wt)
}

/// The common assertions of the outline: the event is in the history in
/// "feat-login" (not inferred), with its time, and unattributed.
fn assert_recorded(e: &GitEventView, since_ms: i64) {
    assert!(!e.details.worktree_inferred, "{e:#?}");
    assert_eq!(e.actor, Actor::Unattributed);
    let now = wall_ms();
    assert!(
        e.observed_utc_ms >= since_ms && e.observed_utc_ms <= now,
        "{e:#?}"
    );
    assert!(e.utc_offset_s.abs() <= 14 * 3600);
}

fn wall_ms() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since.as_millis()).unwrap()
}

fn is_clean_on(branch: &'static str) -> impl Fn(&Value) -> bool {
    move |w| w["branch"] == branch && w["clean"] == true
}

// ------------------------------------------------------------ Escenario 1

/// Un cambio de archivo se refleja casi al instante.
#[test]
fn a_file_change_is_reflected_almost_instantly() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    let mut subscriber = m.client();
    let _: SubscribeResult = subscriber
        .call(methods::EVENTS_SUBSCRIBE, json!({}))
        .unwrap();

    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
    let t0 = monotonic_ns();
    let deadline = Instant::now() + Duration::from_secs(5);
    let t_client_recv = loop {
        assert!(
            Instant::now() < deadline,
            "no worktree.state with the change"
        );
        let Some(n) = subscriber
            .next_notification(Duration::from_millis(500))
            .unwrap()
        else {
            continue;
        };
        let t = monotonic_ns();
        let event = &n.params["event"];
        if event["kind"] != WORKTREE_STATE {
            continue;
        }
        assert!(event["timings"].is_object(), "{event}");
        let data: WorktreeStateData = serde_json::from_value(event["data"].clone()).unwrap();
        let shows = data.worktrees.iter().any(|w| {
            Path::new(w.path.raw()) == wt
                && matches!(&w.status, WorktreeStatus::Ready { changes, .. }
                    if changes.iter().any(|c| c.path.raw() == "login.txt"))
        });
        if shows {
            break t;
        }
    };
    let engine_ms = (t_client_recv - t0) / 1_000_000;
    assert!(
        engine_ms <= ENGINE_BUDGET_MS,
        "engine part {engine_ms} ms > {ENGINE_BUDGET_MS} ms"
    );

    let w = m.worktree_when(&wt, |w| w["clean"] == false);
    assert_eq!(
        w["changes"],
        json!([{"path": "login.txt", "area": "unstaged", "kind": "modified"}])
    );
    drop(subscriber);
    m.stop();
}

// ------------------------------------------------------------ Escenario 2
// Cada evento de Git queda registrado con su momento y su actor: one test
// per example.

/// Ejemplo: commit. Also what `raptor events` shows, in English and Spanish.
#[test]
fn git_event_commit() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    let since = wall_ms();
    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
    m.f.git_in(&wt, &["commit", "-qam", "password"]);
    let e = m.event(GitEventKind::Commit, &wt);
    assert_recorded(&e, since);
    assert_eq!(e.details.branch.as_ref().unwrap().raw(), "feat-login");
    m.worktree_when(&wt, is_clean_on("feat-login"));

    let out = m.raptor(&["events"], &[]);
    let shown = String::from_utf8_lossy(&out.stdout);
    // With its declared authorship (US-GRD-019): "commit by … · wt-feat-login (feat-login)".
    assert!(shown.contains("commit by "), "{shown}");
    assert!(shown.contains("(feat-login)"), "{shown}");
    assert!(shown.contains("(unattributed)"), "{shown}");
    let out = m.raptor(&["events"], &[("LANG", "es_ES.UTF-8")]);
    let shown = String::from_utf8_lossy(&out.stdout);
    assert!(shown.contains("commit de "), "{shown}");
    assert!(shown.contains("(sin atribuir)"), "{shown}");
    m.stop();
}

/// Ejemplo: cambio de rama.
#[test]
fn git_event_branch_switch() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo(true);
    m.f.git(&["branch", "login-v2"]);
    m.add();
    std::thread::sleep(Duration::from_millis(300));
    let since = wall_ms();
    m.f.git_in(&wt, &["switch", "-q", "login-v2"]);
    let e = m.event(GitEventKind::BranchSwitch, &wt);
    assert_recorded(&e, since);
    assert_eq!(e.details.from.as_ref().unwrap().raw(), "feat-login");
    assert_eq!(e.details.branch.as_ref().unwrap().raw(), "login-v2");
    m.worktree_when(&wt, is_clean_on("login-v2"));
    m.stop();
}

/// Ejemplo: creación de rama.
#[test]
fn git_event_branch_create() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    let since = wall_ms();
    m.f.git_in(&wt, &["switch", "-q", "-c", "login-v2"]);
    let e = m.event(GitEventKind::BranchCreate, &wt);
    assert_recorded(&e, since);
    assert_eq!(e.details.branch.as_ref().unwrap().raw(), "login-v2");
    m.worktree_when(&wt, is_clean_on("login-v2"));
    m.stop();
}

/// Ejemplo: borrado de rama.
#[test]
fn git_event_branch_delete() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    m.f.git_in(&wt, &["switch", "-q", "-c", "old-feature"]);
    m.f.git_in(&wt, &["switch", "-q", "feat-login"]);
    m.event(GitEventKind::BranchCreate, &wt);
    let since = wall_ms();
    m.f.git_in(&wt, &["branch", "-q", "-D", "old-feature"]);
    let e = m.event(GitEventKind::BranchDelete, &wt);
    assert_recorded(&e, since);
    assert_eq!(e.details.branch.as_ref().unwrap().raw(), "old-feature");
    m.worktree_when(&wt, is_clean_on("feat-login"));
    m.stop();
}

/// Ejemplo: creación de worktree. The worktree created is "feat-login".
#[test]
fn git_event_worktree_create() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(false);
    let since = wall_ms();
    m.f.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "feat-login"]);
    let wt = canonical(&wt);
    let e = m.event(GitEventKind::WorktreeCreate, &wt);
    assert_recorded(&e, since);
    m.worktree_when(&wt, is_clean_on("feat-login"));
    // Its changes are followed from then on.
    std::fs::write(wt.join("login.txt"), "user\nnew\n").unwrap();
    m.worktree_when(&wt, |w| w["clean"] == false);
    m.stop();
}

/// Ejemplo: borrado de worktree. The worktree deleted is "feat-login".
#[test]
fn git_event_worktree_delete() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    let since = wall_ms();
    m.f.git(&["worktree", "remove", wt.to_str().unwrap()]);
    let e = m.event(GitEventKind::WorktreeDelete, &wt);
    assert_recorded(&e, since);
    m.worktree_when(&wt, Value::is_null);
    m.stop();
}

/// Ejemplo: rebase.
#[test]
fn git_event_rebase() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    m.f.git_in(&wt, &["add", "a.txt"]);
    m.f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    m.f.write("b.txt", "b\n");
    m.f.git(&["add", "b.txt"]);
    m.f.git(&["commit", "-q", "-m", "b"]);
    m.event(GitEventKind::Commit, &wt);
    let since = wall_ms();
    m.f.git_in(&wt, &["rebase", "-q", "main"]);
    let e = m.event(GitEventKind::Rebase, &wt);
    assert_recorded(&e, since);
    assert!(
        m.history()
            .iter()
            .all(|e| e.kind != GitEventKind::BranchSwitch),
        "a rebase is not a branch switch"
    );
    m.worktree_when(&wt, is_clean_on("feat-login"));
    m.stop();
}

/// Ejemplo: merge.
#[test]
fn git_event_merge() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    m.f.git_in(&wt, &["add", "a.txt"]);
    m.f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    m.f.write("b.txt", "b\n");
    m.f.git(&["add", "b.txt"]);
    m.f.git(&["commit", "-q", "-m", "b"]);
    m.event(GitEventKind::Commit, &wt);
    let since = wall_ms();
    m.f.git_in(&wt, &["merge", "-q", "--no-edit", "main"]);
    let e = m.event(GitEventKind::Merge, &wt);
    assert_recorded(&e, since);
    m.worktree_when(&wt, is_clean_on("feat-login"));
    assert!(wt.join("b.txt").exists());
    m.stop();
}

/// Ejemplo: push, to a temporary bare remote.
#[test]
fn git_event_push() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo(true);
    let remote = m.f.root.join("remote.git");
    m.f.git(&["init", "-q", "--bare", remote.to_str().unwrap()]);
    m.f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    m.add();
    std::thread::sleep(Duration::from_millis(300));
    let since = wall_ms();
    m.f.git_in(&wt, &["push", "-q", "-u", "origin", "feat-login"]);
    let e = m.event(GitEventKind::Push, &wt);
    assert_recorded(&e, since);
    assert_eq!(
        e.details.branch.as_ref().unwrap().raw(),
        "origin/feat-login"
    );
    assert!(
        m.history()
            .iter()
            .all(|e| e.kind != GitEventKind::BranchCreate),
        "a push is not a new branch"
    );
    m.worktree_when(&wt, is_clean_on("feat-login"));
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// Ningún evento se presenta como hecho por un agente sin atribución.
#[test]
fn no_event_is_shown_as_done_by_an_agent_without_attribution() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = observed(true);
    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
    m.f.git_in(&wt, &["commit", "-qam", "password"]);
    let e = m.event(GitEventKind::Commit, &wt);
    assert_eq!(e.actor, Actor::Unattributed);
    let out = m.raptor(&["events", "--json"], &[]);
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        shown
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["actor"] == "unattributed"),
        "{shown}"
    );
    let repo_id = m.repo_id();
    m.stop();

    // In the history itself: no session, no evidence.
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let (store, _) = profile.open_store(&repo_id).unwrap();
    let events = store.events_for_worktree(&wt).unwrap();
    assert!(events.iter().any(|e| e.kind == "commit"), "{events:#?}");
    assert!(
        events
            .iter()
            .all(|e| e.session_id.is_none() && e.evidence.is_none())
    );
}

// ------------------------------------------------------------ Escenario 4

/// Diez worktrees activos a la vez se siguen sin perder eventos.
#[test]
fn ten_active_worktrees_are_followed_without_losing_events() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, _) = demo(false);
    let mut wts = Vec::new();
    for i in 0..10 {
        let name = format!("agent-{i}");
        m.f.git(&["branch", &name]);
        wts.push(canonical(&m.f.add_worktree(&name, &name)));
    }
    m.add();
    std::thread::sleep(Duration::from_millis(500));
    let mut subscriber = m.client();
    let _: SubscribeResult = subscriber
        .call(methods::EVENTS_SUBSCRIBE, json!({}))
        .unwrap();

    let t0s: Vec<u64> = std::thread::scope(|s| {
        let handles: Vec<_> = wts
            .iter()
            .map(|wt| {
                let f = &m.f;
                s.spawn(move || {
                    std::fs::write(wt.join("work.txt"), "done\n").unwrap();
                    f.git_in(wt, &["add", "work.txt"]);
                    f.git_in(wt, &["commit", "-q", "-m", "work"]);
                    monotonic_ns()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // The first state of each worktree that shows its commit (clean again
    // and no longer at the shared start commit).
    let mut reflected: Vec<Option<u64>> = vec![None; wts.len()];
    let mut commits_on_stream = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while reflected.iter().any(Option::is_none) || commits_on_stream < wts.len() {
        assert!(
            Instant::now() < deadline,
            "{reflected:?} {commits_on_stream}"
        );
        let Some(n) = subscriber
            .next_notification(Duration::from_millis(500))
            .unwrap()
        else {
            continue;
        };
        let t = monotonic_ns();
        let event = &n.params["event"];
        if event["kind"] == GIT_EVENT && event["data"]["kind"] == "commit" {
            commits_on_stream += 1;
        }
        if event["kind"] != WORKTREE_STATE {
            continue;
        }
        let data: WorktreeStateData = serde_json::from_value(event["data"].clone()).unwrap();
        for w in &data.worktrees {
            let Some(i) = wts.iter().position(|p| Path::new(w.path.raw()) == p) else {
                continue;
            };
            if let WorktreeStatus::Ready { head, counts, .. } = &w.status
                && matches!(head, HeadView::Branch { .. })
                && counts.is_clean()
                && reflected[i].is_none()
                && t >= t0s[i]
            {
                reflected[i] = Some(t);
            }
        }
    }
    for (i, t) in reflected.iter().enumerate() {
        let ms = t.unwrap().saturating_sub(t0s[i]) / 1_000_000;
        assert!(
            ms <= ENGINE_BUDGET_MS,
            "worktree {i}: engine part {ms} ms > {ENGINE_BUDGET_MS} ms"
        );
    }

    let history = m.history();
    let mut places: Vec<PathBuf> = history
        .iter()
        .filter(|e| e.kind == GitEventKind::Commit)
        .map(|e| PathBuf::from(e.worktree.raw()))
        .collect();
    places.sort();
    assert_eq!(places, wts, "{history:#?}");
    for wt in &wts {
        m.worktree_when(wt, |w| w["clean"] == true);
    }
    drop(subscriber);
    m.stop();
}

// ------------------------------------------------------------ Transversal

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

/// Observar los cambios no modifica el repo (BR-CONS-001): with the
/// observer watching every worktree, the repo stays byte for byte.
#[test]
fn repo_intact_watching_the_repo_does_not_modify_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (m, wt) = demo(true);
    std::fs::write(wt.join("login.txt"), "user\nchanged\n").unwrap();
    m.f.write("untracked.txt", "u\n");
    let report = check("US-GRP-002 watch", &m.f, &engine_profile(), || {
        m.add();
        m.worktree_when(&wt, |w| w["clean"] == false);
        std::thread::sleep(Duration::from_millis(500));
        let out = m.raptor(&["events"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["repo", "retire", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
    });
    report.assert_intact();
}
