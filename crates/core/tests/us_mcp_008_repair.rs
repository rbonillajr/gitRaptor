//! The manual snapshot's admission and identity (DS-US-MCP-008 repair): a request refused for
//! its session or its quota never costs the engine's wait or the walk of the worktree, and a
//! capture holds the worktree root the caller verified.
//!
//! Every test runs on a testkit fixture (temporary repo, home and profile; NFR-01). The wait is
//! a signal with a deadline, never a sleep.

mod tm_common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::{LogLimits, Logger};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::continuous::CaptureDeps;
use gitraptor_core::timemachine::engine::{EngineLink, RawGitEvent};
use gitraptor_core::timemachine::manual::{
    self, ManualAsk, ManualCaptured, ManualError, PER_MINUTE,
};
use gitraptor_core::timemachine::oplog::{Channel, Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::TmRepos;
use gitraptor_testkit::Fixture;
use rusqlite::Connection;
use tm_common::{Env, REPO_ID, git};

const T0: i64 = 1_728_000_000_000;
const WAIT: Duration = Duration::from_secs(20);

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn ask(worktree: &Path, session: &str) -> ManualAsk {
    ManualAsk {
        repo_id: REPO_ID.into(),
        worktree: worktree.to_path_buf(),
        label: "before the migration".into(),
        requester: agent(session),
        channel: Channel::Mcp,
    }
}

fn manual_rows(profile: &ProfileDirs) -> i64 {
    Connection::open(profile.data.join("tm").join(REPO_ID).join("oplog.db"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM snapshots WHERE level = 'manual'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

/// An engine whose wait for calm can be held: it says when it was entered, counts its calls and
/// waits for the test to let it go.
struct Gated {
    settles: AtomicUsize,
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
    gate: bool,
}

impl EngineLink for Gated {
    fn mark(&self, _repo_id: &str) -> i64 {
        1
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        self.settles.fetch_add(1, Ordering::SeqCst);
        if self.gate {
            let _ = self.entered.lock().unwrap().send(());
            self.release.lock().unwrap().recv_timeout(WAIT).ok()?;
        }
        Some(1)
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(Vec::new())
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct Daemon {
    f: Fixture,
    dirs: ProfileDirs,
    deps: CaptureDeps,
    engine: Arc<Gated>,
    release: Sender<()>,
    entered: Receiver<()>,
}

fn daemon(gate: bool) -> Daemon {
    let f = Fixture::with_commit(&git());
    let dirs = ProfileDirs::under_root(&f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
    let common = f.repo.join(".git").canonicalize().unwrap();
    repos.insert(REPO_ID, &common, oplog);
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    let engine = Arc::new(Gated {
        settles: AtomicUsize::new(0),
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
        gate,
    });
    let deps = CaptureDeps {
        repos,
        engine: Arc::clone(&engine) as Arc<dyn EngineLink>,
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
    };
    Daemon {
        f,
        dirs,
        deps,
        engine,
        release,
        entered,
    }
}

/// Item 1: a second request of a session that has a capture in flight is refused before the
/// engine is waited for, so concurrent requests do not multiply the wait or the walk.
#[test]
fn concurrent_runs_of_one_session_walk_the_worktree_once() {
    let d = daemon(true);
    let first_ask = ask(&d.f.repo, "s1");
    std::thread::scope(|s| {
        let first = s.spawn(|| manual::capture(&d.deps, &first_ask, T0));
        d.entered
            .recv_timeout(WAIT)
            .expect("the first capture reached the engine's wait");

        let second = manual::capture(&d.deps, &ask(&d.f.repo, "s1"), T0);
        assert!(
            matches!(second, Err(ManualError::InFlight)),
            "a second capture of the session is refused at once, got {second:?}"
        );
        assert_eq!(
            d.engine.settles.load(Ordering::SeqCst),
            1,
            "the refused request never reached the engine's wait"
        );

        d.release.send(()).unwrap();
        let first: Result<ManualCaptured, ManualError> = first.join().unwrap();
        assert!(first.is_ok(), "{first:?}");
    });
    assert_eq!(manual_rows(&d.dirs), 1);
}

/// Item 1: a session over its quota is refused before the engine's wait too.
#[test]
fn a_session_over_its_quota_is_refused_before_the_engine_is_waited_for() {
    let d = daemon(false);
    for _ in 0..PER_MINUTE {
        let now = manual::wall_now_ms();
        manual::capture(&d.deps, &ask(&d.f.repo, "s1"), now).expect("within the quota");
    }
    let before = d.engine.settles.load(Ordering::SeqCst);

    let refused = manual::capture(&d.deps, &ask(&d.f.repo, "s1"), manual::wall_now_ms());

    assert!(matches!(refused, Err(ManualError::Quota(_))), "{refused:?}");
    assert_eq!(
        d.engine.settles.load(Ordering::SeqCst),
        before,
        "no wait for the engine"
    );
}

fn env() -> Env {
    Env::new(Fixture::with_commit(&git()))
}

fn take_expecting(env: &Env, expected: Option<(u64, u64)>) -> Result<ManualCaptured, ManualError> {
    let a = ask(&env.f.repo, "s1");
    manual::expecting_root(expected, || {
        manual::capture_in_store(
            &env.store,
            &env.oplog,
            &a,
            None,
            false,
            None,
            T0,
            Instant::now() + WAIT,
        )
    })
}

/// Item 3: the root verified by the caller is the one captured; another folder under the same
/// path discards the attempt before anything is recorded.
#[cfg(unix)]
#[test]
fn a_root_swapped_after_the_verification_is_discarded_without_a_row() {
    use std::os::unix::fs::MetadataExt;
    let env = env();
    let root = env.f.repo.canonicalize().unwrap();
    let meta = std::fs::symlink_metadata(&root).unwrap();
    let real = (meta.dev(), meta.ino());

    let swapped = take_expecting(&env, Some((real.0, real.1 + 1)));
    assert!(
        matches!(swapped, Err(ManualError::Discarded)),
        "{swapped:?}"
    );
    assert_eq!(manual_rows_in(&env), 0, "refused before it was recorded");

    let same = take_expecting(&env, Some(real));
    assert!(same.is_ok(), "{same:?}");
    assert_eq!(manual_rows_in(&env), 1);
}

#[cfg(unix)]
fn manual_rows_in(env: &Env) -> i64 {
    manual_rows(&env.dirs)
}

/// Without an expected root (a direct call) nothing is checked.
#[test]
fn without_an_expected_root_the_capture_is_unchanged() {
    let env = env();
    let ok = take_expecting(&env, None);
    assert!(ok.is_ok(), "{ok:?}");
}
