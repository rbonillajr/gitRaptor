//! Manual snapshots: the point a Time Machine takes when an agent asks for one, with its own
//! quota (ADR-TMC-004, Enmienda MCP).
//!
//! A manual snapshot is not a protected operation: it takes no write lock of the repo, never
//! enters the undo stack and writes nothing in the user's repository (NFR-01). The quota is
//! counted in the oplog (so it survives a restart and any number of connections), under the same
//! recording lock that records the attempt, and every attempt that reached capture counts,
//! discarded ones included. Nothing is ever deleted to make room: a full quota or a full disk is
//! a refusal.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_api::catalog::check_snapshot_label;
use gitraptor_api::methods::QuotaWindow;
use gitraptor_git::ReadError;

use super::continuous::{CaptureDeps, FreeSpaceFloor};
use super::engine::GitState;
use super::oplog::{Channel, ManualMeta, Oplog, Requester, SnapshotLevel};
use super::protected::{registered_worktrees, worktree_key};
use super::store::{
    CaptureError, CaptureRequest, InFlight, SnapshotStore, ValidityGuard, WorktreeScope,
};
use crate::profile::settings::include_credential_files;

pub const PER_MINUTE: usize = 5;
pub const MINUTE_MS: i64 = 60_000;
pub const PER_DAY: usize = 20;
pub const DAY_MS: i64 = 86_400_000;
/// Every requester together, per worktree, in 24 h.
pub const PER_WORKTREE_DAY: usize = 60;
/// Every requester and worktree together, per repo, in 24 h.
pub const PER_REPO_DAY: usize = 200;
/// Longest label, in bytes, read before its characters are walked.
pub const MAX_LABEL_BYTES: usize = 256;
/// What the daemon spends on one manual snapshot, the wait for the engine included.
pub const MANUAL_BUDGET: Duration = Duration::from_secs(25);
/// What is left of the volume for the guaranteed prior, at least (the larger of this and the
/// worktree's estimated size).
const MIN_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;
/// Entries the estimate of a worktree's size looks at before it stops.
const ESTIMATE_MAX_ENTRIES: usize = 200_000;
/// How long the estimate of a worktree's size is reused.
const RESERVE_TTL: Duration = Duration::from_secs(300);

thread_local! {
    /// The `(dev, inode)` the capture running on this thread must find at the worktree's root.
    static EXPECTED_ROOT: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
}

/// Runs `f` (a manual capture) holding it to the worktree root the caller verified: under the
/// recording lock the root is read again and a capture whose root is another folder is
/// discarded. Scoped to the calling thread and restored afterwards, so the capture entry points
/// keep their signatures.
pub fn expecting_root<R>(id: Option<(u64, u64)>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<(u64, u64)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            EXPECTED_ROOT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(EXPECTED_ROOT.with(|c| c.replace(id)));
    f()
}

/// What a manual snapshot is asked for.
#[derive(Debug, Clone)]
pub struct ManualAsk {
    pub repo_id: String,
    pub worktree: PathBuf,
    /// The repo's common Git directory, from the registry: the worktree is read only if the repo
    /// registers it and owns its `.git` (#223 I-03).
    pub common_dir: PathBuf,
    /// Untrusted text: it is data, never a ref, a path, an argument or a log field.
    pub label: String,
    pub requester: Requester,
    pub channel: Channel,
}

/// A manual snapshot that was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualCaptured {
    pub snapshot_id: String,
    /// Canonical root of the worktree it covers.
    pub worktree: PathBuf,
}

/// The window that refused a request and when a slot frees up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaHit {
    pub window: QuotaWindow,
    /// Instant the oldest attempt of the window leaves it, in ms since the epoch.
    pub release_at_ms: i64,
}

/// What the quota counts, read once.
///
/// Every manual row that reached capture, `discarded` included; the worktree by the equality of
/// its key, never by text inside the `worktrees` column.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuotaInput {
    /// The requester's attempts in the worktree.
    pub requester_ms: Vec<i64>,
    /// Everybody's attempts in the worktree.
    pub worktree_ms: Vec<i64>,
    /// Everybody's attempts in the repo.
    pub repo_ms: Vec<i64>,
}

/// Why a manual snapshot was not taken. None of them leaves a point, and none deletes one.
#[derive(Debug)]
pub enum ManualError {
    Quota(QuotaHit),
    /// Less free space than the manual floor.
    NoSpace,
    /// A Git operation (a rebase, a merge...) is in progress in the worktree.
    InProgress,
    /// The same requester has a manual snapshot in flight.
    InFlight,
    /// A `git` holds the index of the worktree.
    Busy,
    /// The worktree changed under the capture, or its time ran out: the attempt counts, no point.
    Discarded,
    /// The recording lock or the engine did not become free within the budget.
    TimeLimit,
    /// The repo has no Time Machine to take the point in.
    Unavailable,
    Capture(CaptureError),
}

/// Two errors are equal when they are the same refusal; a failure of the capture itself is
/// compared by what it says (its sources are not comparable).
impl PartialEq for ManualError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Quota(a), Self::Quota(b)) => a == b,
            (Self::Capture(a), Self::Capture(b)) => a.to_string() == b.to_string(),
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}

impl Eq for ManualError {}

/// A window's stamps in `(now - window, ∞)` (no upper bound: a clock that goes back never empties
/// a window), and, if it is full, when its oldest leaves.
fn full(stamps: &[i64], now_ms: i64, window_ms: i64, limit: usize) -> Option<i64> {
    let floor = now_ms.saturating_sub(window_ms);
    let (count, oldest) = stamps
        .iter()
        .copied()
        .filter(|s| *s > floor)
        .fold((0usize, i64::MAX), |(n, min), s| (n + 1, min.min(s)));
    (count >= limit).then(|| oldest.saturating_add(window_ms))
}

/// Pure. The requester's minute, the requester's day, the worktree's day, then the repo's day: the
/// first one that is full refuses. A stamp counts when `stamp > now_ms - window`; the release
/// time is the oldest stamp in the window plus the window.
pub fn quota(input: &QuotaInput, now_ms: i64) -> Result<(), QuotaHit> {
    let windows = [
        (
            QuotaWindow::Minute,
            &input.requester_ms,
            MINUTE_MS,
            PER_MINUTE,
        ),
        (QuotaWindow::Day, &input.requester_ms, DAY_MS, PER_DAY),
        (
            QuotaWindow::WorktreeDay,
            &input.worktree_ms,
            DAY_MS,
            PER_WORKTREE_DAY,
        ),
        (QuotaWindow::RepoDay, &input.repo_ms, DAY_MS, PER_REPO_DAY),
    ];
    for (window, stamps, window_ms, limit) in windows {
        if let Some(release_at_ms) = full(stamps, now_ms, window_ms, limit) {
            return Err(QuotaHit {
                window,
                release_at_ms,
            });
        }
    }
    Ok(())
}

/// The key the quota and the timeline know a worktree by: its resolved root and the `(dev,
/// inode)` of that root, as the daemon reads them. Two worktrees never share it, whatever their
/// names look like; a replaced directory under the same path gets another.
pub fn worktree_identity_key(root: &Path, id: Option<(u64, u64)>) -> String {
    let (dev, ino) = id.unwrap_or((0, 0));
    format!("{dev}:{ino}:{}", root.to_string_lossy())
}

/// `(device, inode)` of the folder, without following a link; `None` where the OS gives none.
pub(crate) fn folder_id(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path)
            .ok()
            .map(|m| (m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        gitraptor_winsys::file_id::of_path(path)
            .ok()
            .map(|(volume, index)| (u64::from(volume), index))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

/// The canonical root of `worktree` and its identity key, as the daemon reads them. The quota
/// of a request and the precheck of `prepare` must derive the key the same way: this is the
/// only place that does.
pub fn resolve_worktree(worktree: &Path) -> io::Result<(PathBuf, String)> {
    let root = gitraptor_git::paths::canonicalize(worktree)?;
    let key = worktree_identity_key(&root, folder_id(&root));
    Ok((root, key))
}

/// The root a [`worktree_identity_key`] was made from.
pub fn identity_key_root(key: &str) -> Option<&str> {
    let mut parts = key.splitn(3, ':');
    let (dev, ino, root) = (parts.next()?, parts.next()?, parts.next()?);
    (dev.bytes().all(|b| b.is_ascii_digit()) && ino.bytes().all(|b| b.is_ascii_digit()))
        .then_some(root)
}

/// Indexed read of the oplog, without the store's recording lock (used by `prepare`): the quota
/// as the oplog stands. It answers early so a looping agent gets its real wait; the capture
/// counts again under the lock, which is the authoritative count.
pub fn precheck(
    oplog: &Mutex<Oplog>,
    store: &SnapshotStore,
    session_id: &str,
    worktree_key: &str,
    now_ms: i64,
) -> Result<(), QuotaHit> {
    let _ = store;
    let input = {
        let log = oplog.lock().unwrap_or_else(|p| p.into_inner());
        // An oplog that cannot be read refuses nothing here: the capture, which must read it
        // under the lock, fails closed.
        match log.manual_quota_input(session_id, worktree_key, now_ms) {
            Ok(input) => input,
            Err(_) => return Ok(()),
        }
    };
    quota(&input, now_ms)
}

/// Free space seen by the manual capture: injectable so tests cross the floor deterministically.
/// Production reads the volume of the store.
pub trait FreeSpaceProbe: Send + Sync {
    fn available_bytes(&self, path: &Path) -> io::Result<u64>;

    /// Size of the volume, for the percentage of the floor. A probe that does not know it leaves
    /// that part of the floor out.
    fn total_bytes(&self, _path: &Path) -> io::Result<u64> {
        Ok(0)
    }
}

/// The manual floor: the floor of the continuous capture plus a reserve for the guaranteed prior.
/// Checked under the recording lock right before capturing and again while the capture reads
/// files: crossing it discards the attempt (it counts) and deletes nothing.
pub struct ManualFloor<'a> {
    pub floor: FreeSpaceFloor,
    pub reserve_bytes: u64,
    pub probe: &'a dyn FreeSpaceProbe,
}

impl ManualFloor<'_> {
    /// Whether the volume of `path` has less than the floor and the reserve. A volume that cannot
    /// be read is below it: a manual snapshot is never taken blind.
    pub(crate) fn crossed(&self, path: &Path) -> bool {
        let Ok(available) = self.probe.available_bytes(path) else {
            return true;
        };
        let total = self.probe.total_bytes(path).unwrap_or(0);
        let percent = total / 100 * u64::from(self.floor.percent);
        available
            < self
                .floor
                .bytes
                .max(percent)
                .saturating_add(self.reserve_bytes)
    }
}

/// The volume of the store, read from the OS. Outside Unix it reports plenty, as the floor of the
/// continuous capture does there: the store's own write fails as no space if the disk is full.
pub(crate) struct VolumeProbe;

impl FreeSpaceProbe for VolumeProbe {
    #[cfg(unix)]
    fn available_bytes(&self, path: &Path) -> io::Result<u64> {
        let st = rustix::fs::statvfs(path)?;
        Ok(st.f_bavail.saturating_mul(st.f_frsize.max(1)))
    }

    #[cfg(unix)]
    fn total_bytes(&self, path: &Path) -> io::Result<u64> {
        let st = rustix::fs::statvfs(path)?;
        Ok(st.f_blocks.saturating_mul(st.f_frsize.max(1)))
    }

    #[cfg(not(unix))]
    fn available_bytes(&self, _path: &Path) -> io::Result<u64> {
        Ok(u64::MAX)
    }
}

/// Whether a Git operation is in progress in `worktree` of the repo at `common_dir`. A worktree
/// that cannot be read, or that the repo does not own, counts as in progress: the capture is
/// refused rather than taken blind.
fn operation_in_progress(common_dir: &Path, worktree: &Path) -> bool {
    crate::observe::open_registered_worktree(common_dir, worktree)
        .map_or(true, |reader| reader.in_progress().is_some())
}

/// Takes one manual snapshot in `store`.
///
/// 1. A session with a capture in flight is refused at once ([`ManualError::InFlight`]).
/// 2. The recording lock is waited for until `deadline` ([`ManualError::TimeLimit`]).
/// 3. Under it, in one read: the quota (requester's windows, worktree and repo ceilings), the
///    free-space floor and the Git operation in progress. Then the attempt is recorded, before
///    the worktree is read.
/// 4. The capture is discarded, and nothing deleted, if the Git state changed, an operation
///    started, `deadline` passed or the volume went below the floor.
///
/// `engine_mark` is the engine's mark the read state reaches; `now_ms` the request's instant
/// (every row of the attempt carries it). Nothing is written in the repo.
#[allow(clippy::too_many_arguments)]
pub fn capture_in_store(
    store: &SnapshotStore,
    oplog: &Mutex<Oplog>,
    ask: &ManualAsk,
    engine_mark: Option<i64>,
    include_credentials: bool,
    floor: Option<&ManualFloor<'_>>,
    now_ms: i64,
    deadline: Instant,
) -> Result<ManualCaptured, ManualError> {
    // Defense in depth: the daemon validated the label when the plan was prepared.
    if check_snapshot_label(&ask.label).is_err() {
        return Err(invalid("label"));
    }
    let Some(session) = ask.requester.session_id() else {
        return Err(invalid("requester"));
    };
    let Some(in_flight) = store.manual().enter(session) else {
        return Err(ManualError::InFlight);
    };
    capture_guarded(
        store,
        oplog,
        ask,
        engine_mark,
        include_credentials,
        floor,
        now_ms,
        deadline,
        &in_flight,
    )
}

/// [`capture_in_store`] once the session's in-flight guard is held (the proof is the borrow).
#[allow(clippy::too_many_arguments)]
fn capture_guarded(
    store: &SnapshotStore,
    oplog: &Mutex<Oplog>,
    ask: &ManualAsk,
    engine_mark: Option<i64>,
    include_credentials: bool,
    floor: Option<&ManualFloor<'_>>,
    now_ms: i64,
    deadline: Instant,
    _in_flight: &InFlight<'_>,
) -> Result<ManualCaptured, ManualError> {
    let Some(session) = ask.requester.session_id() else {
        return Err(invalid("requester"));
    };
    let (root, key) =
        resolve_worktree(&ask.worktree).map_err(|e| ManualError::Capture(e.into()))?;
    // A worktree whose `.git` the repo does not own is refused before anything is recorded or
    // read behind it (#223 I-03, NFR-01).
    if let Err(e @ ReadError::Untrusted(_)) =
        crate::observe::open_registered_worktree(&ask.common_dir, &root)
    {
        return Err(ManualError::Capture(e.into()));
    }
    let registered = registered_worktrees(&ask.common_dir).unwrap_or_default();
    let store_key = worktree_key(&registered, &root, 0);

    let Some(_recording) = store.manual().lock_recording(deadline) else {
        return Err(ManualError::TimeLimit);
    };
    // The root the caller verified must still be the folder at this path: a swap between the
    // verification and here discards the attempt before anything is recorded.
    if let Some(expected) = EXPECTED_ROOT.with(Cell::get)
        && folder_id(&root) != Some(expected)
    {
        return Err(ManualError::Discarded);
    }
    {
        let log = oplog.lock().unwrap_or_else(|p| p.into_inner());
        let input = log
            .manual_quota_input(session, &key, now_ms)
            .map_err(|e| ManualError::Capture(e.into()))?;
        quota(&input, now_ms).map_err(ManualError::Quota)?;
    }
    if floor.is_some_and(|f| f.crossed(store.path())) {
        return Err(ManualError::NoSpace);
    }
    if operation_in_progress(&ask.common_dir, &root) {
        return Err(ManualError::InProgress);
    }

    let at_start = GitState::read(&root);
    let guarded = root.clone();
    let common = ask.common_dir.clone();
    let req = CaptureRequest {
        level: SnapshotLevel::Manual,
        common_dir: ask.common_dir.clone(),
        worktrees: vec![WorktreeScope {
            key: store_key,
            path: root.clone(),
            hint: None,
        }],
        engine_mark,
        cause_operation: None,
        cause_event_seq: None,
        include_credentials,
        still_valid: Some(ValidityGuard(std::sync::Arc::new(move || {
            Instant::now() < deadline
                && GitState::read(&guarded) == at_start
                && !operation_in_progress(&common, &guarded)
        }))),
        give_way: None,
    };
    let meta = ManualMeta {
        label: ask.label.clone(),
        requester: ask.requester.clone(),
        channel: ask.channel,
        worktree_key: key,
        requested_ms: now_ms,
    };
    // The floor again while the files are read: the same answer as before the capture, asked at
    // each point where a capture may stop.
    let crossed = AtomicBool::new(false);
    let abort = || {
        floor.is_some_and(|f| {
            let below = f.crossed(store.path());
            if below {
                crossed.store(true, Ordering::Release);
            }
            below
        })
    };
    match store.capture_manual_until(oplog, &req, &meta, &abort) {
        Ok(out) => Ok(ManualCaptured {
            snapshot_id: out.snapshot_id,
            worktree: root,
        }),
        // Given way to a prior, discarded by the guard or by the floor: the attempt counts and
        // there is no point.
        Err(CaptureError::Yielded | CaptureError::Discarded) => Err(ManualError::Discarded),
        Err(e) => Err(ManualError::Capture(e)),
    }
}

fn invalid(what: &str) -> ManualError {
    ManualError::Capture(CaptureError::InvalidInput(format!("invalid {what}")))
}

/// What the volume must keep for the guaranteed prior of the worktree: the larger of 1 GiB and
/// the worktree's estimated size (regular files outside `.git`).
///
/// The walk stops at [`ESTIMATE_MAX_ENTRIES`] entries or at `deadline`. A walk cut short only saw
/// part of the tree, so it answers conservatively: the larger of what it summed, the last
/// estimate of this worktree (`last`) and the minimum.
fn reserve_for(worktree: &Path, deadline: Instant, last: Option<u64>) -> u64 {
    let mut bytes = 0u64;
    let mut seen = 0usize;
    let mut complete = true;
    let mut stack = vec![worktree.to_path_buf()];
    'walk: while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > ESTIMATE_MAX_ENTRIES
                || (seen.is_multiple_of(256) && Instant::now() >= deadline)
            {
                complete = false;
                break 'walk;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(entry.path());
                }
            } else if kind.is_file() {
                bytes = bytes.saturating_add(entry.metadata().map_or(0, |m| m.len()));
            }
        }
    }
    if !complete {
        bytes = bytes.max(last.unwrap_or(0));
    }
    bytes.max(MIN_RESERVE_BYTES)
}

/// [`reserve_for`] through the store's cache: one walk per worktree per [`RESERVE_TTL`], however
/// many requests arrive.
pub(crate) fn cached_reserve(
    store: &SnapshotStore,
    key: &str,
    worktree: &Path,
    deadline: Instant,
) -> u64 {
    let last = store.manual().last_reserve(key);
    if let Some((age, bytes)) = last
        && age < RESERVE_TTL
    {
        return bytes;
    }
    let bytes = reserve_for(worktree, deadline, last.map(|(_, b)| b));
    store.manual().remember_reserve(key, bytes);
    bytes
}

/// The daemon's path: waits for a calm engine within the budget, refuses an index a `git` holds,
/// then [`capture_in_store`] with the profile's floor and credential setting. The credential
/// option comes from the profile, never from the request.
pub fn capture(
    deps: &CaptureDeps,
    ask: &ManualAsk,
    now_ms: i64,
) -> Result<ManualCaptured, ManualError> {
    let started = Instant::now();
    let deadline = started + MANUAL_BUDGET;
    let (oplog, store) = deps
        .repos
        .repo(&ask.repo_id)
        .ok_or(ManualError::Unavailable)?;
    let store = store.ok_or(ManualError::Unavailable)?;
    let session = ask
        .requester
        .session_id()
        .ok_or_else(|| invalid("requester"))?;
    // Before anything that costs I/O (the wait for the engine, the walk of the worktree): a
    // session with a capture in flight, or over its quota, is refused here, so concurrent
    // requests cannot multiply that work.
    let in_flight = store.manual().enter(session).ok_or(ManualError::InFlight)?;
    let (_, key) = resolve_worktree(&ask.worktree).map_err(|e| ManualError::Capture(e.into()))?;
    precheck(&oplog, &store, session, &key, now_ms).map_err(ManualError::Quota)?;
    let mark = deps
        .engine
        .settle(
            &ask.repo_id,
            std::slice::from_ref(&ask.worktree),
            MANUAL_BUDGET,
        )
        .ok_or(ManualError::TimeLimit)?;
    if GitState::read(&ask.worktree).is_some_and(|s| s.locked()) {
        return Err(ManualError::Busy);
    }
    let probe = VolumeProbe;
    let floor = deps.free_space_floor.map(|floor| ManualFloor {
        floor,
        reserve_bytes: cached_reserve(&store, &key, &ask.worktree, deadline),
        probe: &probe,
    });
    if check_snapshot_label(&ask.label).is_err() {
        return Err(invalid("label"));
    }
    capture_guarded(
        &store,
        &oplog,
        ask,
        Some(mark),
        include_credential_files(&deps.profile),
        floor.as_ref(),
        now_ms,
        deadline,
        &in_flight,
    )
}

/// Milliseconds since the Unix epoch, the instant a request is recorded with.
pub fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests;
