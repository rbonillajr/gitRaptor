//! The manual snapshot of the Time Machine (DS-US-MCP-008 T003): quota, ceilings, free-space
//! floor, atomic recording and the promise that a manual point never touches the user's repo
//! and never gets in the way of a guaranteed prior (NFR-01).
//!
//! Every test runs on a testkit fixture: a temporary repo, home and profile, never this repo or
//! the real profile. The clock is the `now_ms` the quota functions take: instants of the past
//! are passed, nothing waits. The one wait (an in-flight capture) is a state with a deadline.
//!
//! Written test-first: the manual rows are counted with plain SQL over the oplog file
//! (`snapshots` rows of level `manual`, each one an attempt that reached capture), the way
//! another process of the user could read it.

mod tm_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitraptor_api::methods::QuotaWindow;
use gitraptor_core::daemon::{LogLimits, Logger};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::continuous::{CaptureDeps, FreeSpaceFloor};
use gitraptor_core::timemachine::engine::{EngineLink, RawGitEvent};
use gitraptor_core::timemachine::manual::{
    self, DAY_MS, FreeSpaceProbe, MINUTE_MS, ManualAsk, ManualCaptured, ManualError, ManualFloor,
    PER_DAY, PER_MINUTE, PER_REPO_DAY, PER_WORKTREE_DAY, QuotaHit, QuotaInput,
};
use gitraptor_core::timemachine::oplog::{
    Channel, Oplog, Requester, RequesterOrigin, SnapshotLevel,
};
use gitraptor_core::timemachine::protected::{DEFAULT_PRIOR_DEADLINE, TmRepos};
use gitraptor_core::timemachine::store::{
    CaptureRequest, SnapshotStore, WorktreeScope, snapshot_refs,
};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Scope, Snapshot, diff};
use rusqlite::Connection;
use tm_common::{Env, REPO_ID, git};

/// 2024-10-04, in ms: far from any real clock the code could fall back to.
const T0: i64 = 1_728_000_000_000;
const DEADLINE: Duration = Duration::from_secs(20);

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

fn budget() -> Instant {
    Instant::now() + Duration::from_secs(25)
}

/// One manual capture at the instant `now_ms` of the test's clock.
fn take(env: &Env, ask: &ManualAsk, now_ms: i64) -> Result<ManualCaptured, ManualError> {
    manual::capture_in_store(
        &env.store,
        &env.oplog,
        ask,
        None,
        false,
        None,
        now_ms,
        budget(),
    )
}

fn ok(result: Result<ManualCaptured, ManualError>, what: &str) -> ManualCaptured {
    match result {
        Ok(point) => point,
        Err(e) => panic!("{what}: expected a point, got {e:?}"),
    }
}

/// The quota hit of a refused capture; panics with what happened otherwise.
fn refused(result: Result<ManualCaptured, ManualError>, what: &str) -> QuotaHit {
    match result {
        Err(ManualError::Quota(hit)) => hit,
        other => panic!("{what}: expected a quota refusal, got {other:?}"),
    }
}

fn oplog_file(env: &Env) -> PathBuf {
    env.tm_root().join(REPO_ID).join("oplog.db")
}

/// Rows of level `manual`: every attempt that reached capture, discarded ones included.
fn manual_rows(env: &Env) -> i64 {
    Connection::open(oplog_file(env))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM snapshots WHERE level = 'manual'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn refs(env: &Env) -> BTreeSet<String> {
    snapshot_refs(&env.store).unwrap().into_keys().collect()
}

/// A linked worktree on its own branch.
fn worktree(env: &Env, name: &str) -> PathBuf {
    env.f.git(&["branch", name]);
    env.f.add_worktree(name, name)
}

fn env() -> Env {
    Env::new(Fixture::with_commit(&git()))
}

/// A guaranteed prior of the main worktree, taken the way an operation does.
fn prior(env: &Env) -> String {
    env.prior().snapshot_id
}

// ---------------------------------------------------------------------------- quota per window

#[test]
fn the_sixth_manual_snapshot_in_a_minute_is_refused_with_the_seconds_to_wait() {
    let env = env();
    let wt = env.f.repo.clone();
    let mut taken = Vec::new();
    for i in 0..PER_MINUTE as i64 {
        taken.push(
            ok(
                take(&env, &ask(&wt, "s1"), T0 + i * 1_000),
                "within the quota",
            )
            .snapshot_id,
        );
    }
    let before = refs(&env);
    assert_eq!(before.len(), PER_MINUTE);

    // The sixth, ten seconds in: refused, with when the oldest leaves the window.
    let hit = refused(
        take(&env, &ask(&wt, "s1"), T0 + 10_000),
        "sixth in a minute",
    );
    assert_eq!(hit.window, QuotaWindow::Minute);
    assert_eq!(
        hit.release_at_ms,
        T0 + MINUTE_MS,
        "the oldest attempt plus the window"
    );
    assert_eq!(
        (hit.release_at_ms - (T0 + 10_000)) / 1_000,
        50,
        "seconds to wait"
    );

    // Nothing was deleted, nothing was added, and the refusal left no row.
    assert_eq!(refs(&env), before);
    assert_eq!(manual_rows(&env), PER_MINUTE as i64);
    for id in &taken {
        assert!(
            before.iter().any(|r| r.ends_with(id.as_str())),
            "{id} is still there"
        );
    }

    // The guaranteed prior is not touched by the manual quota.
    let p = prior(&env);
    assert!(refs(&env).iter().any(|r| r.ends_with(&p)));

    // The window is mobile: one millisecond before the oldest leaves, still refused; at the
    // instant it leaves, one slot is free.
    refused(
        take(&env, &ask(&wt, "s1"), T0 + MINUTE_MS - 1),
        "just before the release",
    );
    ok(
        take(&env, &ask(&wt, "s1"), T0 + MINUTE_MS),
        "at the release",
    );
}

#[test]
fn the_twenty_first_in_24h_is_refused_with_the_release_time() {
    let env = env();
    let wt = env.f.repo.clone();
    // 61 s apart: never more than one in a minute, so only the day window can refuse.
    for i in 0..PER_DAY as i64 {
        ok(
            take(&env, &ask(&wt, "s1"), T0 + i * 61_000),
            "within the day quota",
        );
    }
    let before = refs(&env);
    assert_eq!(before.len(), PER_DAY);

    let now = T0 + PER_DAY as i64 * 61_000;
    let hit = refused(take(&env, &ask(&wt, "s1"), now), "twenty-first in 24 h");
    assert_eq!(hit.window, QuotaWindow::Day);
    assert_eq!(
        hit.release_at_ms,
        T0 + DAY_MS,
        "when the oldest leaves the window"
    );

    // Nothing deleted, the refusal left no row, and a guaranteed prior still completes.
    assert_eq!(refs(&env), before);
    assert_eq!(manual_rows(&env), PER_DAY as i64);
    let p = prior(&env);
    assert_eq!(refs(&env).len(), PER_DAY + 1);
    assert!(refs(&env).iter().any(|r| r.ends_with(&p)));
}

// ------------------------------------------------------------------------------------ ceilings

#[test]
fn rotating_sessions_cannot_exceed_the_worktree_ceiling() {
    let env = env();
    let wt = env.f.repo.clone();
    // One attempt per session: no session is near its own quota.
    for i in 0..PER_WORKTREE_DAY {
        ok(
            take(&env, &ask(&wt, &format!("session-{i}")), T0 + i as i64),
            "each session within its own quota",
        );
    }
    let hit = refused(
        take(
            &env,
            &ask(&wt, "a-new-session"),
            T0 + PER_WORKTREE_DAY as i64,
        ),
        "above the worktree ceiling",
    );
    assert_eq!(hit.window, QuotaWindow::WorktreeDay);
    assert_eq!(hit.release_at_ms, T0 + DAY_MS);
    assert_eq!(manual_rows(&env), PER_WORKTREE_DAY as i64);
    assert_eq!(
        refs(&env).len(),
        PER_WORKTREE_DAY,
        "nothing deleted to make room"
    );
}

#[test]
fn rotating_worktrees_cannot_exceed_the_repo_ceiling() {
    let env = env();
    // Below the ceiling of each worktree, above the one of the repo.
    let per_worktree = PER_REPO_DAY / 4;
    assert!(per_worktree < PER_WORKTREE_DAY);
    let worktrees: Vec<PathBuf> = (0..5).map(|i| worktree(&env, &format!("rot{i}"))).collect();
    let mut n = 0;
    for wt in &worktrees[..4] {
        for _ in 0..per_worktree {
            ok(
                take(&env, &ask(wt, &format!("session-{n}")), T0 + n),
                "within every ceiling but the repo's",
            );
            n += 1;
        }
    }
    assert_eq!(n as usize, PER_REPO_DAY);
    let hit = refused(
        take(&env, &ask(&worktrees[4], "a-new-session"), T0 + n),
        "above the repo ceiling",
    );
    assert_eq!(hit.window, QuotaWindow::RepoDay);
    assert_eq!(hit.release_at_ms, T0 + DAY_MS);
    assert_eq!(manual_rows(&env), PER_REPO_DAY as i64);
}

// ---------------------------------------------------------------------------- the worktree key

/// Two worktrees whose keys look alike (a `_` is a wildcard of `LIKE`, one is a prefix of the
/// other): the quota of one never counts the other (K1).
#[test]
fn the_quota_matches_the_worktree_key_exactly() {
    let env = env();
    let a = worktree(&env, "feat_a");
    let b = worktree(&env, "featxa");
    let c = worktree(&env, "feat_a2");
    for i in 0..PER_DAY as i64 {
        ok(
            take(&env, &ask(&a, "s1"), T0 + i * 61_000),
            "within the day quota of feat_a",
        );
    }
    let now = T0 + PER_DAY as i64 * 61_000;
    assert_eq!(
        refused(take(&env, &ask(&a, "s1"), now), "feat_a is full").window,
        QuotaWindow::Day
    );
    // The same session, in the worktrees with similar keys, has all of its quota.
    ok(
        take(&env, &ask(&b, "s1"), now),
        "featxa is another worktree",
    );
    ok(
        take(&env, &ask(&c, "s1"), now),
        "feat_a2 is another worktree",
    );
    // And the other way round: filling `featxa` does not use up `feat_a`'s.
    assert_eq!(manual_rows(&env), PER_DAY as i64 + 2);
}

// -------------------------------------------------------------------------------- the clock

#[test]
fn windows_have_no_upper_bound_when_the_clock_goes_back() {
    // Pure: stamps after `now` are still inside the window.
    let stamps: Vec<i64> = (0..PER_MINUTE as i64).map(|i| T0 + i).collect();
    let input = QuotaInput {
        requester_ms: stamps.clone(),
        worktree_ms: stamps.clone(),
        repo_ms: stamps,
    };
    for back in [1, 1_000, MINUTE_MS, 3 * 3_600_000, DAY_MS * 400] {
        let hit = manual::quota(&input, T0 - back).unwrap_err();
        assert_eq!(hit.window, QuotaWindow::Minute, "clock back by {back} ms");
    }
    // One stamp more than the day quota, all in the "future" of the clock.
    let many: Vec<i64> = (0..=PER_DAY as i64).map(|i| T0 + i * 100_000).collect();
    let input = QuotaInput {
        requester_ms: many.clone(),
        worktree_ms: many.clone(),
        repo_ms: many,
    };
    assert!(manual::quota(&input, T0 - 7_200_000).is_err());

    // Through the oplog: five attempts, then the clock goes back an hour. The reading of the
    // window must not be `BETWEEN now - window AND now`.
    let env = env();
    let wt = env.f.repo.clone();
    for i in 0..PER_MINUTE as i64 {
        ok(take(&env, &ask(&wt, "s1"), T0 + i), "within the quota");
    }
    let hit = refused(
        take(&env, &ask(&wt, "s1"), T0 - 3_600_000),
        "the clock went back an hour",
    );
    assert_eq!(hit.window, QuotaWindow::Minute);
    assert_eq!(manual_rows(&env), PER_MINUTE as i64);
}

// ----------------------------------------------------------------------------- consumed quota

/// A deadline already past: the capture reaches the worktree and is discarded at its validity
/// point, so there is no point (and the attempt still counts, C1).
fn expired() -> Instant {
    let now = Instant::now();
    now.checked_sub(Duration::from_secs(1)).unwrap_or(now)
}

fn discard(env: &Env, ask: &ManualAsk, now_ms: i64) {
    let result = manual::capture_in_store(
        &env.store,
        &env.oplog,
        ask,
        None,
        false,
        None,
        now_ms,
        expired(),
    );
    assert!(
        matches!(result, Err(ManualError::Discarded)),
        "expected a discarded capture, got {result:?}"
    );
}

#[test]
fn discarded_captures_consume_quota() {
    let env = env();
    let wt = env.f.repo.clone();

    // Four attempts that reach capture and are discarded, then one point: five attempts.
    for i in 0..4 {
        discard(&env, &ask(&wt, "s1"), T0 + i);
    }
    assert!(refs(&env).is_empty(), "a discarded capture leaves no point");
    ok(
        take(&env, &ask(&wt, "s1"), T0 + 4),
        "the fifth attempt of the minute",
    );
    let hit = refused(take(&env, &ask(&wt, "s1"), T0 + 5), "the sixth attempt");
    assert_eq!(
        hit.window,
        QuotaWindow::Minute,
        "discards count in the minute"
    );

    // Refusals before capture (quota) do not count: eight more refusals add no row.
    for i in 0..8 {
        refused(take(&env, &ask(&wt, "s1"), T0 + 6 + i), "still refused");
    }
    assert_eq!(manual_rows(&env), 5);

    // They count in the day too: 15 more spaced attempts fill the 20, the next is refused.
    for i in 1..=15 {
        ok(
            take(&env, &ask(&wt, "s1"), T0 + i * 61_000),
            "within the day quota",
        );
    }
    let hit = refused(
        take(&env, &ask(&wt, "s1"), T0 + 16 * 61_000),
        "twenty-first",
    );
    assert_eq!(hit.window, QuotaWindow::Day, "4 discarded + 16 taken = 20");

    // And in the ceilings: sixty attempts discarded by sixty sessions fill the worktree's.
    let env = self::env();
    let wt = env.f.repo.clone();
    for i in 0..PER_WORKTREE_DAY {
        discard(&env, &ask(&wt, &format!("session-{i}")), T0 + i as i64);
    }
    let hit = refused(
        take(
            &env,
            &ask(&wt, "a-new-session"),
            T0 + PER_WORKTREE_DAY as i64,
        ),
        "the discarded attempts filled the ceiling",
    );
    assert_eq!(hit.window, QuotaWindow::WorktreeDay);
    assert!(refs(&env).is_empty());
}

// ------------------------------------------------------------------------------------- NFR-01

#[test]
fn a_manual_snapshot_never_modifies_the_worktree() {
    // A busy repo: staged, unstaged and untracked changes, an ignored folder and a dirty stat.
    let env = Env::busy();
    let linked = worktree(&env, "side");
    let scopes = [
        Scope::new("main", env.f.repo.clone()),
        Scope::new("side", linked.clone()),
    ];
    let head = env.f.git(&["rev-parse", "HEAD"]);
    let status = env.f.git(&["status", "--porcelain=v2", "--branch"]);
    // After the reads: `git status` refreshes the stat of a dirty `.git/index`, which is Git's write, not ours.
    let before = Snapshot::take(&scopes, &BTreeSet::new());

    ok(
        take(&env, &ask(&env.f.repo, "s1"), T0),
        "a manual point of the main worktree",
    );
    ok(
        take(&env, &ask(&linked, "s1"), T0 + 1),
        "a manual point of the linked worktree",
    );

    let changes = diff(&before, &Snapshot::take(&scopes, &BTreeSet::new()));
    assert!(
        changes.is_empty(),
        "the repo and its .git changed: {changes:#?}"
    );
    assert_eq!(env.f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(env.f.git(&["status", "--porcelain=v2", "--branch"]), status);
}

// -------------------------------------------------------------------- the free-space floor

/// An engine that is always calm, for the daemon's path (`manual::capture`).
struct Calm;

impl EngineLink for Calm {
    fn mark(&self, _repo_id: &str) -> i64 {
        1
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
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

/// The daemon's wiring for one repo of a fixture: the repo registered in `TmRepos` with its
/// oplog, and the store opened by the test for the guaranteed priors.
struct Daemon {
    f: Fixture,
    dirs: ProfileDirs,
    repos: Arc<TmRepos>,
    oplog: Arc<std::sync::Mutex<Oplog>>,
    store: SnapshotStore,
}

fn daemon() -> Daemon {
    let f = Fixture::with_commit(&git());
    let dirs = ProfileDirs::under_root(&f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
    let common = f.repo.join(".git").canonicalize().unwrap();
    repos.insert(REPO_ID, &common, oplog);
    let oplog = repos.oplog(REPO_ID).unwrap();
    let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
    Daemon {
        f,
        dirs,
        repos,
        oplog,
        store,
    }
}

impl Daemon {
    fn deps(&self, floor: Option<FreeSpaceFloor>) -> CaptureDeps {
        CaptureDeps {
            repos: Arc::clone(&self.repos),
            engine: Arc::new(Calm),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: floor,
        }
    }

    fn prior(&self) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::GuaranteedPrior,
            repo: self.f.repo.clone(),
            worktrees: vec![WorktreeScope {
                key: "main".into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: None,
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.store.capture(&self.oplog, &req).unwrap().snapshot_id
    }

    fn refs(&self) -> BTreeSet<String> {
        snapshot_refs(&self.store).unwrap().into_keys().collect()
    }
}

/// A floor no volume can meet: the disk is "at the floor".
fn full_disk() -> FreeSpaceFloor {
    FreeSpaceFloor {
        bytes: u64::MAX,
        percent: 100,
    }
}

#[test]
fn with_the_disk_at_the_floor_a_later_guaranteed_prior_completes() {
    let d = daemon();
    // A point that exists before the disk fills up: the refusal must not delete it.
    let existing = d.prior();
    let before = d.refs();
    assert_eq!(before.len(), 1);

    let refused = manual::capture(&d.deps(Some(full_disk())), &ask(&d.f.repo, "s1"), T0);
    assert!(
        matches!(refused, Err(ManualError::NoSpace)),
        "a manual capture with the disk at the floor must be refused, got {refused:?}"
    );
    assert_eq!(d.refs(), before, "nothing captured and nothing deleted");

    // A guaranteed prior is not subject to the manual floor: it completes.
    let later = d.prior();
    let after = d.refs();
    assert_eq!(after.len(), 2);
    assert!(after.iter().any(|r| r.ends_with(&later)));
    assert!(after.iter().any(|r| r.ends_with(&existing)));
}

// ------------------------------------------------------------------- never delaying a prior

/// A worktree heavy enough for a manual capture to last a moment.
fn heavy(env: &Env) {
    for i in 0..30 {
        let mut bytes = vec![0u8; 1024 * 1024];
        // Incompressible enough, and different per file: no deduplication.
        let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ (i as u64 + 1);
        for chunk in bytes.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let word = x.to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        std::fs::write(env.f.repo.join(format!("blob-{i}.bin")), bytes).unwrap();
    }
}

#[test]
fn a_manual_capture_never_delays_a_prior() {
    let env = env();
    heavy(&env);
    let wt = env.f.repo.clone();
    std::thread::scope(|scope| {
        let manual_capture = scope.spawn(|| take(&env, &ask(&wt, "s1"), T0));
        // The signal that the manual capture is under way: its attempt is on the oplog (the
        // row is written when it starts, before the worktree is read).
        let start = Instant::now();
        while manual_rows(&env) == 0 {
            assert!(
                start.elapsed() < DEADLINE,
                "the manual capture never started"
            );
            assert!(
                !manual_capture.is_finished(),
                "it ended without ever recording"
            );
            std::thread::yield_now();
        }

        let asked = Instant::now();
        let p = env.prior();
        assert!(
            asked.elapsed() < DEFAULT_PRIOR_DEADLINE,
            "the prior waited {:?} for the manual capture",
            asked.elapsed()
        );
        assert!(refs(&env).iter().any(|r| r.ends_with(&p.snapshot_id)));

        // The manual capture gives way or ends; it never blocks the prior and never panics.
        let result = manual_capture
            .join()
            .expect("the manual capture did not panic");
        assert!(
            matches!(
                result,
                Ok(_) | Err(ManualError::Discarded | ManualError::Capture(_))
            ),
            "{result:?}"
        );
    });
}

// ------------------------------------------------------------ the floor, with a scripted probe

/// Free space that is above the floor for the first `above` readings and zero after.
struct Scripted {
    above: usize,
    calls: std::sync::atomic::AtomicUsize,
}

impl Scripted {
    fn new(above: usize) -> Self {
        Self {
            above,
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl FreeSpaceProbe for Scripted {
    fn available_bytes(&self, _path: &Path) -> std::io::Result<u64> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if n < self.above { 1_000_000_000 } else { 0 })
    }
}

fn manual_floor(probe: &Scripted) -> ManualFloor<'_> {
    ManualFloor {
        floor: FreeSpaceFloor {
            bytes: 1_000,
            percent: 0,
        },
        reserve_bytes: 0,
        probe,
    }
}

/// S1: the pre-check saw room; under the manual lock the space is below the floor, so nothing
/// is captured and nothing is recorded.
#[test]
fn the_floor_is_checked_again_under_the_lock() {
    let env = env();
    let wt = env.f.repo.clone();
    let existing = prior(&env);
    // The pre-check (no probe: it reads the oplog only) lets the request through.
    let session = "s1";
    manual::precheck(&env.oplog, &env.store, session, "any-key", T0).unwrap();

    let probe = Scripted::new(0);
    let floor = manual_floor(&probe);
    let result = manual::capture_in_store(
        &env.store,
        &env.oplog,
        &ask(&wt, session),
        None,
        false,
        Some(&floor),
        T0,
        budget(),
    );

    assert!(matches!(result, Err(ManualError::NoSpace)), "{result:?}");
    assert_eq!(
        manual_rows(&env),
        0,
        "refused before capturing: no attempt recorded"
    );
    assert_eq!(refs(&env).len(), 1, "no point added");
    assert!(
        refs(&env).iter().any(|r| r.ends_with(&existing)),
        "nothing deleted"
    );
}

/// S2: room at the start, the floor is crossed while files are written: the attempt is
/// discarded (it counts, C1), no point exists and nothing is deleted.
#[test]
fn crossing_the_manual_floor_mid_capture_discards_and_deletes_nothing() {
    let env = env();
    let wt = env.f.repo.clone();
    let existing = prior(&env);
    let before = refs(&env);

    // Above for the check under the lock and the first file; below after.
    let probe = Scripted::new(2);
    let floor = manual_floor(&probe);
    let result = manual::capture_in_store(
        &env.store,
        &env.oplog,
        &ask(&wt, "s1"),
        None,
        false,
        Some(&floor),
        T0,
        budget(),
    );

    assert!(matches!(result, Err(ManualError::Discarded)), "{result:?}");
    assert_eq!(manual_rows(&env), 1, "the discarded attempt counts");
    assert_eq!(refs(&env), before, "no point, nothing deleted");
    assert!(refs(&env).iter().any(|r| r.ends_with(&existing)));
}
