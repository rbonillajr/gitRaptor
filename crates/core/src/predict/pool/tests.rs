//! Criterion tests of T007 (CKPB-POOL-POWER, CKPB-POOL): the `utility` pool of the predictor,
//! over a fake [`DryMerger`] that counts, and optionally blocks until the test lets it go. The
//! jobs are told apart by the `repo` path the merger receives. No fixed sleeps: the test waits on
//! channels with a timeout, on `PowerMode::wait_change` and on `PredictPool::paused`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gitraptor_api::resources::{Pool, PoolClass, PowerSavingSetting, WorkClass};
use gitraptor_git::dry_merge::{
    DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome, MergeBaseUsed,
};

use super::{Done, Job, PREDICT_CONCURRENCY, PredictPool};
use crate::power::PowerMode;
use crate::predict::merger::DryMerger;
use crate::predict::queue::Priority;
use crate::predict::{PairKey, PairSide, Triple};

/// Long enough for a loaded machine, short enough that a missing behaviour fails the test.
const WAIT: Duration = Duration::from_secs(10);
/// How long a thing that must NOT happen is given to happen.
const QUIET: Duration = Duration::from_millis(300);

fn clean_outcome() -> DryMergeOutcome {
    DryMergeOutcome {
        merge_base: MergeBaseUsed::None,
        files: Vec::new(),
        truncated: false,
        rewrites_escalated: false,
    }
}

/// Counts what it receives; a repo named `limit-*` ends in `LimitExceeded`; with a gate, every
/// merge waits for one token of `release` after announcing itself on `started`.
struct FakeMerger {
    calls: Mutex<Vec<PathBuf>>,
    started: Mutex<Sender<PathBuf>>,
    gate: Option<Mutex<Receiver<()>>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl FakeMerger {
    /// A merger that never blocks.
    fn free() -> (Arc<Self>, Receiver<PathBuf>) {
        let (started_tx, started_rx) = mpsc::channel();
        let merger = Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            started: Mutex::new(started_tx),
            gate: None,
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
        });
        (merger, started_rx)
    }

    /// A merger that waits for a token of the returned sender at every merge.
    fn gated() -> (Arc<Self>, Receiver<PathBuf>, Sender<()>) {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let merger = Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            started: Mutex::new(started_tx),
            gate: Some(Mutex::new(release_rx)),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
        });
        (merger, started_rx, release_tx)
    }

    fn count(&self) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }
}

impl DryMerger for FakeMerger {
    fn merge(
        &self,
        repo: &Path,
        _input: &DryMergeInput,
        _limits: &DryMergeLimits,
        _deadline: Instant,
    ) -> Result<DryMergeOutcome, DryMergeError> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(repo.to_owned());
        let _ = self
            .started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(repo.to_owned());
        if let Some(gate) = &self.gate {
            let token = gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv_timeout(WAIT);
            if token.is_err() {
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                return Err(DryMergeError::Unavailable("the test never released".into()));
            }
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if repo.to_string_lossy().starts_with("limit-") {
            Err(DryMergeError::LimitExceeded)
        } else {
            Ok(clean_outcome())
        }
    }
}

fn key(repo: &str, worktree: &str) -> PairKey {
    PairKey {
        repo_id: repo.to_owned(),
        a: PathBuf::from(worktree),
        b: PairSide::Base,
    }
}

fn triple(tip: &str) -> Triple {
    Triple {
        tip_a: tip.to_owned(),
        tip_b: "base".to_owned(),
        merge_base: None,
    }
}

/// A job of the pair (`repo`, `worktree`); the merger sees the `repo` path as `repo`.
fn job(repo: &str, worktree: &str, tip: &str) -> Job {
    Job {
        repo: PathBuf::from(repo),
        key: key(repo, worktree),
        triple: triple(tip),
    }
}

fn start(merger: Arc<FakeMerger>, power: PowerMode) -> (PredictPool, Receiver<Done>) {
    let (done_tx, done_rx) = mpsc::channel();
    let pool = PredictPool::start(merger, power, done_tx).expect("the pool thread starts");
    (pool, done_rx)
}

fn awake() -> PowerMode {
    PowerMode::fixed(false)
}

fn expect_started(started: &Receiver<PathBuf>, what: &str) -> PathBuf {
    started
        .recv_timeout(WAIT)
        .unwrap_or_else(|e| panic!("the pool never handed {what} to the merger: {e}"))
}

fn expect_quiet<T: std::fmt::Debug>(rx: &Receiver<T>, what: &str) {
    match rx.recv_timeout(QUIET) {
        Err(RecvTimeoutError::Timeout) => {}
        other => panic!("{what}, got {other:?}"),
    }
}

fn expect_done(done: &Receiver<Done>) -> Done {
    done.recv_timeout(WAIT)
        .expect("the pool never sent the result of a job")
}

/// Waits until `condition` holds, yielding between looks; fails after [`WAIT`].
fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let end = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

#[test]
fn power_saving_computes_no_pair() {
    let (power, writer) = PowerMode::channel(PowerSavingSetting::On, true);
    let (merger, started) = FakeMerger::free();
    let (pool, done) = start(Arc::clone(&merger), power.clone());

    wait_until("the pool to report paused in power saving", || {
        pool.paused()
    });
    pool.submit(job("r1", "/w/a", "t1"), Priority::ChangedVsBase);
    pool.submit(job("r1", "/w/b", "t1"), Priority::Rest);

    expect_quiet(&started, "in power saving no job may reach the merger");
    expect_quiet(&done, "in power saving no result may come out");
    assert_eq!(merger.count(), 0, "the merger must be called for no pair");
    assert!(pool.paused(), "the pool holds its queue in power saving");
    assert_eq!(pool.queued(), 2, "the held jobs stay queued");

    // Back on AC the held work runs, with no further submit.
    assert!(writer.set(PowerSavingSetting::On, false));
    let first = expect_done(&done);
    let second = expect_done(&done);
    assert_eq!(
        merger.count(),
        2,
        "the held jobs must run once power is back"
    );
    let mut ran = [first.key, second.key];
    ran.sort();
    assert_eq!(ran, [key("r1", "/w/a"), key("r1", "/w/b")]);
    assert!(!pool.paused(), "the pool is no longer paused");
    pool.stop();
}

#[test]
fn power_saving_that_starts_later_pauses_the_pool() {
    let (power, writer) = PowerMode::channel(PowerSavingSetting::Auto, false);
    let (merger, started) = FakeMerger::free();
    let (pool, done) = start(Arc::clone(&merger), power.clone());
    assert!(!pool.paused(), "on AC the pool is not paused");

    let seen = power.version();
    assert!(writer.set(PowerSavingSetting::Auto, true));
    assert!(power.wait_change(seen, WAIT) > seen);
    wait_until("the pool to follow the mode into pause", || pool.paused());

    pool.submit(job("r1", "/w/a", "t1"), Priority::Rest);
    expect_quiet(&started, "a pool paused by power saving takes no job");
    assert_eq!(merger.count(), 0);

    let seen = power.version();
    assert!(writer.set(PowerSavingSetting::Auto, false));
    assert!(power.wait_change(seen, WAIT) > seen);
    expect_done(&done);
    assert_eq!(merger.count(), 1);
    pool.stop();
}

#[test]
fn the_pool_runs_one_job_at_a_time() {
    assert_eq!(
        PREDICT_CONCURRENCY, 1,
        "PREDICT_CONCURRENCY is 1 (§ Valores numéricos)"
    );
    let (merger, started, release) = FakeMerger::gated();
    let (pool, done) = start(Arc::clone(&merger), awake());

    for (i, wt) in ["/w/a", "/w/b", "/w/c"].iter().enumerate() {
        pool.submit(job("r1", wt, &format!("t{i}")), Priority::Rest);
    }
    expect_started(&started, "the first job");
    expect_quiet(
        &started,
        "a second job started while the first was still running",
    );
    assert_eq!(pool.queued(), 2, "the two waiting jobs stay queued");

    for _ in 0..3 {
        release.send(()).unwrap();
    }
    for _ in 0..3 {
        expect_done(&done);
    }
    assert_eq!(merger.count(), 3);
    assert_eq!(merger.max_in_flight(), 1, "never two merges at once");
    pool.stop();
}

#[test]
fn the_pool_thread_is_the_predictor_pool_of_class_utility() {
    let (merger, _started) = FakeMerger::free();
    let (pool, _done) = start(merger, awake());
    let wanted = PoolClass {
        pool: Pool::Predictor,
        class: WorkClass::Utility,
    };
    wait_until(
        "Pool::Predictor of class Utility in work_class::pools()",
        || crate::work_class::pools().contains(&wanted),
    );
    pool.stop();
}

#[test]
fn a_submit_of_the_same_pair_replaces_the_queued_job() {
    let (merger, started, release) = FakeMerger::gated();
    let (pool, done) = start(Arc::clone(&merger), awake());

    pool.submit(job("r-block", "/w/x", "t0"), Priority::Rest);
    expect_started(&started, "the blocking job");
    pool.submit(job("r1", "/w/a", "old"), Priority::Rest);
    pool.submit(job("r1", "/w/a", "new"), Priority::Rest);
    assert_eq!(pool.queued(), 1, "the same pair queues one job, the newest");

    release.send(()).unwrap();
    release.send(()).unwrap();
    let blocker = expect_done(&done);
    assert_eq!(blocker.key, key("r-block", "/w/x"));
    let replaced = expect_done(&done);
    assert_eq!(replaced.key, key("r1", "/w/a"));
    assert_eq!(replaced.triple, triple("new"), "the newest job wins");
    assert!(replaced.result.is_ok());
    assert!(
        replaced.computed_at_utc_ms > 0,
        "the result carries its UTC time"
    );
    expect_quiet(&done, "the replaced job must never run");
    assert_eq!(merger.count(), 2);
    pool.stop();
}

#[test]
fn jobs_come_out_by_priority() {
    let (merger, started, release) = FakeMerger::gated();
    let (pool, done) = start(Arc::clone(&merger), awake());

    pool.submit(job("r-block", "/w/x", "t0"), Priority::Rest);
    expect_started(&started, "the blocking job");
    pool.submit(job("r-rest", "/w/a", "t1"), Priority::Rest);
    pool.submit(job("r-session", "/w/b", "t2"), Priority::PresentSession);
    pool.submit(job("r-changed", "/w/c", "t3"), Priority::ChangedVsBase);

    for _ in 0..4 {
        release.send(()).unwrap();
    }
    let mut order = Vec::new();
    for _ in 0..4 {
        order.push(expect_done(&done).key.repo_id);
    }
    assert_eq!(
        order,
        ["r-block", "r-changed", "r-session", "r-rest"],
        "the pool must take ChangedVsBase, then PresentSession, then Rest"
    );
    pool.stop();
}

#[test]
fn a_limit_exceeded_merge_is_reported_as_the_result() {
    let (merger, _started) = FakeMerger::free();
    let (pool, done) = start(merger, awake());
    pool.submit(job("limit-r", "/w/a", "t1"), Priority::Rest);
    let got = expect_done(&done);
    assert!(
        matches!(got.result, Err(DryMergeError::LimitExceeded)),
        "the merger's error travels in Done.result, got {:?}",
        got.result
    );
    pool.stop();
}

#[test]
fn forget_repo_drops_the_queued_jobs_of_that_repo() {
    let (merger, started, release) = FakeMerger::gated();
    let (pool, done) = start(Arc::clone(&merger), awake());

    pool.submit(job("r-block", "/w/x", "t0"), Priority::Rest);
    expect_started(&started, "the blocking job");
    pool.submit(job("r-forget", "/w/a", "t1"), Priority::Rest);
    pool.submit(job("r-forget", "/w/b", "t1"), Priority::Rest);
    pool.submit(job("r-keep", "/w/c", "t1"), Priority::Rest);
    assert_eq!(pool.queued(), 3);

    pool.forget_repo("r-forget");
    assert_eq!(pool.queued(), 1, "only the job of the other repo stays");

    release.send(()).unwrap();
    release.send(()).unwrap();
    let mut ran = vec![
        expect_done(&done).key.repo_id,
        expect_done(&done).key.repo_id,
    ];
    ran.sort();
    assert_eq!(ran, ["r-block", "r-keep"]);
    expect_quiet(&done, "a forgotten repo's job must never run");
    assert_eq!(merger.count(), 2);
    pool.stop();
}

#[test]
fn stop_ends_the_thread_and_drops_the_queue() {
    let (power, _writer) = PowerMode::channel(PowerSavingSetting::On, true);
    let (merger, _started) = FakeMerger::free();
    let (pool, done) = start(Arc::clone(&merger), power);
    wait_until("the pool to report paused", || pool.paused());
    pool.submit(job("r1", "/w/a", "t1"), Priority::Rest);
    assert_eq!(pool.queued(), 1);

    pool.stop();
    // The thread owned the only sender of `done`: once it ends, the channel is closed with no
    // result in it.
    match done.recv_timeout(WAIT) {
        Err(RecvTimeoutError::Disconnected) => {}
        other => panic!("stop must end the thread and drop the queue, got {other:?}"),
    }
    assert_eq!(merger.count(), 0, "a dropped job is never merged");
}
