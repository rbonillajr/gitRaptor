//! TS-GRP-006 end to end: observation tiers in a real daemon (in-process)
//! over a testkit fixture and a temporary profile; never this repo or the
//! real profile (NFR-01). The threshold is short so the repo sleeps during
//! the test; every step waits for a state (a log line, a snapshot), never a
//! fixed time.
//!
//! The NFR-01 condition of the PO (Q49): dormancy must not reduce the
//! protection. An edit in a dormant repo followed by a raw `reset --hard`
//! leaves the edit recoverable from the Time Machine.
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LOG_FILE, LogLimits, ShutdownHandle, StopCause, StopReport,
    TierConfig, TierTestOp, TmCapture,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use serde_json::json;

const DEADLINE: Duration = Duration::from_secs(20);

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

struct Running {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Observes `fx`'s repo in a fresh profile, with tiers on and a short
/// threshold, and the continuous capture with a short quiet time.
fn start(fx: Fixture) -> Running {
    start_with(fx, Duration::from_millis(800), |_, _| {})
}

/// The same with a given threshold; `seed` writes to the repo's store
/// before the daemon starts.
fn start_with(
    fx: Fixture,
    dormant_after: Duration,
    seed: impl FnOnce(&mut gitraptor_core::profile::RepoStore, &Path),
) -> Running {
    start_tiers(
        fx,
        TierConfig {
            dormant_after: Some(dormant_after),
            check_every: Duration::from_millis(50),
            ..TierConfig::default()
        },
        seed,
    )
}

/// The same with the tiers as given.
fn start_tiers(
    fx: Fixture,
    tiers: TierConfig,
    seed: impl FnOnce(&mut gitraptor_core::profile::RepoStore, &Path),
) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    seed(&mut store, &canonical(&fx.repo));
    drop(store);
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers,
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: true,
                quiet: Duration::from_millis(150),
                max_interval: Duration::from_millis(600),
            },
            layer: None,
            no_free_space_floor: true,
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let r = Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        handle,
        join: Some(join),
    };
    drop(connect(&r.tp));
    r
}

impl Running {
    fn log(&self) -> String {
        std::fs::read_to_string(self.tp.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// Waits until the daemon log has `event` at least `n` times.
    fn logged(&self, event: &str, n: usize) {
        let start = Instant::now();
        while self.log().matches(event).count() < n {
            assert!(
                start.elapsed() < DEADLINE,
                "{event} not logged {n} times:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn store(&self) -> Option<SnapshotStore> {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .ok()
            .flatten()
    }

    fn files(&self, store: &SnapshotStore, id: &str, key: &str) -> Vec<(String, Vec<u8>)> {
        store
            .files(id, key)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
            .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
            .collect()
    }

    /// Waits for a snapshot of worktree `key` holding `path` with `content`.
    fn snapshot_with(&self, key: &str, path: &str, content: &[u8]) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = self.store() {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    if self
                        .files(&store, id, key)
                        .iter()
                        .any(|(p, b)| p == path && b == content)
                    {
                        return id.clone();
                    }
                }
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no snapshot of {key} with {path}:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        let value = connect(&self.tp)
            .call(
                methods::TM_UNDO,
                json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
            )
            .unwrap();
        serde_json::from_value(value).unwrap()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

/// `api.rs` committed, and a linked worktree "feat-login" on its own
/// branch (key `wt-wt-feat-login` in a snapshot).
fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.git(&["add", "api.rs"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

const KEY: &str = "wt-wt-feat-login";
const EDITED: &[u8] = b"fn api() { login(); }\n";

/// NFR-01 (Q49): an edit in a dormant repo wakes it through its sentinel,
/// the Time Machine captures it, and after a raw `reset --hard` the undo
/// brings the edit back.
#[test]
fn edit_then_reset_hard_in_a_dormant_repo_is_recoverable() {
    let (fx, wt) = repo_with_login();
    let r = start(fx);
    // Dado un repo dormido.
    r.logged("repo_dormant", 1);
    // Cuando se edita un archivo.
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    // El centinela lo despierta y la Time Machine lo captura.
    r.logged("repo_woken", 1);
    let captured = r.snapshot_with(KEY, "api.rs", EDITED);
    // Y un agente hace un reset destructivo con Git crudo.
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), b"fn api() {}\n");
    // Entonces la edición se recupera.
    let undo = r.undo_ok(&wt);
    assert_eq!(undo.target_snapshot_id, captured, "{}", r.log());
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), EDITED);
}

/// A repo that sleeps and wakes again keeps its events: three commits
/// made while dormant are three events in the history, in order.
#[test]
fn commits_while_dormant_reach_the_history_in_order() {
    let (fx, _wt) = repo_with_login();
    let r = start(fx);
    r.logged("repo_dormant", 1);
    let mut commits = Vec::new();
    for i in 0..3 {
        r.fx.write(&format!("c{i}.txt"), "c\n");
        r.fx.git(&["add", "."]);
        r.fx.git(&["commit", "-q", "-m", &format!("c{i}")]);
        commits.push(r.fx.git(&["rev-parse", "HEAD"]).trim().to_owned());
    }
    r.logged("repo_woken", 1);
    // The history is a request about the repo: it wakes it if it slept
    // again, and its page has the three commits in order.
    let start = Instant::now();
    loop {
        let page: serde_json::Value = connect(&r.tp)
            .call(
                methods::EVENTS_HISTORY,
                json!({ "repo_id": r.repo_id, "limit": 100 }),
            )
            .unwrap();
        let seen: Vec<String> = page
            .as_array()
            .or_else(|| page.get("events").and_then(|e| e.as_array()))
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|e| e["kind"] == "commit")
            .filter_map(|e| e["details"]["new_commit"].as_str().map(str::to_owned))
            .collect();
        if seen.ends_with(&commits) {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{seen:?} vs {commits:?}\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Files of this process whose path has `needle` (`lsof`, macOS).
fn open_files_with(needle: &str) -> usize {
    let out = std::process::Command::new("lsof")
        .args(["-n", "-P", "-Fn", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with('n') && l.contains(needle))
        .count()
}

/// N1 step 4: a dormant repo has no file of its store open, and a wake
/// opens it again.
#[test]
fn a_dormant_repo_has_its_store_closed() {
    let (fx, wt) = repo_with_login();
    let r = start(fx);
    let store = format!("{}.sqlite", r.repo_id);
    // Active: its store is open (the threshold has not passed yet).
    let open = open_files_with(&store);
    if !r.log().contains("repo_dormant") {
        assert!(open > 0, "the active store is not open: lsof sees nothing");
    }
    r.logged("repo_dormant", 1);
    // The log line is written once the store is closed.
    assert_eq!(open_files_with(&store), 0, "{}", r.log());
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    r.logged("repo_woken", 1);
    let start = Instant::now();
    while open_files_with(&store) == 0 {
        // It may already sleep again: then it was opened and closed.
        if r.log().matches("repo_dormant").count() > 1 {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "{}", r.log());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The tier of each repo in the snapshot (`observation.tiers`, N8): dormant
/// with the time it was checked, then active once a request wakes it.
#[test]
fn the_snapshot_shows_the_tier() {
    let (fx, _wt) = repo_with_login();
    let r = start(fx);
    let tier = |r: &Running| -> (String, Option<i64>) {
        let snap: serde_json::Value = connect(&r.tp)
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        let repo = &snap["repos"][0];
        (
            repo["tier"].as_str().unwrap_or("").to_owned(),
            repo["checked_utc_ms"].as_i64(),
        )
    };
    r.logged("repo_dormant", 1);
    let (t, checked) = tier(&r);
    // It may already be awake again only if something woke it.
    if !r.log().contains("repo_woken") {
        assert_eq!(t, "dormant", "{}", r.log());
        assert!(checked.is_some());
    }
    // A request about the repo wakes it.
    let _: serde_json::Value = connect(&r.tp)
        .call(
            methods::EVENTS_HISTORY,
            json!({ "repo_id": r.repo_id, "limit": 1 }),
        )
        .unwrap();
    r.logged("repo_woken", 1);
    let start = Instant::now();
    loop {
        let (t, checked) = tier(&r);
        if t == "active" {
            assert_eq!(checked, None);
            break;
        }
        // Or it went back to sleep already: then it was active in between.
        if r.log().matches("repo_dormant").count() > 1 {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "{t}\n{}", r.log());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `engine.resources` counts the repos, worktrees and watches per tier
/// (`observation.tiers`, N8).
#[test]
fn resources_count_the_tiers() {
    let (fx, _wt) = repo_with_login();
    let r = start(fx);
    r.logged("repo_dormant", 1);
    let res: serde_json::Value = connect(&r.tp)
        .call(methods::ENGINE_RESOURCES, json!({}))
        .unwrap();
    let o = &res["observation"];
    if r.log().contains("repo_woken") {
        // Something woke it in between: then it is active or waking.
        assert_eq!(o["dormant"]["repos"], 0, "{res:#}");
        return;
    }
    assert_eq!(o["dormant"]["repos"], 1, "{res:#}");
    assert_eq!(o["dormant"]["worktrees"], 2, "{res:#}");
    assert!(o["dormant"]["watches"].as_u64().unwrap() >= 2, "{res:#}");
    assert_eq!(o["active"]["repos"], 0, "{res:#}");
    assert_eq!(o["dormant"]["sweep_interval_s"], 120, "{res:#}");
    assert_eq!(o["degraded"]["worktrees"], 0, "{res:#}");
}

/// N1, N4: a client subscribed to one repo wakes it and keeps it active; a
/// subscription to the fleet (the global scope) neither wakes it nor keeps
/// it awake. The negative part waits a bounded time: three thresholds.
#[test]
fn a_subscription_to_the_repo_wakes_it_and_the_fleet_does_not() {
    let (fx, _wt) = repo_with_login();
    let r = start(fx);
    r.logged("repo_dormant", 1);
    let mut fleet = connect(&r.tp);
    let _: serde_json::Value = fleet
        .call(
            methods::SCOPE_SUBSCRIBE,
            json!({ "scope": { "scope": "global" } }),
        )
        .unwrap();
    let mut tui = connect(&r.tp);
    let _: serde_json::Value = tui
        .call(
            methods::SCOPE_SUBSCRIBE,
            json!({ "scope": { "scope": "repo", "repo_id": r.repo_id } }),
        )
        .unwrap();
    r.logged("repo_woken", 1);
    let woken = r.log().matches("repo_woken").count();
    let dormant = r.log().matches("repo_dormant").count();
    let until = Instant::now() + Duration::from_millis(3 * 800);
    while Instant::now() < until {
        assert_eq!(
            r.log().matches("repo_dormant").count(),
            dormant,
            "slept while subscribed:\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(r.log().matches("repo_woken").count(), woken);
    // Without the repo's subscriber it sleeps again, the fleet's still there.
    drop(tui);
    r.logged("repo_dormant", dormant + 1);
    drop(fleet);
}

/// N1 at startup: a repo whose last Git event is older than the threshold
/// sleeps at the first check, not a whole threshold after the start. With
/// a threshold of one hour, only the persisted activity can explain it.
#[test]
fn an_idle_repo_sleeps_at_the_first_check_after_a_start() {
    use gitraptor_core::profile::{NewEvent, Timestamp, WriteOp};
    let (fx, _wt) = repo_with_login();
    let two_hours_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 2 * 3600 * 1000;
    let r = start_with(fx, Duration::from_secs(3600), |store, root| {
        store
            .write_batch(&[
                WriteOp::UpsertWorktree {
                    path: root.to_path_buf(),
                    admin_name: None,
                    seen_ms: two_hours_ago,
                },
                WriteOp::AppendEvent(NewEvent {
                    worktree: root.to_path_buf(),
                    kind: "commit".into(),
                    metadata: "{}".into(),
                    observed: Timestamp {
                        utc_ms: two_hours_ago,
                        offset_s: 0,
                    },
                    session_id: None,
                    evidence: None,
                    gap_id: None,
                    authorship: None,
                }),
            ])
            .unwrap();
    });
    r.logged("repo_dormant", 1);
}

/// NFR-01, the race the review found: changes in flight while the repo goes
/// dormant. The tasks hand their open windows over as they stop, and those
/// batches are still queued in the loop when the sleep finishes; the store
/// must close after they are persisted, never before, or the commits are
/// lost for good (the wake classifies from the view the flush ended with).
///
/// Deterministic, with no timing: the observer loses the file events, so
/// the commits only reach the tasks through an overflow, which every task
/// queues before the `Sleep` of the threshold check that follows (one FIFO
/// per task, both sent by the loop in that order). The threshold is zero
/// and only the test runs the check. Fails with the fix reverted (closing
/// the store right after `sleep_repo`).
#[test]
fn changes_in_flight_when_the_repo_sleeps_are_persisted() {
    let (fx, wt) = repo_with_login();
    let r = start_tiers(
        fx,
        TierConfig {
            dormant_after: Some(Duration::ZERO),
            check_every: Duration::from_secs(3600),
            ..TierConfig::default()
        },
        |_, _| {},
    );
    assert!(r.handle.tier_test(TierTestOp::LoseEvents(true)));
    let mut commits = Vec::new();
    for i in 0..3 {
        r.fx.write(&format!("c{i}.txt"), "c\n");
        r.fx.git(&["add", "."]);
        r.fx.git(&["commit", "-q", "-m", &format!("c{i}")]);
        commits.push(r.fx.git(&["rev-parse", "HEAD"]).trim().to_owned());
    }
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    // Every task opens a window; the sleep that follows flushes them.
    assert!(r.handle.tier_test(TierTestOp::Overflow));
    assert!(r.handle.tier_test(TierTestOp::CheckTiers));
    assert!(r.handle.tier_test(TierTestOp::LoseEvents(false)));
    r.logged("repo_dormant", 1);
    // Reading the history wakes the repo; the commits must be there, once
    // each and in order, from the batches flushed on the way to sleep.
    let page: serde_json::Value = connect(&r.tp)
        .call(
            methods::EVENTS_HISTORY,
            json!({ "repo_id": r.repo_id, "limit": 100 }),
        )
        .unwrap();
    let events = page
        .as_array()
        .or_else(|| page.get("events").and_then(|e| e.as_array()))
        .cloned()
        .unwrap_or_default();
    let seen: Vec<String> = events
        .iter()
        .filter(|e| e["kind"] == "commit")
        .filter_map(|e| e["details"]["new_commit"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(seen, commits, "{events:#?}\n{}", r.log());
    // The edit of the linked worktree was persisted by the same flush: its
    // state shows it once the repo is awake (recoverable by the Time
    // Machine's capture, which follows the published state).
    r.logged("repo_woken", 1);
    let snap: serde_json::Value = connect(&r.tp)
        .call(methods::ENGINE_SNAPSHOT, json!({}))
        .unwrap();
    let linked = snap["repos"][0]["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["main"] == false)
        .cloned()
        .unwrap();
    assert_eq!(linked["status"]["counts"]["unstaged"], 1, "{linked:#}");
}
