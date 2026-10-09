//! Continuous capture: level (b) of ADR-TMC-004 § 2 (US-TMC-004).
//!
//! What agents or the developer do with raw Git or in the editor is captured from what the engine
//! already observed, outside its 300 ms budget: the observer only hands a signal to a channel and
//! never waits. One thread captures, one worktree at a time:
//!
//! - **File activity**: when the worktree has been quiet for `Q`, or at most every `M` while the
//!   activity goes on.
//! - **A persisted Git event**: right away, with the event as its cause.
//!
//! Every capture first waits for the engine to be calm (so its mark covers every `git` that
//! already ended), and is discarded at its validity point if the repo's Git directory changed or
//! the index got locked meanwhile: a point never mixes the state before and after a `git`. A
//! capture that fails creates no point and says so in the log.
//!
//! After an operation of GitRaptor (an undo too) [`anchor`] takes the state it left, with the
//! operation as cause: the raw events up to the anchor's mark are its echo, not raw Git.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gitraptor_git::{ReaderOptions, RepoReader};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::engine::{EngineLink, GitState, SETTLE};
use super::oplog::SnapshotLevel;
use super::protected::{TmRepos, registered_worktrees, worktree_key};
use super::store::{CaptureError, CaptureOutcome, CaptureRequest, ValidityGuard, WorktreeScope};
use crate::daemon::{Field, Logger};
use crate::profile::ProfileDirs;
use crate::watch::ObserverHooks;

/// Cadence of the continuous capture. `Q` and `M` are internal values, confirmed by
/// SPIKE-TMC-001 on macOS (⚠️ ASSUMPTION on Linux and Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    pub enabled: bool,
    /// `Q`: quiet time of a worktree before it is captured.
    pub quiet: Duration,
    /// `M`: longest time between captures while the activity goes on.
    pub max_interval: Duration,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quiet: Duration::from_secs(1),
            max_interval: Duration::from_secs(5),
        }
    }
}

/// Fault injection for tests: answers a capture request with an error instead of capturing.
/// Honored only in debug builds.
pub type CaptureLayer = Arc<dyn Fn(&CaptureRequest) -> Option<CaptureError> + Send + Sync>;

/// Longest wait for a calm engine before a capture; past it the capture is tried again later.
const CAPTURE_SETTLE_LIMIT: Duration = Duration::from_secs(1);
/// Longest wait for a calm engine after an operation, before its anchor.
pub const ANCHOR_SETTLE_LIMIT: Duration = Duration::from_secs(2);
/// A capture discarded or given way this many times in a row is dropped until new activity.
const MAX_RETRIES: u32 = 50;

enum Signal {
    Activity {
        repo_id: String,
        worktree: PathBuf,
    },
    GitEvent {
        repo_id: String,
        worktree: PathBuf,
        seq: i64,
    },
    Forget {
        repo_id: String,
    },
    Stop,
}

/// The running continuous capture.
pub struct ContinuousCapture {
    tx: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

/// What one capture needs from the daemon.
#[derive(Clone)]
pub struct CaptureDeps {
    pub repos: Arc<TmRepos>,
    pub engine: Arc<dyn EngineLink>,
    pub profile: ProfileDirs,
    pub logger: Logger,
    pub layer: Option<CaptureLayer>,
    /// Free space the capture leaves to the user (SEC-TMC-12: max(5 GB, 5 %)); `None` in tests
    /// that do not check it.
    pub free_space_floor: Option<FreeSpaceFloor>,
}

/// The free space under which the continuous capture stops (SEC-TMC-12): the larger of `bytes`
/// and `percent` of the volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeSpaceFloor {
    pub bytes: u64,
    pub percent: u8,
}

impl Default for FreeSpaceFloor {
    fn default() -> Self {
        Self {
            bytes: 5 * 1024 * 1024 * 1024,
            percent: 5,
        }
    }
}

/// Whether the volume of the profile's data folder has less free space than `floor`. Unknown
/// counts as enough: the store's own write then fails as no space if it really is full.
fn below_floor(profile: &ProfileDirs, floor: FreeSpaceFloor) -> bool {
    #[cfg(unix)]
    {
        let Ok(st) = rustix::fs::statvfs(&profile.data) else {
            return false;
        };
        let frsize = st.f_frsize.max(1);
        let free = st.f_bavail.saturating_mul(frsize);
        let total = st.f_blocks.saturating_mul(frsize);
        let pct = total / 100 * u64::from(floor.percent);
        free < floor.bytes.max(pct)
    }
    #[cfg(not(unix))]
    {
        let _ = (profile, floor);
        false
    }
}

impl ContinuousCapture {
    pub fn start(config: CaptureConfig, deps: CaptureDeps) -> std::io::Result<Self> {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("raptor-tm-capture".into())
            .spawn(move || run(config, deps, rx))?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }

    /// Files of a worktree changed (never blocks).
    pub fn activity(&self, repo_id: &str, worktree: &Path) {
        let _ = self.tx.send(Signal::Activity {
            repo_id: repo_id.to_owned(),
            worktree: worktree.to_path_buf(),
        });
    }

    /// The engine persisted a Git event of a worktree (never blocks).
    pub fn git_event(&self, repo_id: &str, worktree: &Path, seq: i64) {
        let _ = self.tx.send(Signal::GitEvent {
            repo_id: repo_id.to_owned(),
            worktree: worktree.to_path_buf(),
            seq,
        });
    }

    /// A repo is no longer observed.
    pub fn forget(&self, repo_id: &str) {
        let _ = self.tx.send(Signal::Forget {
            repo_id: repo_id.to_owned(),
        });
    }

    /// The hooks the observer calls: Git directory activity for the engine marks, file activity
    /// for the capture.
    pub fn observer_hooks(&self, marks: Arc<super::engine::RepoMarks>) -> Arc<dyn ObserverHooks> {
        Arc::new(Hooks {
            marks,
            tx: std::sync::Mutex::new(self.tx.clone()),
        })
    }

    /// Stops the thread; a capture in progress ends first.
    pub fn stop(mut self) {
        let _ = self.tx.send(Signal::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ContinuousCapture {
    fn drop(&mut self) {
        let _ = self.tx.send(Signal::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Hooks {
    marks: Arc<super::engine::RepoMarks>,
    tx: std::sync::Mutex<Sender<Signal>>,
}

impl ObserverHooks for Hooks {
    fn worktree_touched(&self, _repo_id: &str, _root: &Path) {}

    fn git_dir_touched(&self, repo_id: &str, t_recv: u64) {
        // The time it reached the router, not the window's first event: the calm is measured
        // from the last change.
        let _ = t_recv;
        self.marks
            .touch(repo_id, gitraptor_api::clock::monotonic_ns());
    }

    fn worktree_changed(&self, repo_id: &str, root: &Path) {
        self.marks.bump_activity(repo_id);
        let _ = self
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send(Signal::Activity {
                repo_id: repo_id.to_owned(),
                worktree: root.to_path_buf(),
            });
    }
}

/// One worktree waiting to be captured.
struct Pending {
    first: Instant,
    last: Instant,
    /// Latest Git event not captured yet.
    event: Option<i64>,
    retry_at: Option<Instant>,
    retries: u32,
}

impl Pending {
    fn due(&self, config: &CaptureConfig) -> Instant {
        if let Some(at) = self.retry_at {
            return at;
        }
        if self.event.is_some() {
            return self.last;
        }
        (self.last + config.quiet).min(self.first + config.max_interval)
    }
}

/// The seeding of a repo's store, once per run (ADR-TMC-001 § 3).
enum Seed {
    /// Seeding in the background; its captures wait.
    Running(Arc<std::sync::atomic::AtomicBool>),
    Done,
}

/// Indexing limits of the seeding the continuous capture starts: one thread and a bounded
/// allocation, so it never competes with the engine (US-TMC-004).
fn seed_limits() -> gitraptor_git::tm_write::store::SeedLimits {
    gitraptor_git::tm_write::store::SeedLimits {
        threads: 1,
        alloc_limit_bytes: 256 << 20,
    }
}

/// Whether the store of `repo_id` can take a capture of `worktree` that only copies what is new.
/// A store that lacks the worktree's `HEAD` commit is seeded first, in the background: without
/// it, the first capture of a large repo would copy its whole history object by object.
fn seeded(
    deps: &CaptureDeps,
    seeds: &mut HashMap<String, Seed>,
    repo_id: &str,
    worktree: &Path,
) -> bool {
    match seeds.get(repo_id) {
        Some(Seed::Done) => return true,
        Some(Seed::Running(done)) => {
            if done.load(std::sync::atomic::Ordering::Acquire) {
                seeds.insert(repo_id.to_owned(), Seed::Done);
                return true;
            }
            return false;
        }
        None => {}
    }
    let (Some((_, Some(store))), Some(common)) =
        (deps.repos.repo(repo_id), deps.repos.common_dir(repo_id))
    else {
        // No store: the capture says so itself.
        return true;
    };
    if store.has_head_of(&common, worktree) {
        seeds.insert(repo_id.to_owned(), Seed::Done);
        return true;
    }
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let logger = deps.logger.clone();
    let repo = repo_id.to_owned();
    // The packs are read from the repo's own Git folder, never through a worktree's `.git`.
    let path = common;
    let spawned = std::thread::Builder::new()
        .name("raptor-tm-seed".into())
        .spawn(move || {
            let started = Instant::now();
            let ms = || i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            match store.seed_with(&path, seed_limits()) {
                Ok(report) => logger.info(
                    "tm_seeded",
                    &[
                        ("repo", Field::id(&repo)),
                        (
                            "objects",
                            i64::try_from(report.objects).unwrap_or(i64::MAX).into(),
                        ),
                        ("skipped", report.skipped.len().into()),
                        ("ms", ms().into()),
                    ],
                ),
                // A capture then copies what it lacks.
                Err(_) => logger.warn(
                    "tm_seed_failed",
                    &[("repo", Field::id(&repo)), ("ms", ms().into())],
                ),
            }
            flag.store(true, std::sync::atomic::Ordering::Release);
        });
    if spawned.is_err() {
        seeds.insert(repo_id.to_owned(), Seed::Done);
        return true;
    }
    seeds.insert(repo_id.to_owned(), Seed::Running(done));
    false
}

/// While a repo's store seeds, its captures are tried again this often.
const SEED_WAIT: Duration = Duration::from_secs(1);

fn run(config: CaptureConfig, deps: CaptureDeps, rx: Receiver<Signal>) {
    let mut pending: HashMap<(String, PathBuf), Pending> = HashMap::new();
    let mut seeds: HashMap<String, Seed> = HashMap::new();
    loop {
        let next = pending.values().map(|p| p.due(&config)).min();
        let signal = match next {
            Some(due) => match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                Ok(s) => Some(s),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(s) => Some(s),
                Err(_) => return,
            },
        };
        // Everything already queued, so the latest request per worktree wins (coalescing).
        let mut signals: Vec<Signal> = signal.into_iter().collect();
        while let Ok(s) = rx.try_recv() {
            signals.push(s);
        }
        for s in signals {
            let now = Instant::now();
            match s {
                Signal::Stop => return,
                Signal::Forget { repo_id } => {
                    pending.retain(|(r, _), _| *r != repo_id);
                    seeds.remove(&repo_id);
                }
                Signal::Activity { repo_id, worktree } => {
                    if !config.enabled {
                        continue;
                    }
                    let p = pending.entry((repo_id, worktree)).or_insert(Pending {
                        first: now,
                        last: now,
                        event: None,
                        retry_at: None,
                        retries: 0,
                    });
                    p.last = now;
                }
                Signal::GitEvent {
                    repo_id,
                    worktree,
                    seq,
                } => {
                    if !config.enabled {
                        continue;
                    }
                    let p = pending.entry((repo_id, worktree)).or_insert(Pending {
                        first: now,
                        last: now,
                        event: None,
                        retry_at: None,
                        retries: 0,
                    });
                    p.last = now;
                    p.event = Some(p.event.map_or(seq, |e| e.max(seq)));
                    p.retry_at = None;
                }
            }
        }
        let now = Instant::now();
        let due: Vec<(String, PathBuf)> = pending
            .iter()
            .filter(|(_, p)| p.due(&config) <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for key in due {
            let Some(p) = pending.get_mut(&key) else {
                continue;
            };
            if !seeded(&deps, &mut seeds, &key.0, &key.1) {
                p.retry_at = Some(Instant::now() + SEED_WAIT);
                continue;
            }
            let attempt = capture_one(&deps, &key.0, &key.1, p.event, p.give_way(&config));
            if !p.after(attempt, Instant::now(), &config) {
                if p.retries > MAX_RETRIES {
                    deps.logger
                        .warn("tm_capture_gave_up", &[("repo", Field::id(&key.0))]);
                }
                pending.remove(&key);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attempt {
    /// Captured, or failed for good (the change has no point).
    Done,
    /// Not consistent or not calm yet: try again shortly.
    Again,
    /// Gave way to new activity of the repo: try again once it is quiet. Never counts as a
    /// failure, so sustained activity never makes the capture give up.
    GaveWay,
}

impl Pending {
    /// Whether the capture gives way to new activity of its repo: not once it has waited six
    /// times `M`, so sustained activity still gets a capture (at most every 6×M while it lasts).
    fn give_way(&self, config: &CaptureConfig) -> bool {
        self.first.elapsed() < config.max_interval * 6
    }

    /// After an attempt at `now`: whether the worktree stays pending, and when it is tried again.
    fn after(&mut self, attempt: Attempt, now: Instant, config: &CaptureConfig) -> bool {
        match attempt {
            Attempt::Done => false,
            Attempt::GaveWay => {
                self.retry_at = Some(now + config.quiet);
                true
            }
            Attempt::Again => {
                self.retries += 1;
                self.retry_at = Some(now + SETTLE);
                self.retries <= MAX_RETRIES
            }
        }
    }
}

fn capture_one(
    deps: &CaptureDeps,
    repo_id: &str,
    worktree: &Path,
    event: Option<i64>,
    give_way: bool,
) -> Attempt {
    if !worktree.is_dir() {
        return Attempt::Done;
    }
    match observe(
        deps,
        repo_id,
        &[worktree.to_path_buf()],
        None,
        event,
        CAPTURE_SETTLE_LIMIT,
        give_way,
    ) {
        Ok(_) => Attempt::Done,
        Err(Failure::NotCalm | Failure::Busy) => Attempt::Again,
        Err(Failure::Capture(CaptureError::Yielded)) => Attempt::GaveWay,
        Err(Failure::Capture(CaptureError::Discarded)) => Attempt::Again,
        Err(Failure::Unavailable) => Attempt::Done,
        Err(Failure::NoSpace) => {
            deps.logger.warn(
                "tm_capture_failed",
                &[("repo", Field::id(repo_id)), ("kind", "no-space".into())],
            );
            Attempt::Done
        }
        Err(Failure::Capture(e)) => {
            deps.logger.warn(
                "tm_capture_failed",
                &[
                    ("repo", Field::id(repo_id)),
                    ("kind", failure_kind(&e).into()),
                ],
            );
            Attempt::Done
        }
    }
}

/// Why an observation capture did not produce a point.
#[derive(Debug)]
pub enum Failure {
    /// The repo is not observed by the Time Machine, or its store cannot be opened.
    Unavailable,
    /// The engine did not become calm in time.
    NotCalm,
    /// A `git` holds the index of one of the worktrees.
    Busy,
    /// Less free space than the floor of SEC-TMC-12: the capture is skipped.
    NoSpace,
    Capture(CaptureError),
}

/// Takes one `observation` snapshot of `worktrees` (US-TMC-004): after a calm engine, with its
/// mark, guarded at the validity point. With `give_way`, new activity in the repo makes it give
/// way (`Yielded`), so it never competes with the engine during a burst.
pub fn observe(
    deps: &CaptureDeps,
    repo_id: &str,
    worktrees: &[PathBuf],
    cause_operation: Option<&str>,
    cause_event_seq: Option<i64>,
    settle: Duration,
    give_way: bool,
) -> Result<CaptureOutcome, Failure> {
    let (oplog, store) = deps.repos.repo(repo_id).ok_or(Failure::Unavailable)?;
    let store = store.ok_or(Failure::Unavailable)?;
    let common = deps.repos.common_dir(repo_id).ok_or(Failure::Unavailable)?;
    if worktrees.is_empty() {
        return Err(Failure::Unavailable);
    }
    // Nothing behind a `.git` the repo does not own is read, not even its state (#223 I-03).
    let registered = registered_worktrees(&common).map_err(|e| Failure::Capture(e.into()))?;
    for w in worktrees {
        if let Err(e @ gitraptor_git::ReadError::Untrusted(_)) =
            crate::observe::open_registered_in(&common, &registered, w)
        {
            return Err(Failure::Capture(e.into()));
        }
    }
    let mark = deps
        .engine
        .settle(repo_id, worktrees, settle)
        .ok_or(Failure::NotCalm)?;
    let at_start: Vec<Option<GitState>> = worktrees.iter().map(|w| GitState::read(w)).collect();
    if at_start.iter().flatten().any(GitState::locked) {
        return Err(Failure::Busy);
    }
    if let Some(floor) = deps.free_space_floor
        && below_floor(&deps.profile, floor)
    {
        return Err(Failure::NoSpace);
    }
    let scopes = worktrees
        .iter()
        .enumerate()
        .map(|(i, path)| WorktreeScope {
            key: worktree_key(&registered, path, i),
            path: path.clone(),
            hint: None,
        })
        .collect();
    let guarded: Vec<PathBuf> = worktrees.to_vec();
    let activity = deps.engine.activity(repo_id);
    let engine = Arc::clone(&deps.engine);
    let repo = repo_id.to_owned();
    let req = CaptureRequest {
        level: SnapshotLevel::Observation,
        common_dir: common,
        worktrees: scopes,
        engine_mark: Some(mark),
        cause_operation: cause_operation.map(str::to_owned),
        cause_event_seq,
        include_credentials: crate::profile::settings::include_credential_files(&deps.profile),
        still_valid: Some(ValidityGuard(Arc::new(move || {
            guarded
                .iter()
                .map(|w| GitState::read(w))
                .eq(at_start.iter().cloned())
        }))),
        give_way: give_way
            .then(|| ValidityGuard(Arc::new(move || engine.activity(&repo) != activity))),
    };
    if cfg!(debug_assertions)
        && let Some(err) = deps.layer.as_ref().and_then(|layer| layer(&req))
    {
        return Err(Failure::Capture(err));
    }
    store.capture(&oplog, &req).map_err(Failure::Capture)
}

/// The state an operation of GitRaptor left in its worktrees (US-TMC-004): the engine events up to
/// the anchor's mark are the operation's own echo, not raw Git (ADR-TMC-003 § 4). Tried a few
/// times if a `git` interferes; `None` if no anchor could be taken.
pub fn anchor(
    deps: &CaptureDeps,
    repo_id: &str,
    worktrees: &[PathBuf],
    operation_id: &str,
) -> Option<String> {
    for _ in 0..3 {
        match observe(
            deps,
            repo_id,
            worktrees,
            Some(operation_id),
            None,
            ANCHOR_SETTLE_LIMIT,
            false,
        ) {
            Ok(out) => return Some(out.snapshot_id),
            Err(
                Failure::Capture(CaptureError::Yielded | CaptureError::Discarded) | Failure::Busy,
            ) => {
                continue;
            }
            Err(e) => {
                deps.logger.warn(
                    "tm_anchor_failed",
                    &[
                        ("repo", Field::id(repo_id)),
                        (
                            "kind",
                            match &e {
                                Failure::Capture(c) => failure_kind(c),
                                Failure::NotCalm => "not-calm",
                                Failure::NoSpace => "no-space",
                                _ => "unavailable",
                            }
                            .into(),
                        ),
                    ],
                );
                return None;
            }
        }
    }
    deps.logger.warn(
        "tm_anchor_failed",
        &[("repo", Field::id(repo_id)), ("kind", "busy".into())],
    );
    None
}

/// Local branches of the repo whose common Git directory is `common_dir` and the commit each one
/// points to, by short name; empty if the repo cannot be read. Read from that directory, never
/// through a worktree's `.git` (#223 I-03).
pub fn branch_tips(common_dir: &Path) -> BTreeMap<String, String> {
    RepoReader::open(common_dir, &ReaderOptions::default())
        .and_then(|r| r.local_branches())
        .map(|list| list.into_iter().map(|b| (b.name, b.commit)).collect())
        .unwrap_or_default()
}

/// Waits, up to [`ANCHOR_SETTLE_LIMIT`], until the engine persisted the move of every branch an
/// operation moved under a worktree in `worktrees`, so the [`anchor`] taken next covers it.
///
/// The applier moves branches from the common Git folder: a branch checked out in a linked
/// worktree then moves without an entry in that worktree's `HEAD` reflog, which is what the
/// engine's calm compares, so `settle` does not wait for it. An anchor taken before the engine
/// persists that move would leave it outside the operation's echo, as raw Git on top of the
/// worktree's stack. A branch is waited for when it is checked out in one of `worktrees` and
/// points elsewhere than in `before`; the wait ends when that worktree has an event past
/// `since` naming the branch. Past the limit it returns anyway: the anchor is then taken as
/// before.
pub fn await_moved_branches(
    deps: &CaptureDeps,
    repo_id: &str,
    worktrees: &[PathBuf],
    before: &BTreeMap<String, String>,
    since: i64,
) {
    let Some(common) = deps.repos.common_dir(repo_id) else {
        return;
    };
    let after = branch_tips(&common);
    let mut pending: Vec<(&PathBuf, String)> = worktrees
        .iter()
        .filter_map(|root| {
            let branch = super::undo::head_branch(&common, root)?;
            (before.get(&branch) != after.get(&branch)).then_some((root, branch))
        })
        .collect();
    let end = Instant::now() + ANCHOR_SETTLE_LIMIT;
    while !pending.is_empty() {
        pending.retain(|(root, branch)| {
            !deps.engine.raw_events(repo_id, root).is_some_and(|events| {
                events
                    .iter()
                    .any(|e| e.seq > since && e.branch.as_deref() == Some(branch.as_str()))
            })
        });
        if pending.is_empty() || Instant::now() + ECHO_POLL > end {
            return;
        }
        std::thread::sleep(ECHO_POLL);
    }
}

/// How often [`await_moved_branches`] reads the engine again while waiting.
const ECHO_POLL: Duration = Duration::from_millis(10);

fn failure_kind(e: &CaptureError) -> &'static str {
    match e {
        CaptureError::InvalidInput(_) => "invalid-input",
        CaptureError::Yielded => "yielded",
        CaptureError::Discarded => "discarded",
        CaptureError::Read(gitraptor_git::ReadError::Untrusted(_)) => "worktree-untrusted",
        CaptureError::Read(_) => "read",
        CaptureError::Store(_) => "store",
        CaptureError::Oplog(_) => "oplog",
        CaptureError::Io(e) if super::protected::is_no_space(e) => "no-space",
        CaptureError::Io(_) => "io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CaptureConfig {
        CaptureConfig {
            enabled: true,
            quiet: Duration::from_millis(100),
            max_interval: Duration::from_millis(500),
        }
    }

    fn pending(first: Instant) -> Pending {
        Pending {
            first,
            last: first,
            event: None,
            retry_at: None,
            retries: 0,
        }
    }

    /// Architect's A1: giving way to sustained activity never makes the capture give up, and
    /// after six times `M` it no longer gives way, so the activity still gets a capture.
    #[test]
    fn sustained_activity_still_gets_a_capture() {
        let config = config();
        let now = Instant::now();
        let mut p = pending(now);
        assert!(p.give_way(&config));
        for _ in 0..(MAX_RETRIES * 4) {
            assert!(p.after(Attempt::GaveWay, now, &config));
        }
        assert_eq!(p.retries, 0);
        assert_eq!(p.retry_at, Some(now + config.quiet));
        let old = pending(now - config.max_interval * 6);
        assert!(!old.give_way(&config));
    }

    #[test]
    fn a_capture_that_never_becomes_consistent_gives_up() {
        let config = config();
        let now = Instant::now();
        let mut p = pending(now);
        let mut kept = 0;
        while p.after(Attempt::Again, now, &config) {
            kept += 1;
        }
        assert_eq!(kept, MAX_RETRIES);
        assert!(!pending(now).after(Attempt::Done, now, &config));
    }

    #[test]
    fn a_git_event_is_due_at_once_and_activity_after_the_quiet_time() {
        let config = config();
        let now = Instant::now();
        let mut p = pending(now);
        assert_eq!(p.due(&config), now + config.quiet);
        p.first = now - config.max_interval;
        assert_eq!(p.due(&config), now);
        let mut p = pending(now);
        p.event = Some(3);
        assert_eq!(p.due(&config), now);
    }
}
