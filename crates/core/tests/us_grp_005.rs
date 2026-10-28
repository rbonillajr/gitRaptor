//! US-GRP-005: the gap between two runs of the engine is recorded, what
//! changed in it is reconciled as unattributed, and `events.gaps` lists it.
//! Real daemons (in-process) over a temporary profile and testkit repos
//! (NFR-01). No fixed waits: thread joins, answers and store marks with a
//! 10 s ceiling.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{ClientKind, EventsHistoryResult, GitEventKind, GitEventView};
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_api::{Actor, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{Origin, ProfileDirs, WriteOp};
use gitraptor_testkit::{Exceptions, Fixture, check};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(10);
const GAPS: &str = "events.gaps";

fn config(dirs: ProfileDirs, heartbeat: Duration) -> DaemonConfig {
    let env = DaemonEnv::from_vars(std::env::vars_os());
    DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat,
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    }
}

struct Live {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Live {
    fn spawn(dirs: ProfileDirs, daemon: Daemon) -> Self {
        let handle = daemon.shutdown_handle();
        let join = Some(std::thread::spawn(move || daemon.run()));
        Self { dirs, handle, join }
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    fn stop(mut self) {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap();
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

fn rpc(err: ClientError) -> (i64, String) {
    match err {
        ClientError::Rpc(e) => (e.code, e.message),
        other => panic!("expected an RPC error, got {other}"),
    }
}

fn history(c: &mut Client, repo_id: &str) -> Vec<GitEventView> {
    let r: EventsHistoryResult = c
        .call(
            methods::EVENTS_HISTORY,
            json!({"repo_id": repo_id, "limit": 500}),
        )
        .unwrap();
    r.events
}

fn gaps(c: &mut Client, repo_id: &str) -> Vec<Value> {
    let r: Value = c
        .call(GAPS, json!({"repo_id": repo_id, "since_utc_ms": 0}))
        .unwrap();
    r["gaps"].as_array().unwrap().clone()
}

struct Scenario {
    fx: Fixture,
    tp: TempProfile,
    wt: PathBuf,
    repo_id: String,
}

/// "demo" with the worktree `feat-login`; a first engine run that is
/// dropped without `stop`, as a crash leaves it. With `session`, a
/// registered Claude Code session is in the store (as `daemon_lifecycle.rs`).
fn scenario(session: bool) -> Scenario {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.git(&["branch", "feat-login"]);
    let wt = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    if session {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let mut batch = sample_batch(&wt, "s1", 0);
        if let WriteOp::StartSession {
            origin,
            detection_key,
            ..
        } = &mut batch[1]
        {
            *origin = Origin::Registered;
            *detection_key = None;
        }
        store.write_batch(&batch).unwrap();
    }
    drop(profile);
    drop(Daemon::start(config(tp.dirs(), Duration::from_secs(3600))).unwrap());
    Scenario {
        fx,
        tp,
        wt,
        repo_id: entry.repo_id,
    }
}

impl Scenario {
    /// Two continuous commits in `feat-login` and a new branch `hotfix`;
    /// returns the two commit ids.
    fn change(&self) -> (String, String) {
        let mut ids = Vec::new();
        for n in 1..=2 {
            std::fs::write(self.wt.join(format!("f{n}.txt")), format!("{n}\n")).unwrap();
            self.fx.git_in(&self.wt, &["add", "."]);
            self.fx
                .git_in(&self.wt, &["commit", "-q", "-m", &format!("c{n}")]);
            ids.push(
                self.fx
                    .git_in(&self.wt, &["rev-parse", "HEAD"])
                    .trim()
                    .to_owned(),
            );
        }
        self.fx.git(&["branch", "hotfix"]);
        (ids.remove(0), ids.remove(0))
    }

    fn restart(&self) -> Live {
        let daemon = Daemon::start(config(self.tp.dirs(), Duration::from_secs(3600))).unwrap();
        Live::spawn(self.tp.dirs(), daemon)
    }

    fn stop_command_then_restart(&self) -> Live {
        let daemon = Daemon::start(config(self.tp.dirs(), Duration::from_secs(3600))).unwrap();
        daemon.stop(StopCause::StopCommand {
            requested_by: "cli".into(),
        });
        self.restart()
    }
}

fn reconcile_commits(s: &Scenario) -> (String, String, Live) {
    let (c1, c2) = s.change();
    (c1, c2, s.restart())
}

fn assert_commits_reconciled(s: &Scenario, c1: &str, c2: &str, live: &Live) {
    let mut c = live.client();
    let events = history(&mut c, &s.repo_id);
    let commits: Vec<&GitEventView> = events
        .iter()
        .filter(|e| e.kind == GitEventKind::Commit)
        .collect();
    assert_eq!(commits.len(), 2, "{events:?}");
    assert_eq!(commits[0].details.new_commit.as_deref(), Some(c1));
    assert_eq!(commits[1].details.new_commit.as_deref(), Some(c2));
    let gap_id = commits[0].gap_id.clone().expect("commit carries a gap_id");
    for e in &commits {
        assert_eq!(e.actor, Actor::Unattributed);
        assert_eq!(e.gap_id.as_deref(), Some(gap_id.as_str()));
    }
    let hotfix = events
        .iter()
        .find(|e| {
            e.kind == GitEventKind::BranchCreate
                && e.details.branch.as_ref().map(|b| b.raw()) == Some("hotfix")
        })
        .expect("branch-create of hotfix");
    assert_eq!(hotfix.gap_id.as_deref(), Some(gap_id.as_str()));
    assert_eq!(hotfix.actor, Actor::Unattributed);

    let listed = gaps(&mut c, &s.repo_id);
    let gap = listed
        .iter()
        .find(|g| g["gap_id"] == gap_id.as_str())
        .unwrap_or_else(|| panic!("gap {gap_id} not listed: {listed:?}"));
    assert_eq!(gap["cause"], "daemon-down-during-session");
    assert_eq!(gap["during_session"], true);

    let snap: ScopeSnapshot = c
        .call(
            methods::SCOPE_SNAPSHOT,
            json!({"scope": Scope::Repo { repo_id: s.repo_id.clone() }}),
        )
        .unwrap();
    let ScopeSnapshot::Repo(repo) = snap else {
        panic!("{snap:?}")
    };
    let w = repo
        .repo
        .worktrees
        .iter()
        .find(|w| Path::new(w.path.raw()) == s.wt)
        .expect("feat-login worktree");
    assert!(w.last_activity_in_gap, "{w:?}");
    assert_eq!(w.last_activity_utc_ms, Some(commits[1].observed_utc_ms));
}

#[test]
fn the_commits_of_a_gap_are_reconciled_unattributed() {
    let s = scenario(true);
    let (c1, c2, live) = reconcile_commits(&s);
    assert_commits_reconciled(&s, &c1, &c2, &live);
}

fn assert_stop_gap(s: &Scenario, live: &Live, during_session: bool) {
    let mut c = live.client();
    let listed = gaps(&mut c, &s.repo_id);
    let gap = listed
        .iter()
        .find(|g| g["cause"] == "daemon-stopped")
        .unwrap_or_else(|| panic!("no daemon-stopped gap: {listed:?}"));
    let started = gap["started_utc_ms"].as_i64().unwrap();
    let ended = gap["ended_utc_ms"].as_i64().unwrap();
    assert!(started <= ended, "{gap}");
    assert_eq!(
        gap["during_session"].as_bool().unwrap_or(false),
        during_session,
        "{gap}"
    );
    let id = gap["gap_id"].as_str().unwrap();
    let events = history(&mut c, &s.repo_id);
    assert!(
        events.iter().all(|e| e.gap_id.as_deref() != Some(id)),
        "nothing changed, so nothing is reconciled: {events:?}"
    );
}

#[test]
fn the_gap_is_listed_in_the_repo_history() {
    let s = scenario(false);
    let live = s.stop_command_then_restart();
    assert_stop_gap(&s, &live, false);
}

#[test]
fn a_stop_command_during_a_session_marks_the_gap() {
    let s = scenario(true);
    let live = s.stop_command_then_restart();
    assert_stop_gap(&s, &live, true);
}

#[test]
fn a_reflog_that_does_not_reach_the_old_tip_collapses_into_one_branch_update() {
    let s = scenario(false);
    let old = s.fx.git_in(&s.wt, &["rev-parse", "HEAD"]).trim().to_owned();
    let (_, c2) = s.change();
    s.fx.git(&["reflog", "expire", "--expire=now", "--all"]);
    let live = s.restart();
    let mut c = live.client();
    let events = history(&mut c, &s.repo_id);
    assert!(
        events.iter().all(|e| e.kind != GitEventKind::Commit),
        "never a partial list: {events:?}"
    );
    let updates: Vec<&GitEventView> = events
        .iter()
        .filter(|e| {
            e.kind == GitEventKind::BranchUpdate
                && e.details.branch.as_ref().map(|b| b.raw()) == Some("feat-login")
        })
        .collect();
    assert_eq!(updates.len(), 1, "{events:?}");
    assert_eq!(updates[0].details.old_commit.as_deref(), Some(old.as_str()));
    assert_eq!(updates[0].details.new_commit.as_deref(), Some(c2.as_str()));
}

#[test]
fn a_corrupt_store_records_a_store_corrupt_gap_from_the_add_date() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 5_000)
        .unwrap();
    drop(profile.open_store(&entry.repo_id).unwrap());
    let store_path = tp
        .dirs()
        .repos_dir()
        .join(format!("{}.sqlite", entry.repo_id));
    drop(profile);
    std::fs::write(&store_path, vec![0xA5u8; 8192]).unwrap();

    let daemon = Daemon::start(config(tp.dirs(), Duration::from_secs(3600))).unwrap();
    let live = Live::spawn(tp.dirs(), daemon);
    let mut c = live.client();
    let listed = gaps(&mut c, &entry.repo_id);
    let gap = listed
        .iter()
        .find(|g| g["cause"] == "store-corrupt")
        .unwrap_or_else(|| panic!("no store-corrupt gap: {listed:?}"));
    assert_eq!(gap["started_utc_ms"], entry.added_ms);
    let id = gap["gap_id"].as_str().unwrap();
    let events = history(&mut c, &entry.repo_id);
    assert!(events.iter().all(|e| e.gap_id.as_deref() != Some(id)));
    drop(c);
    live.stop();

    let (store, _) = tp.open().open_store(&entry.repo_id).unwrap();
    assert!(
        store
            .last_known_state(&canonical(&fx.repo))
            .unwrap()
            .is_some(),
        "the last known state of the main worktree is written"
    );
}

fn mark(dirs: &ProfileDirs, repo_id: &str) -> Option<i64> {
    let (profile, _) = gitraptor_core::profile::Profile::open(dirs.clone()).ok()?;
    let (store, _) = profile.open_store(repo_id).ok()?;
    store.observed_until().ok().flatten()
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < DEADLINE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_unreadable_repo_is_not_observed_and_keeps_its_mark() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let other_dir = tp.root.path().join("repos");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = init_repo(&other_dir, "healthy", true);
    let mut profile = tp.open();
    let (a, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    let (b, _) = profile.add_repo(&common_dir(&other), None, 1).unwrap();
    drop(profile);

    let first = Live::spawn(
        tp.dirs(),
        Daemon::start(config(tp.dirs(), Duration::from_millis(50))).unwrap(),
    );
    let before = wait_for("marks of both repos", || {
        mark(&tp.dirs(), &b.repo_id)?;
        mark(&tp.dirs(), &a.repo_id)
    });
    first.stop();
    let before = mark(&tp.dirs(), &a.repo_id).unwrap_or(before);
    let gaps_before = tp
        .open()
        .open_store(&a.repo_id)
        .unwrap()
        .0
        .gaps()
        .unwrap()
        .len();

    let moved = fx.root.join("repo-moved");
    std::fs::rename(&fx.repo, &moved).unwrap();

    let daemon = Daemon::start(config(tp.dirs(), Duration::from_millis(50))).unwrap();
    assert!(
        daemon.report().unavailable.contains(&a.repo_id),
        "unavailable: {:?}",
        daemon.report().unavailable
    );
    let live = Live::spawn(tp.dirs(), daemon);
    // Two heartbeats: the healthy repo's mark moves twice.
    let start = mark(&tp.dirs(), &b.repo_id);
    let mut seen = Vec::new();
    wait_for("two heartbeats", || {
        let m = mark(&tp.dirs(), &b.repo_id)?;
        if Some(m) != start && !seen.contains(&m) {
            seen.push(m);
        }
        (seen.len() >= 2).then_some(())
    });
    live.stop();

    assert_eq!(mark(&tp.dirs(), &a.repo_id), Some(before), "mark is kept");
    let gaps_after = tp
        .open()
        .open_store(&a.repo_id)
        .unwrap()
        .0
        .gaps()
        .unwrap()
        .len();
    assert_eq!(gaps_after, gaps_before, "no gap is written for it");
}

#[test]
fn the_gap_reconciliation_writes_nothing_in_the_repo() {
    let s = scenario(true);
    let (c1, c2) = s.change();
    let report = check("US-GRP-005 BR-CONS-001", &s.fx, &Exceptions::none(), || {
        let live = s.restart();
        assert_commits_reconciled(&s, &c1, &c2, &live);
        live.stop();
    });
    report.assert_intact();
}

#[test]
fn events_gaps_rejects_an_invalid_or_unknown_repo() {
    let tp = TempProfile::new();
    let live = Live::spawn(
        tp.dirs(),
        Daemon::start(config(tp.dirs(), Duration::from_secs(3600))).unwrap(),
    );
    let mut c = live.client();
    let invalid = c
        .call::<_, Value>(GAPS, json!({"repo_id": "not-hex!"}))
        .unwrap_err();
    assert_eq!(rpc(invalid).0, code::INVALID_PARAMS);

    let unknown_id = "0123456789abcdef0123456789abcdef";
    let expected = rpc(c
        .call::<_, Value>(methods::EVENTS_HISTORY, json!({"repo_id": unknown_id}))
        .unwrap_err());
    let got = rpc(c
        .call::<_, Value>(GAPS, json!({"repo_id": unknown_id}))
        .unwrap_err());
    assert_ne!(got.0, code::METHOD_NOT_FOUND, "{got:?}");
    assert_eq!(got, expected);
}
