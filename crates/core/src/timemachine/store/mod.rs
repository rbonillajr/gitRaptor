//! The snapshot store of a repo and the capture that feeds it (TS-TMC-001, ADR-TMC-001).
//!
//! Each observed repo has a private bare store at `<data>/tm/<repo_id>/store.git`, next to its
//! oplog. A snapshot is a commit of the store with `wt/<key>/files` (raw working tree content),
//! `wt/<key>/index` (what is staged) and `meta`, and a ref `refs/tm/snap/<id>`. Nothing is ever
//! written to the user's repository: it is only read through [`gitraptor_git::RepoReader`].
//!
//! A snapshot exists only when its ref is in the store **and** its oplog row is `complete`
//! (ADR-TMC-001 § 1, ADR-TMC-003). The store writes go through the gix writer of the Time Machine
//! write layer ([`gitraptor_git::tm_write::store`]).

mod capture;
mod meta;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use gitraptor_git::tm_write::store::{
    SeedLimits, SeedReport, StoreError, StoreHandle, StoreRepo, VerifyReport,
};
use gitraptor_git::{Oid, ReadError, ReaderOptions, RepoReader};

use super::oplog::{SnapshotRefs, TM_DIR, repo_dir};
use crate::profile::{ProfileDirs, ProfileError, fsperm};

pub use capture::{
    CaptureOutcome, CaptureRequest, ChangeHint, Detection, ValidityGuard, WorktreeScope,
};
pub use meta::{ConflictEntry, META_FORMAT, Meta, MetaWorktree, RegisteredWorktree};

/// Files larger than this are left out of an observation capture, which is then partial
/// (ADR-TMC-001 § 2, confirmed by SPIKE-TMC-001 on macOS). A guaranteed prior always includes
/// them (US-TMC-020, escenario 3).
pub const OBSERVATION_MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Why a capture did not produce a snapshot.
#[derive(Debug)]
pub enum CaptureError {
    InvalidInput(String),
    /// An observation capture gave way to a guaranteed prior (ADR-TMC-004 § 2). Nothing was
    /// recorded; the next capture covers the same changes.
    Yielded,
    /// The capture stopped being consistent before its validity point (a `git` ran while it
    /// read): nothing was recorded and it is taken again (ADR-TMC-004 § 2).
    Discarded,
    Read(ReadError),
    Store(StoreError),
    Oplog(ProfileError),
    Io(io::Error),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid capture request: {m}"),
            Self::Yielded => write!(f, "gave way to a guaranteed prior snapshot"),
            Self::Discarded => write!(f, "discarded: the worktree changed under Git while read"),
            Self::Read(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
            Self::Oplog(e) => write!(f, "oplog: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<ReadError> for CaptureError {
    fn from(e: ReadError) -> Self {
        Self::Read(e)
    }
}

impl From<StoreError> for CaptureError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Yielded => Self::Yielded,
            other => Self::Store(other),
        }
    }
}

impl From<ProfileError> for CaptureError {
    fn from(e: ProfileError) -> Self {
        Self::Oplog(e)
    }
}

impl From<io::Error> for CaptureError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// How the store was found when opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreStatus {
    Created,
    Existing,
    /// The old store could not be trusted: it was renamed (never deleted) and a new one was
    /// created. The timeline shows a gap with this reason (ADR-TMC-001 § 4).
    Replaced {
        set_aside: PathBuf,
        reason: String,
    },
}

/// Time spent per stage of ADR-TMC-006 § 2, with a monotonic clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageTimings {
    /// Waiting for the store writer (counts in the overhead, ADR-TMC-006 § 1).
    pub queue: Duration,
    pub detect: Duration,
    pub anchor: Duration,
    pub blobs: Duration,
    /// Trees and the store commit.
    pub trees: Duration,
    /// Barrier, ref and oplog row, with their durability.
    pub ref_oplog: Duration,
    pub total: Duration,
    pub files_read: u64,
    pub bytes_read: u64,
}

/// The snapshot store of one repo.
pub struct SnapshotStore {
    repo_id: String,
    store: StoreRepo,
    /// One capture at a time per store (the single writer of ADR-TMC-004 § 2).
    writer: Mutex<capture::State>,
    /// Guaranteed priors waiting for the writer: an observation capture gives way to them.
    priors_waiting: AtomicUsize,
    seed_limits: SeedLimits,
    /// Manual snapshots in flight and the lock under which their quota is counted and their
    /// attempt recorded. Apart from `writer`: a guaranteed prior never waits for it.
    manual: ManualState,
}

/// The state manual snapshots share in one store (one per repo): the sessions with a capture in
/// flight and the recording lock. It is lost on a restart: the quota windows are durable in the
/// oplog.
#[derive(Default)]
pub(crate) struct ManualState {
    inner: Mutex<ManualInner>,
    released: Condvar,
    /// Estimated reserve per worktree key, with when it was measured: the walk that makes it is
    /// not repeated for every request.
    reserves: Mutex<std::collections::HashMap<String, (Instant, u64)>>,
}

#[derive(Default)]
struct ManualInner {
    in_flight: HashSet<String>,
    recording: bool,
}

/// A session with a manual capture in flight. Dropped (also on panic), it is free again.
pub(crate) struct InFlight<'a> {
    state: &'a ManualState,
    session: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut inner = self.state.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.in_flight.remove(&self.session);
    }
}

/// The recording lock of manual snapshots. Dropped (also on panic), the next one goes in.
pub(crate) struct Recording<'a>(&'a ManualState);

impl Drop for Recording<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.recording = false;
        self.0.released.notify_one();
    }
}

impl ManualState {
    /// The last reserve measured for `key` and how long ago.
    pub(crate) fn last_reserve(&self, key: &str) -> Option<(Duration, u64)> {
        let map = self.reserves.lock().unwrap_or_else(|p| p.into_inner());
        map.get(key).map(|(at, bytes)| (at.elapsed(), *bytes))
    }

    /// Remembers the reserve measured for `key` now. Bounded: a full cache is emptied first.
    pub(crate) fn remember_reserve(&self, key: &str, bytes: u64) {
        let mut map = self.reserves.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= 256 {
            map.clear();
        }
        map.insert(key.to_owned(), (Instant::now(), bytes));
    }

    /// Marks `session` as having a capture in flight; `None` if it already has one.
    pub(crate) fn enter(&self, session: &str) -> Option<InFlight<'_>> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .in_flight
            .insert(session.to_owned())
            .then(|| InFlight {
                state: self,
                session: session.to_owned(),
            })
    }

    /// Takes the recording lock, waiting for it at most until `deadline`. It is tried before the
    /// clock is looked at, so a free lock is never refused for a deadline already past.
    pub(crate) fn lock_recording(&self, deadline: Instant) -> Option<Recording<'_>> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if !inner.recording {
                inner.recording = true;
                return Some(Recording(self));
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            if left.is_zero() {
                return None;
            }
            inner = self
                .released
                .wait_timeout(inner, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

impl std::fmt::Debug for SnapshotStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotStore")
            .field("repo_id", &self.repo_id)
            .field("path", &self.store.path())
            .finish()
    }
}

/// The `tm/` folder, private, excluded from OS backups (SEC-TMC-06).
fn tm_root(dirs: &ProfileDirs) -> Result<PathBuf, ProfileError> {
    let root = dirs.data.join(TM_DIR);
    fsperm::ensure_private_dir(&root)?;
    exclude_from_backups(&root)?;
    Ok(root)
}

/// Marks `tm/` as excluded from backups: the attribute Apple's Time Machine reads
/// (`tmutil addexclusion` writes the same) and a `CACHEDIR.TAG` for Linux backup tools.
fn exclude_from_backups(root: &Path) -> Result<(), ProfileError> {
    let tag = root.join("CACHEDIR.TAG");
    if fs_missing(&tag) {
        use std::io::Write;
        let mut f = fsperm::create_private_file(&tag)?;
        f.write_all(
            b"Signature: 8a477f597d28d172789f06886806bc55\n\
              # GitRaptor Time Machine snapshots: excluded from backups (SEC-TMC-06).\n",
        )?;
    }
    #[cfg(target_vendor = "apple")]
    {
        // Binary plist of the string "com.apple.backupd".
        const VALUE: [u8; 61] = [
            0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0x5f, 0x10, 0x11, 0x63, 0x6f, 0x6d,
            0x2e, 0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x62, 0x61, 0x63, 0x6b, 0x75, 0x70, 0x64,
            0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x1c,
        ];
        rustix::fs::setxattr(
            root,
            "com.apple.metadata:com_apple_backup_excludeItem",
            &VALUE,
            rustix::fs::XattrFlags::empty(),
        )
        .map_err(io::Error::from)?;
    }
    Ok(())
}

fn fs_missing(path: &Path) -> bool {
    matches!(path.symlink_metadata(), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

fn now_ms() -> i64 {
    crate::daemon::now_ms()
}

impl SnapshotStore {
    /// Opens the store of `repo_id`, creating it if there is none. A store that cannot be
    /// trusted is set aside (renamed) and replaced by a new one.
    pub fn open_or_create(
        dirs: &ProfileDirs,
        repo_id: &str,
    ) -> Result<(Self, StoreStatus), CaptureError> {
        let root = tm_root(dirs)?;
        fsperm::ensure_private_dir(&repo_dir(dirs, repo_id)?)?;
        let location = StoreRepo::location(&root, repo_id)?;
        let (store, status) = if fs_missing(&location) {
            (StoreRepo::create(&root, repo_id)?, StoreStatus::Created)
        } else {
            match StoreRepo::open(&root, repo_id) {
                Ok(store) => (store, StoreStatus::Existing),
                Err(StoreError::Untrusted(reason)) => {
                    let mut aside = location.clone().into_os_string();
                    aside.push(format!(".aside-{}", now_ms()));
                    let set_aside = PathBuf::from(aside);
                    std::fs::rename(&location, &set_aside)?;
                    let store = StoreRepo::create(&root, repo_id)?;
                    (store, StoreStatus::Replaced { set_aside, reason })
                }
                Err(e) => return Err(e.into()),
            }
        };
        Ok((Self::from_repo(repo_id, store), status))
    }

    /// Where the store of `repo_id` lives, without opening it: the key of the repo's write lock,
    /// shared by the applier and the executor (ADR-CKP-002 § 5). Equals [`Self::path`] of the
    /// opened store.
    pub fn location(dirs: &ProfileDirs, repo_id: &str) -> Result<PathBuf, CaptureError> {
        let location = StoreRepo::location(&dirs.data.join(TM_DIR), repo_id)?;
        // The opened store reports its canonical path: canonicalize the deepest part that
        // exists, so the key is the same before and after the store is created.
        let mut existing = location.as_path();
        let mut rest = Vec::new();
        while std::fs::symlink_metadata(existing).is_err() {
            let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                return Ok(location);
            };
            rest.push(name.to_owned());
            existing = parent;
        }
        // The same form `StoreRepo::open` reports (drive form on Windows).
        let mut out = gitraptor_git::paths::canonicalize(existing)?;
        out.extend(rest.iter().rev());
        Ok(out)
    }

    /// Opens the store of `repo_id` if one exists and can be trusted; `None` otherwise. Used by
    /// the recovery at startup, which never creates or sets aside anything.
    pub fn open_existing(dirs: &ProfileDirs, repo_id: &str) -> Result<Option<Self>, CaptureError> {
        let root = dirs.data.join(TM_DIR);
        let location = StoreRepo::location(&root, repo_id)?;
        if fs_missing(&location) {
            return Ok(None);
        }
        match StoreRepo::open(&root, repo_id) {
            Ok(store) => Ok(Some(Self::from_repo(repo_id, store))),
            Err(StoreError::Untrusted(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn from_repo(repo_id: &str, store: StoreRepo) -> Self {
        Self {
            repo_id: repo_id.to_owned(),
            store,
            writer: Mutex::new(capture::State::default()),
            priors_waiting: AtomicUsize::new(0),
            seed_limits: SeedLimits::default(),
            manual: ManualState::default(),
        }
    }

    /// The state manual snapshots share in this store.
    pub(crate) fn manual(&self) -> &ManualState {
        &self.manual
    }

    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    pub fn path(&self) -> &Path {
        self.store.path()
    }

    /// Bytes the store takes on disk (diagnostics, ADR-TMC-001 § 4).
    pub fn size_bytes(&self) -> io::Result<u64> {
        self.store.size_bytes()
    }

    /// Seeds the store from the packs of the repo at `repo` (ADR-TMC-001 § 3). Runs in the
    /// background; until it ends, a capture copies what it lacks.
    pub fn seed(&self, repo: &Path) -> Result<SeedReport, CaptureError> {
        let reader = RepoReader::open(repo, &ReaderOptions::default())?;
        Ok(self.store.seed_from(&reader, self.seed_limits)?)
    }

    /// Like [`SnapshotStore::seed`], with the indexing limited to `limits` (the continuous
    /// capture seeds with one thread, US-TMC-004).
    pub fn seed_with(
        &self,
        repo: &Path,
        limits: gitraptor_git::tm_write::store::SeedLimits,
    ) -> Result<SeedReport, CaptureError> {
        let reader = RepoReader::open(repo, &ReaderOptions::default())?;
        Ok(self.store.seed_from(&reader, limits)?)
    }

    /// Whether the store has the commit `HEAD` of `worktree` points to: once it does, a capture
    /// of it copies only what is new (ADR-TMC-001 § 3). An unborn or unreadable `HEAD` counts as
    /// had: there is nothing to copy.
    pub fn has_head_of(&self, worktree: &Path) -> bool {
        let Ok(reader) = RepoReader::open(worktree, &ReaderOptions::default()) else {
            return true;
        };
        match reader.head().ok().and_then(|h| h.commit) {
            Some(hex) => Oid::from_hex(&hex).is_none_or(|id| self.store.handle().has(id)),
            None => true,
        }
    }

    /// Copies into the store the commits `tips` reach and it lacks (anchoring, ADR-TMC-001 § 3).
    /// The daemon calls it when the engine publishes a new commit or a ref move.
    pub fn anchor(&self, repo: &Path, tips: &[String]) -> Result<u64, CaptureError> {
        let reader = RepoReader::open(repo, &ReaderOptions::default())?;
        let tips: Vec<Oid> = tips
            .iter()
            .map(|t| Oid::from_hex(t).ok_or_else(|| CaptureError::InvalidInput("not an id".into())))
            .collect::<Result<_, _>>()?;
        Ok(self.store.handle().copy_closure(&reader, &tips)?.objects)
    }

    /// Re-hashes everything a snapshot needs before it is handed out for restoring and parses its
    /// `meta` (SEC-TMC-09).
    pub fn verify(&self, snapshot_id: &str) -> Result<VerifyReport, CaptureError> {
        let handle = self.store.handle();
        let commit = handle
            .snapshot_commit(snapshot_id)?
            .ok_or_else(|| CaptureError::InvalidInput("no such snapshot".into()))?;
        let report = handle.verify_commit(commit)?;
        self.read_meta_with(&handle, commit)?;
        Ok(report)
    }

    /// The `meta` of a snapshot.
    pub fn meta(&self, snapshot_id: &str) -> Result<Meta, CaptureError> {
        let handle = self.store.handle();
        let commit = handle
            .snapshot_commit(snapshot_id)?
            .ok_or_else(|| CaptureError::InvalidInput("no such snapshot".into()))?;
        self.read_meta_with(&handle, commit)
    }

    fn read_meta_with(&self, handle: &StoreHandle, commit: Oid) -> Result<Meta, CaptureError> {
        let tree = handle.commit_tree(commit)?;
        let (_, blob) = handle
            .tree_entry(tree, "meta".into())?
            .ok_or_else(|| StoreError::Corrupt("snapshot without meta".into()))?;
        Meta::parse(&handle.read_blob(blob)?)
            .map_err(|e| StoreError::Corrupt(format!("meta: {e}")).into())
    }

    /// Files of worktree `key` in a snapshot: path, kind and blob, sorted by path.
    pub fn files(
        &self,
        snapshot_id: &str,
        key: &str,
    ) -> Result<Vec<(String, gitraptor_git::tm_write::store::TreeEntryKind, Oid)>, CaptureError>
    {
        self.subtree(snapshot_id, &format!("wt/{key}/files"))
    }

    /// Staged entries of worktree `key` in a snapshot.
    pub fn staged(
        &self,
        snapshot_id: &str,
        key: &str,
    ) -> Result<Vec<(String, gitraptor_git::tm_write::store::TreeEntryKind, Oid)>, CaptureError>
    {
        self.subtree(snapshot_id, &format!("wt/{key}/index"))
    }

    fn subtree(
        &self,
        snapshot_id: &str,
        path: &str,
    ) -> Result<Vec<(String, gitraptor_git::tm_write::store::TreeEntryKind, Oid)>, CaptureError>
    {
        let handle = self.store.handle();
        let commit = handle
            .snapshot_commit(snapshot_id)?
            .ok_or_else(|| CaptureError::InvalidInput("no such snapshot".into()))?;
        let root = handle.commit_tree(commit)?;
        let Some((_, tree)) = handle.tree_entry(root, path.into())? else {
            return Ok(Vec::new());
        };
        Ok(handle.list_tree(tree)?)
    }

    /// Bytes of a blob of the store.
    pub fn read_blob(&self, id: Oid) -> Result<Vec<u8>, CaptureError> {
        Ok(self.store.handle().read_blob(id)?)
    }

    /// Commit of a snapshot in the store, if its ref exists.
    pub fn snapshot_commit(&self, snapshot_id: &str) -> Result<Option<Oid>, CaptureError> {
        Ok(self.store.handle().snapshot_commit(snapshot_id)?)
    }

    /// Parents of a commit of the store (the user's commits it anchors).
    pub fn commit_parents(&self, commit: Oid) -> Result<Vec<Oid>, CaptureError> {
        Ok(self.store.handle().commit_parents(commit)?)
    }

    /// Forgets what the previous captures read, as after a daemon restart: the next capture of
    /// every worktree runs a full detection.
    pub fn reset_continuity(&self) {
        let mut state = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        *state = capture::State::default();
    }

    /// Takes the writer: a guaranteed prior announces itself first, so a running observation
    /// capture gives way within one MiB of blob data (ADR-TMC-004 § 2).
    fn writer(
        &self,
        prior: bool,
    ) -> (
        std::sync::MutexGuard<'_, capture::State>,
        Option<PriorTicket<'_>>,
    ) {
        let ticket = prior.then(|| PriorTicket::new(&self.priors_waiting));
        let guard = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        (guard, ticket)
    }

    fn prior_waiting(&self) -> bool {
        self.priors_waiting.load(Ordering::Acquire) > 0
    }
}

/// A guaranteed prior waiting for the writer. Dropped (also on panic), it stops counting.
struct PriorTicket<'a>(&'a AtomicUsize);

impl<'a> PriorTicket<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self(counter)
    }
}

impl Drop for PriorTicket<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn to_io(e: StoreError) -> io::Error {
    match e {
        StoreError::Io(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

impl SnapshotRefs for SnapshotStore {
    fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>> {
        let refs = self.store.handle().snapshot_refs().map_err(to_io)?;
        Ok(Some(refs.into_iter().map(|(id, _)| id).collect()))
    }

    fn delete(&mut self, snapshot_ids: &[String]) -> io::Result<()> {
        self.store.handle().delete_refs(snapshot_ids).map_err(to_io)
    }
}

/// Snapshot ids with their store commit, for diagnostics.
pub fn snapshot_refs(store: &SnapshotStore) -> Result<HashMap<String, String>, CaptureError> {
    Ok(store
        .store
        .handle()
        .snapshot_refs()?
        .into_iter()
        .map(|(id, c)| (id, c.to_hex()))
        .collect())
}
