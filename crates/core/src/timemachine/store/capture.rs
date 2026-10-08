//! Capture of a snapshot (TS-TMC-001; ADR-TMC-001 § 1–2, ADR-TMC-006 § 5).
//!
//! **Detection** (escalón 2 in the base design). With continuity, only the paths the engine
//! published since the mark of the previous capture are looked at, with a stat cache. Continuity
//! is taken as broken unless proven: no hint, the hint does not start at our mark, the engine says
//! it is not continuous, an ignore file changed, or a hinted path is a folder. Then the detection
//! is **full**: the working tree is walked with gix, read-only and without filters (never
//! `git status`), and every tracked file is compared with the stat cache or its index entry.
//!
//! **Content** is raw: what is on disk, never converted. A clean file reuses its index blob only
//! if its stat matches, it is not racy and no conversion attribute applies to it.
//!
//! **Write** (escalón 3): blobs in parallel with gix, trees with the gix editor, one barrier, then
//! `pending` in the oplog, the ref, and `complete`. An observation capture gives way to a waiting
//! guaranteed prior, even in the middle of a large blob.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_git::bstr::{BStr, BString, ByteSlice};
use gitraptor_git::tm_write::store::{StoreHandle, TreeEntryKind};
use gitraptor_git::{
    EntryKind, FileKind, FileStat, IndexView, Oid, ReaderOptions, RepoReader, UntrackedKind,
};

use super::meta::{ConflictEntry, META_FORMAT, Meta, MetaWorktree, RegisteredWorktree};
use super::{CaptureError, OBSERVATION_MAX_FILE_BYTES, SnapshotStore, StageTimings, now_ms};
use crate::timemachine::oplog::{
    CompleteInfo, Exclusion, NewSnapshot, Oplog, SnapshotLevel, SnapshotState,
};

/// A request for one snapshot.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub level: SnapshotLevel,
    /// Any worktree of the repo: refs and the worktree list are read from it.
    pub repo: PathBuf,
    /// The worktrees in the snapshot. A guaranteed prior includes every worktree of the
    /// operation's scope; an observation, the one that changed (ADR-TMC-001 § 1).
    pub worktrees: Vec<WorktreeScope>,
    /// Engine sequence the read state reaches (ADR-GRP-013).
    pub engine_mark: Option<i64>,
    pub cause_operation: Option<String>,
    pub cause_event_seq: Option<i64>,
    /// Capture the closed list of credential files when untracked (profile option
    /// `timeMachine.includeCredentialFiles`; BR-TMC-CONS-002). Otherwise they are left out and
    /// declared.
    pub include_credentials: bool,
    /// Checked right before the validity point: if it says no, the capture is discarded without
    /// a row (`CaptureError::Discarded`). The continuous capture uses it so a snapshot never mixes
    /// the state before and after a `git` (ADR-TMC-004 § 2, consistency).
    pub still_valid: Option<ValidityGuard>,
    /// Observation only: when it says yes, the capture gives way, as it does to a guaranteed
    /// prior (`CaptureError::Yielded`, nothing recorded). The continuous capture gives way to new
    /// activity in the repo, so it never competes with the engine during a burst (US-TMC-004).
    pub give_way: Option<ValidityGuard>,
}

/// A check of [`CaptureRequest::still_valid`].
#[derive(Clone)]
pub struct ValidityGuard(pub std::sync::Arc<dyn Fn() -> bool + Send + Sync>);

impl std::fmt::Debug for ValidityGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ValidityGuard")
    }
}

/// One worktree of a capture.
#[derive(Debug, Clone)]
pub struct WorktreeScope {
    /// Name under `wt/` in the snapshot: `[A-Za-z0-9._-]`, 1 to 64 characters.
    pub key: String,
    /// Root of the worktree.
    pub path: PathBuf,
    /// Paths the engine published since the previous capture, if it has them.
    pub hint: Option<ChangeHint>,
}

/// "Paths changed since mark X" from the engine (ADR-TMC-006 § 5, escalón 2). Pending interface
/// with TS-GRP-002/003; without it every capture runs a full detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeHint {
    /// Mark the paths start from: must be the mark of this worktree's previous capture.
    pub since: i64,
    /// Mark the paths reach.
    pub mark: i64,
    /// Worktree-relative paths, with `/`.
    pub paths: Vec<String>,
    /// The engine saw every event between both marks (no gap, no polling, no overflow).
    pub continuous: bool,
}

/// How the changes of a worktree were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// From the engine's paths and the stat cache.
    Engine,
    /// A full walk with gix, with the reason continuity was not proven.
    Full(&'static str),
}

impl Detection {
    fn describe(self) -> String {
        match self {
            Self::Engine => "engine".into(),
            Self::Full(reason) => format!("full:{reason}"),
        }
    }
}

/// What a capture produced.
#[derive(Debug, Clone)]
pub struct CaptureOutcome {
    pub snapshot_id: String,
    /// Commit of the snapshot in the store.
    pub commit: String,
    pub timings: StageTimings,
    /// Nothing changed since the previous capture of the same scope: its tree was reused.
    pub fast_path: bool,
    pub detection: Vec<(String, Detection)>,
    pub exclusions: Vec<Exclusion>,
    /// Some content was left out for its size (observation only).
    pub partial: bool,
    /// Bytes of content the store did not have before.
    pub unique_bytes: u64,
}

/// What the writer remembers between captures. Lost on restart: the first capture after it runs
/// a full detection.
#[derive(Default)]
pub(super) struct State {
    worktrees: HashMap<String, WtState>,
    /// Last root tree per scope, for the fast path.
    last: HashMap<String, LastRoot>,
}

/// A root tree the writer recorded, and what it holds: the fast path reuses it only for the same
/// meta and the same trees of every worktree, never on the writer's word that nothing changed (a
/// capture that gave way at its validity point kept its new trees but recorded no root).
struct LastRoot {
    root: Oid,
    meta: Vec<u8>,
    /// `files` and `index` trees of each worktree, in scope order.
    trees: Vec<(Oid, Oid)>,
}

struct WtState {
    path: PathBuf,
    index_sig: (Option<FileStat>, Option<Oid>),
    index_tree: Oid,
    /// Stage-0 entries in the `index` tree.
    index_map: HashMap<BString, (EntryKind, Oid)>,
    /// Every path in the index, any stage.
    tracked: HashSet<BString>,
    /// Index marks a tree does not keep, for `meta`.
    marks: Marks,
    files_tree: Oid,
    /// Raw content in `files_tree`, by path: an entry here is in the tree with this blob.
    cache: HashMap<BString, Cached>,
    excluded: BTreeMap<BString, &'static str>,
    mark: Option<i64>,
    ignore_sig: Vec<Option<FileStat>>,
    /// Credential files were captured (`include_credentials`) when this state was read.
    credentials: bool,
}

/// Index marks of a worktree, as written to `meta`.
#[derive(Default)]
struct Marks {
    intent_to_add: Vec<String>,
    skip_worktree: Vec<String>,
    conflicts: Vec<ConflictEntry>,
}

impl Marks {
    fn of(index: &IndexView) -> Self {
        let mut m = Self::default();
        for e in &index.entries {
            let p = e.path.to_str_lossy().into_owned();
            if e.intent_to_add {
                m.intent_to_add.push(p.clone());
            }
            if e.skip_worktree {
                m.skip_worktree.push(p.clone());
            }
            if e.stage != 0 {
                m.conflicts.push(ConflictEntry {
                    path: p,
                    stage: e.stage,
                    kind: kind_name(e.kind).into(),
                    id: e.id.to_hex(),
                });
            }
        }
        m
    }
}

#[derive(Clone, Copy)]
struct Cached {
    stat: FileStat,
    kind: EntryKind,
    id: Oid,
    /// Modified at or after the capture that read it began: read again next time.
    racy: bool,
}

/// Credential files that are never captured untracked (SEC-TMC-06, closed list).
pub fn is_credential(path: &BStr) -> bool {
    let name = path.rsplit(|b| *b == b'/').next().unwrap_or(path);
    let name = name.to_str_lossy().to_ascii_lowercase();
    name.starts_with(".env")
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|ext| name.ends_with(ext))
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || matches!(name.as_str(), ".npmrc" | ".pypirc" | ".netrc")
        || name.contains(".tfstate")
        || (name.starts_with("credentials") && name.ends_with(".json"))
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key != "."
        && key != ".."
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A worktree-relative path the engine may send: no root, no `..`, no `.git` component.
fn valid_rela(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.eq_ignore_ascii_case(".git"))
}

fn wall_now() -> (i64, u32) {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    (
        i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        d.subsec_nanos(),
    )
}

fn lstat(path: &Path) -> std::io::Result<Option<FileStat>> {
    match path.symlink_metadata() {
        Ok(m) => Ok(Some(FileStat::of(&m))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) if e.raw_os_error() == Some(20) => Ok(None), // ENOTDIR: a parent became a file
        Err(e) => Err(e),
    }
}

fn abs(root: &Path, rela: &BStr) -> PathBuf {
    root.join(gitraptor_git::bstr::ByteSlice::to_path_lossy(rela.as_bytes()).as_ref())
}

/// A change to apply to a base tree.
enum Change {
    Upsert(BString, EntryKind, Oid),
    Remove(BString),
}

/// A file to read and write as a blob.
struct Job {
    wt: usize,
    path: BString,
    full: PathBuf,
}

enum JobResult {
    Written {
        wt: usize,
        path: BString,
        cached: Cached,
        new: bool,
        bytes: u64,
    },
    /// Gone between the detection and the read, or not a file any more.
    Gone { wt: usize, path: BString },
    /// Too large for an observation capture.
    TooLarge { wt: usize, path: BString },
}

/// Detection of one worktree: what to read and what to change.
struct WtWork {
    key: String,
    state: WtState,
    detection: Detection,
    base: Oid,
    changes: Vec<Change>,
    jobs: Vec<Job>,
    meta: MetaWorktree,
}

/// Stopwatch over the stages.
struct Laps(Instant);

impl Laps {
    fn lap(&mut self) -> Duration {
        let now = Instant::now();
        let d = now - self.0;
        self.0 = now;
        d
    }
}

impl SnapshotStore {
    /// Takes one snapshot: detection, blobs, trees, commit, then the validity point (`pending`
    /// row, ref, `complete` row). Nothing is written to the user's repository.
    pub fn capture(
        &self,
        oplog: &Mutex<Oplog>,
        req: &CaptureRequest,
    ) -> Result<CaptureOutcome, CaptureError> {
        validate(req)?;
        let t_all = Instant::now();
        let prior = req.level != SnapshotLevel::Observation;
        let (mut state, _ticket) = self.writer(prior);
        let timings = StageTimings {
            queue: t_all.elapsed(),
            ..StageTimings::default()
        };
        let mut works = Vec::with_capacity(req.worktrees.len());
        let result = self.capture_locked(&mut state, &mut works, oplog, req, prior, t_all, timings);
        if result.is_err() {
            // What was read stays true (the stat cache names blobs the store has, the `index`
            // tree matches its index signature), so it is kept; but the next capture of these
            // worktrees must look at everything again: their mark is dropped.
            for w in works.drain(..) {
                let mut st = w.state;
                st.mark = None;
                state.worktrees.insert(w.key, st);
            }
        }
        result
    }

    /// The capture once the writer is held. Every worktree detected so far is in `works`, so the
    /// caller can keep its state if this fails or gives way.
    #[allow(clippy::too_many_arguments)]
    fn capture_locked(
        &self,
        state: &mut State,
        works: &mut Vec<WtWork>,
        oplog: &Mutex<Oplog>,
        req: &CaptureRequest,
        prior: bool,
        t_all: Instant,
        mut timings: StageTimings,
    ) -> Result<CaptureOutcome, CaptureError> {
        let yield_now =
            || !prior && (self.prior_waiting() || req.give_way.as_ref().is_some_and(|g| (g.0)()));
        if yield_now() {
            return Err(CaptureError::Yielded);
        }
        let mut laps = Laps(Instant::now());
        let started = wall_now();
        let handle = self.store.handle();

        // ---- detection -------------------------------------------------------------------
        let main = RepoReader::open(&req.repo, &ReaderOptions::default())?;
        let branches: BTreeMap<String, String> = main
            .branch_tips()?
            .into_iter()
            .map(|(name, id)| (name, id.to_hex()))
            .collect();
        let stash = main.stash()?.map(|s| s.to_hex());
        let registered = registered_worktrees(&main)?;
        let gaps: Vec<String> = main
            .history_gaps()
            .into_iter()
            .map(|g| g.as_str().to_owned())
            .collect();

        let mut anchor_time = Duration::ZERO;
        for (i, scope) in req.worktrees.iter().enumerate() {
            let prev = state.worktrees.remove(&scope.key);
            let mut anchor_lap = Duration::ZERO;
            let own;
            let reader = if scope.path == req.repo {
                &main
            } else {
                own = RepoReader::open(&scope.path, &ReaderOptions::default())?;
                &own
            };
            let work = self.detect(
                &handle,
                reader,
                i,
                scope,
                req.include_credentials,
                prev,
                started,
                &mut anchor_lap,
                &yield_now,
            )?;
            anchor_time += anchor_lap;
            works.push(work);
            if yield_now() {
                return Err(CaptureError::Yielded);
            }
        }
        timings.detect = laps.lap().saturating_sub(anchor_time);

        // ---- anchoring of the commits the snapshot points to --------------------------------
        let mut parents: Vec<String> = Vec::new();
        for w in works.iter() {
            if let Some(c) = &w.meta.head_commit {
                parents.push(c.clone());
            }
        }
        parents.extend(branches.values().cloned());
        parents.extend(stash.iter().cloned());
        let mut seen = HashSet::new();
        parents.retain(|p| seen.insert(p.clone()));
        let parent_ids: Vec<Oid> = parents.iter().filter_map(|p| Oid::from_hex(p)).collect();
        let missing: Vec<Oid> = parent_ids
            .iter()
            .copied()
            .filter(|p| !handle.has(*p))
            .collect();
        if !missing.is_empty() {
            handle.copy_closure(&main, &missing)?;
        }
        drop(main);
        timings.anchor = laps.lap() + anchor_time;

        // ---- blobs ---------------------------------------------------------------------------
        let jobs: Vec<Job> = works.iter_mut().flat_map(|w| w.jobs.drain(..)).collect();
        let results = self.write_blobs(jobs, prior, &yield_now)?;
        let mut unique_bytes = 0;
        let mut partial = false;
        for r in results {
            match r {
                JobResult::Written {
                    wt,
                    path,
                    cached,
                    new,
                    bytes,
                } => {
                    timings.files_read += 1;
                    timings.bytes_read += bytes;
                    if new {
                        unique_bytes += bytes;
                    }
                    let w = &mut works[wt];
                    w.changes
                        .push(Change::Upsert(path.clone(), cached.kind, cached.id));
                    w.state.excluded.remove(&path);
                    w.state.cache.insert(path, cached);
                }
                JobResult::Gone { wt, path } => {
                    let w = &mut works[wt];
                    w.state.cache.remove(&path);
                    w.changes.push(Change::Remove(path));
                }
                JobResult::TooLarge { wt, path } => {
                    partial = true;
                    let w = &mut works[wt];
                    w.state.cache.remove(&path);
                    w.state.excluded.insert(path.clone(), "too-large");
                    w.changes.push(Change::Remove(path));
                }
            }
        }
        timings.blobs = laps.lap();
        if yield_now() {
            return Err(CaptureError::Yielded);
        }

        // ---- trees and commit ------------------------------------------------------------------
        for w in works.iter_mut() {
            let files = if w.changes.is_empty() {
                w.base
            } else {
                let mut edit = handle.edit_tree(w.base)?;
                for c in &w.changes {
                    match c {
                        Change::Upsert(p, kind, id) => {
                            edit.upsert(p.as_bstr(), TreeEntryKind::of(*kind), *id)?;
                        }
                        Change::Remove(p) => edit.remove(p.as_bstr())?,
                    }
                }
                edit.write()?
            };
            w.state.files_tree = files;
        }
        let mut exclusions = Vec::new();
        for w in works.iter() {
            for (path, reason) in &w.state.excluded {
                exclusions.push(Exclusion {
                    path: format!("{}:{}", w.key, path.to_str_lossy()),
                    reason: (*reason).to_owned(),
                });
            }
        }
        for g in &gaps {
            exclusions.push(Exclusion {
                path: String::new(),
                reason: g.clone(),
            });
        }
        let meta = Meta {
            format: META_FORMAT,
            scope: works.iter().map(|w| w.key.clone()).collect(),
            worktrees: works.iter().map(|w| w.meta.clone()).collect(),
            registered,
            branches,
            stash,
            exclusions: exclusions.clone(),
            gaps,
        };
        let meta_bytes = meta.to_bytes();
        let scope_key = meta.scope.join(",");
        let trees: Vec<(Oid, Oid)> = works
            .iter()
            .map(|w| (w.state.files_tree, w.state.index_tree))
            .collect();
        let reuse = state
            .last
            .get(&scope_key)
            .filter(|last| last.meta == meta_bytes && last.trees == trees)
            .map(|last| last.root);
        let fast_path = reuse.is_some();
        let root = match reuse {
            Some(root) => root,
            None => {
                let (meta_id, _) = handle.write_blob(&meta_bytes)?;
                let mut edit = handle.edit_tree(Oid::empty_tree())?;
                for w in works.iter() {
                    edit.upsert(
                        format!("wt/{}/files", w.key).as_str().into(),
                        TreeEntryKind::Tree,
                        w.state.files_tree,
                    )?;
                    edit.upsert(
                        format!("wt/{}/index", w.key).as_str().into(),
                        TreeEntryKind::Tree,
                        w.state.index_tree,
                    )?;
                }
                edit.upsert("meta".into(), TreeEntryKind::Blob, meta_id)?;
                edit.write()?
            }
        };
        let mut message = format!("tm snapshot ({})\n\n", req.level.as_str());
        for w in works.iter() {
            message.push_str(&format!("detection {}={}\n", w.key, w.detection.describe()));
        }
        let commit = handle.commit(root, &parent_ids, &message)?;
        timings.trees = laps.lap();

        // ---- validity point: barrier, pending row, ref, complete row ----------------------------
        if yield_now() {
            return Err(CaptureError::Yielded);
        }
        if req.still_valid.as_ref().is_some_and(|g| !(g.0)()) {
            return Err(CaptureError::Discarded);
        }
        // The flush of the `pending` row is the full barrier of ADR-TMC-001 § 4: the oplog runs
        // with `fullfsync` (macOS) and `synchronous=FULL`, and that flush empties the drive
        // cache, so every object synced above is on stable storage before the ref exists. No
        // separate barrier is paid.
        let snapshot_id = record(oplog, &handle, req, commit, unique_bytes, &exclusions)?;
        timings.ref_oplog = laps.lap();

        let detection = works.iter().map(|w| (w.key.clone(), w.detection)).collect();
        for w in works.drain(..) {
            let mut st = w.state;
            if let Some(h) = req
                .worktrees
                .iter()
                .find(|s| s.key == w.key)
                .and_then(|s| s.hint.as_ref())
            {
                st.mark = Some(h.mark);
            } else {
                st.mark = req.engine_mark;
            }
            state.worktrees.insert(w.key, st);
        }
        state.last.insert(
            scope_key,
            LastRoot {
                root,
                meta: meta_bytes,
                trees,
            },
        );
        timings.total = t_all.elapsed();
        Ok(CaptureOutcome {
            snapshot_id,
            commit: commit.to_hex(),
            timings,
            fast_path,
            detection,
            exclusions,
            partial,
            unique_bytes,
        })
    }

    /// Detection of one worktree. Rebuilds its `index` tree when the index changed, anchoring
    /// staged blobs the store lacks (time reported in `anchor`). The index is parsed only when
    /// its stat or checksum changed, or for a full detection.
    #[allow(clippy::too_many_arguments)]
    fn detect(
        &self,
        handle: &StoreHandle,
        reader: &RepoReader,
        wt_index: usize,
        scope: &WorktreeScope,
        credentials: bool,
        prev: Option<WtState>,
        started: (i64, u32),
        anchor_time: &mut Duration,
        yield_now: &dyn Fn() -> bool,
    ) -> Result<WtWork, CaptureError> {
        let root = reader
            .workdir()
            .ok_or_else(|| CaptureError::InvalidInput("worktree without a working tree".into()))?;
        let sig = reader.index_signature();
        let ignore_sig: Vec<Option<FileStat>> = reader
            .ignore_sources()
            .iter()
            .map(|p| lstat(p).ok().flatten())
            .collect();

        let had_prev = prev.as_ref().is_some_and(|p| p.path == scope.path);
        let mut st = match prev {
            Some(p) if p.path == scope.path => p,
            _ => WtState {
                path: scope.path.clone(),
                index_sig: (None, None),
                index_tree: Oid::empty_tree(),
                index_map: HashMap::new(),
                tracked: HashSet::new(),
                marks: Marks::default(),
                files_tree: Oid::empty_tree(),
                cache: HashMap::new(),
                excluded: BTreeMap::new(),
                mark: None,
                ignore_sig: Vec::new(),
                credentials,
            },
        };

        let mut detection = match &scope.hint {
            _ if !had_prev => Detection::Full("first-capture"),
            _ if st.credentials != credentials => Detection::Full("credential-option-changed"),
            None => Detection::Full("no-hint"),
            Some(h) if !h.continuous => Detection::Full("not-continuous"),
            Some(h) if st.mark != Some(h.since) => Detection::Full("mark-mismatch"),
            Some(_) if st.ignore_sig != ignore_sig => Detection::Full("ignore-rules-changed"),
            Some(h) if h.paths.iter().any(|p| !valid_rela(p)) => Detection::Full("invalid-hint"),
            Some(h)
                if h.paths
                    .iter()
                    .any(|p| p.rsplit('/').next() == Some(".gitignore")) =>
            {
                Detection::Full("ignore-rules-changed")
            }
            Some(_) => Detection::Engine,
        };
        st.ignore_sig = ignore_sig;
        st.credentials = credentials;

        // The `index` tree mirrors the user's index (stage 0, no intent-to-add entries).
        let index_changed = !had_prev || st.index_sig != sig || sig.0.is_none();
        let mut index: Option<IndexView> = None;
        let mut status_changed: Vec<BString> = Vec::new();
        if index_changed {
            if yield_now() {
                return Err(CaptureError::Yielded);
            }
            let view = reader.index_view()?;
            let t = Instant::now();
            let (map, tracked) = index_maps(&view);
            let staged: Vec<Oid> = map
                .values()
                .filter(|(k, _)| *k != EntryKind::Gitlink)
                .map(|(_, id)| *id)
                .filter(|id| !handle.has(*id))
                .collect();
            if !staged.is_empty() {
                handle.copy_closure(reader, &staged)?;
            }
            *anchor_time += t.elapsed();
            let mut edit = handle.edit_tree(if had_prev {
                st.index_tree
            } else {
                Oid::empty_tree()
            })?;
            for (path, (kind, id)) in &map {
                if st.index_map.get(path) != Some(&(*kind, *id)) {
                    edit.upsert(path.as_bstr(), TreeEntryKind::of(*kind), *id)?;
                }
            }
            for path in st.index_map.keys() {
                if !map.contains_key(path) {
                    edit.remove(path.as_bstr())?;
                }
            }
            let tree = edit.write()?;
            status_changed = st.tracked.symmetric_difference(&tracked).cloned().collect();
            st.index_tree = tree;
            st.index_map = map;
            st.tracked = tracked;
            st.marks = Marks::of(&view);
            st.index_sig = sig;
            index = Some(view);
        }

        let mut work = WtWork {
            key: scope.key.clone(),
            detection,
            base: st.files_tree,
            changes: Vec::new(),
            jobs: Vec::new(),
            meta: worktree_meta(reader, &scope.key, &scope.path, &st.marks)?,
            state: st,
        };

        if let (Detection::Engine, Some(h)) = (detection, &scope.hint) {
            let mut paths: Vec<BString> =
                h.paths.iter().map(|p| BString::from(p.as_str())).collect();
            paths.extend(status_changed);
            paths.sort();
            paths.dedup();
            if !self.detect_paths(reader, &root, wt_index, &mut work, &paths)? {
                detection = Detection::Full("folder-in-hint");
                work.detection = detection;
                work.changes.clear();
                work.jobs.clear();
            }
        }
        if let Detection::Full(_) = detection {
            let view = match index {
                Some(v) => v,
                None => reader.index_view()?,
            };
            work.base = work.state.index_tree;
            self.detect_full(
                reader, &root, wt_index, &mut work, &view, started, yield_now,
            )?;
        }
        Ok(work)
    }

    /// Engine paths: only these are looked at. Returns `false` if continuity cannot hold (a
    /// hinted path is a folder).
    fn detect_paths(
        &self,
        reader: &RepoReader,
        root: &Path,
        wt_index: usize,
        work: &mut WtWork,
        paths: &[BString],
    ) -> Result<bool, CaptureError> {
        // Built on first need: most hints only name tracked files.
        let mut ignores = None;
        let st = &mut work.state;
        for path in paths {
            let full = abs(root, path.as_bstr());
            if let Some(repo) = nested_repo(root, path.as_bstr()) {
                st.excluded.insert(repo, "nested-repo");
                if st.cache.remove(path).is_some() {
                    work.changes.push(Change::Remove(path.clone()));
                }
                continue;
            }
            let Some(stat) = lstat(&full)? else {
                st.cache.remove(path);
                st.excluded.remove(path);
                work.changes.push(Change::Remove(path.clone()));
                continue;
            };
            if stat.kind == FileKind::Other {
                if full.is_dir() {
                    if st
                        .index_map
                        .get(path)
                        .is_some_and(|(k, _)| *k == EntryKind::Gitlink)
                    {
                        continue;
                    }
                    return Ok(false);
                }
                continue;
            }
            if !st.tracked.contains(path) {
                let check = match ignores.as_mut() {
                    Some(c) => c,
                    None => ignores.insert(reader.ignore_check()?),
                };
                if check.is_ignored(path.as_bstr(), false)? {
                    if st.cache.remove(path).is_some() {
                        work.changes.push(Change::Remove(path.clone()));
                    }
                    continue;
                }
                if !st.credentials && is_credential(path.as_bstr()) {
                    st.excluded.insert(path.clone(), "credential");
                    if st.cache.remove(path).is_some() {
                        work.changes.push(Change::Remove(path.clone()));
                    }
                    continue;
                }
            }
            match st.cache.get(path) {
                Some(c) if c.stat == stat && !c.racy => {}
                _ => {
                    work.jobs.push(Job {
                        wt: wt_index,
                        path: path.clone(),
                        full,
                    });
                }
            }
        }
        Ok(true)
    }

    /// Full walk: every tracked file against the stat cache or its index entry, plus the
    /// untracked files gix finds (ignored ones never). Changes are relative to the `index` tree.
    #[allow(clippy::too_many_arguments)]
    fn detect_full(
        &self,
        reader: &RepoReader,
        root: &Path,
        wt_index: usize,
        work: &mut WtWork,
        index: &IndexView,
        started: (i64, u32),
        yield_now: &dyn Fn() -> bool,
    ) -> Result<(), CaptureError> {
        let mut conversions = reader.conversions()?;
        let st = &mut work.state;
        let mut cache: HashMap<BString, Cached> = HashMap::with_capacity(st.cache.len());
        let mut excluded = BTreeMap::new();
        let mut seen: HashSet<&BStr> = HashSet::new();
        for (n, e) in index.entries.iter().enumerate() {
            if n % 512 == 511 && yield_now() {
                return Err(CaptureError::Yielded);
            }
            let path = e.path.as_bstr();
            if !seen.insert(path) {
                continue;
            }
            if e.kind == EntryKind::Gitlink {
                excluded.insert(e.path.clone(), "submodule");
                continue;
            }
            let in_tree = st.index_map.get(path).copied();
            let full = abs(root, path);
            let stat = match lstat(&full)? {
                Some(s) if s.kind != FileKind::Other => s,
                _ => {
                    if in_tree.is_some() {
                        work.changes.push(Change::Remove(e.path.clone()));
                    }
                    continue;
                }
            };
            let staged_clean = e.stage == 0
                && !e.intent_to_add
                && e.matches_file(&stat, index.mtime)
                && in_tree == Some((e.kind, e.id));
            if let Some(c) = st.cache.get(path).filter(|c| c.stat == stat && !c.racy) {
                if in_tree != Some((c.kind, c.id)) {
                    work.changes
                        .push(Change::Upsert(e.path.clone(), c.kind, c.id));
                }
                cache.insert(e.path.clone(), *c);
            } else if staged_clean && !conversions.converts(path)? {
                cache.insert(
                    e.path.clone(),
                    Cached {
                        stat,
                        kind: e.kind,
                        id: e.id,
                        racy: stat.modified_since(started),
                    },
                );
            } else {
                work.jobs.push(Job {
                    wt: wt_index,
                    path: e.path.clone(),
                    full,
                });
            }
        }
        if yield_now() {
            return Err(CaptureError::Yielded);
        }
        for u in reader.untracked()? {
            match u.kind {
                UntrackedKind::NestedRepo => {
                    excluded.insert(u.path, "nested-repo");
                }
                UntrackedKind::Other => {}
                UntrackedKind::File | UntrackedKind::Symlink => {
                    if !st.credentials && is_credential(u.path.as_bstr()) {
                        excluded.insert(u.path, "credential");
                        continue;
                    }
                    let full = abs(root, u.path.as_bstr());
                    let Some(stat) = lstat(&full)? else { continue };
                    match st.cache.get(&u.path).filter(|c| c.stat == stat && !c.racy) {
                        Some(c) => {
                            work.changes
                                .push(Change::Upsert(u.path.clone(), c.kind, c.id));
                            cache.insert(u.path, *c);
                        }
                        None => work.jobs.push(Job {
                            wt: wt_index,
                            path: u.path,
                            full,
                        }),
                    }
                }
            }
        }
        st.cache = cache;
        st.excluded = excluded;
        Ok(())
    }

    /// Reads and writes the blobs, several files at a time.
    fn write_blobs(
        &self,
        jobs: Vec<Job>,
        prior: bool,
        yield_now: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<JobResult>, CaptureError> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let started = wall_now();
        let limit = if prior {
            None
        } else {
            Some(OBSERVATION_MAX_FILE_BYTES)
        };
        // An observation has no latency gate (ADR-TMC-006 § 1): one thread, so it never
        // competes with the engine for the machine (US-TMC-004). A prior uses up to 8.
        let threads = if prior {
            std::thread::available_parallelism()
                .map_or(4, |n| n.get())
                .min(8)
                .min(jobs.len())
        } else {
            1
        };
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let run = |handle: StoreHandle| -> Result<Vec<JobResult>, CaptureError> {
            let mut out = Vec::new();
            let should_yield = || {
                let y = yield_now();
                if y {
                    stop.store(true, Ordering::Release);
                }
                y || stop.load(Ordering::Acquire)
            };
            loop {
                let i = next.fetch_add(1, Ordering::AcqRel);
                let Some(job) = jobs.get(i) else { break };
                if should_yield() {
                    return Err(CaptureError::Yielded);
                }
                match write_one(&handle, job, limit, started, &should_yield) {
                    Ok(r) => out.push(r),
                    Err(e) => {
                        stop.store(true, Ordering::Release);
                        return Err(e);
                    }
                }
            }
            Ok(out)
        };
        if threads <= 1 {
            return run(self.store.handle());
        }
        let results: Vec<Result<Vec<JobResult>, CaptureError>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    let handle = self.store.handle();
                    let run = &run;
                    s.spawn(move || run(handle))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        Err(CaptureError::InvalidInput("blob writer panicked".into()))
                    })
                })
                .collect()
        });
        let mut out = Vec::with_capacity(jobs.len());
        let mut yielded = false;
        let mut error = None;
        for r in results {
            match r {
                Ok(v) => out.extend(v),
                Err(CaptureError::Yielded) => yielded = true,
                Err(e) => {
                    error.get_or_insert(e);
                }
            }
        }
        if let Some(e) = error {
            return Err(e);
        }
        if yielded {
            return Err(CaptureError::Yielded);
        }
        Ok(out)
    }
}

/// Reads one file (never following a symlink) and writes its blob.
fn write_one(
    handle: &StoreHandle,
    job: &Job,
    limit: Option<u64>,
    started: (i64, u32),
    should_yield: &dyn Fn() -> bool,
) -> Result<JobResult, CaptureError> {
    let gone = || JobResult::Gone {
        wt: job.wt,
        path: job.path.clone(),
    };
    let Some(link_stat) = lstat(&job.full)? else {
        return Ok(gone());
    };
    if link_stat.kind == FileKind::Symlink {
        let target = match std::fs::read_link(&job.full) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(gone()),
            Err(e) => return Err(e.into()),
        };
        let bytes = path_bytes(&target);
        let (id, new) = handle.write_blob(&bytes)?;
        return Ok(JobResult::Written {
            wt: job.wt,
            path: job.path.clone(),
            cached: Cached {
                stat: link_stat,
                kind: EntryKind::Symlink,
                id,
                racy: link_stat.modified_since(started),
            },
            new,
            bytes: bytes.len() as u64,
        });
    }
    let mut file = match open_nofollow(&job.full) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(gone()),
        Err(e) => return Err(e.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(gone());
    }
    let stat = FileStat::of(&meta);
    let Some(kind) = EntryKind::of_file(stat.kind) else {
        return Ok(gone());
    };
    let size = meta.len();
    if limit.is_some_and(|l| size > l) {
        return Ok(JobResult::TooLarge {
            wt: job.wt,
            path: job.path.clone(),
        });
    }
    let (id, new) = if size > 1 << 20 {
        (
            handle.write_blob_stream(&mut file, size, should_yield)?,
            true,
        )
    } else {
        let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        file.read_to_end(&mut bytes)?;
        if bytes.len() as u64 != size {
            return Err(CaptureError::Io(std::io::Error::other(
                "file changed while read",
            )));
        }
        handle.write_blob(&bytes)?
    };
    Ok(JobResult::Written {
        wt: job.wt,
        path: job.path.clone(),
        cached: Cached {
            stat,
            kind,
            id,
            racy: stat.modified_since(started),
        },
        new,
        bytes: size,
    })
}

fn open_nofollow(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    #[cfg(windows)]
    {
        // `FILE_FLAG_OPEN_REPARSE_POINT`: a link or junction is opened itself and refused, never
        // followed (DS-TS-TMC-003 W3). Another reparse point (compressed, deduplicated) is data
        // that only reads right when opened normally, and it redirects no name.
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let file = options
            .clone()
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let meta = file.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "a link or junction is never followed",
            ));
        }
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return Ok(file);
        }
        let file = options.open(path)?;
        if file.metadata()?.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "a link or junction is never followed",
            ));
        }
        return Ok(file);
    }
    #[allow(unreachable_code)]
    options.open(path)
}

fn path_bytes(p: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        p.to_string_lossy().replace('\\', "/").into_bytes()
    }
}

/// The nearest folder above `path` (inside the worktree) with its own `.git`, if any.
fn nested_repo(root: &Path, path: &BStr) -> Option<BString> {
    let parts: Vec<&[u8]> = path.split(|b| *b == b'/').collect();
    for n in 1..parts.len() {
        let prefix = BString::from(parts[..n].join(&b'/'));
        if abs(root, prefix.as_bstr())
            .join(".git")
            .symlink_metadata()
            .is_ok()
        {
            return Some(prefix);
        }
    }
    None
}

/// Stage-0 entries of the `index` tree (no intent-to-add) and every tracked path.
fn index_maps(index: &IndexView) -> (HashMap<BString, (EntryKind, Oid)>, HashSet<BString>) {
    let mut map = HashMap::with_capacity(index.entries.len());
    let mut tracked = HashSet::with_capacity(index.entries.len());
    for e in &index.entries {
        tracked.insert(e.path.clone());
        if e.stage == 0 && !e.intent_to_add {
            map.insert(e.path.clone(), (e.kind, e.id));
        }
    }
    (map, tracked)
}

fn kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Blob => "blob",
        EntryKind::Executable => "executable",
        EntryKind::Symlink => "symlink",
        EntryKind::Gitlink => "gitlink",
    }
}

fn worktree_meta(
    reader: &RepoReader,
    key: &str,
    path: &Path,
    marks: &Marks,
) -> Result<MetaWorktree, CaptureError> {
    let (head_branch, head_commit, detached) = reader.head_tip()?;
    Ok(MetaWorktree {
        key: key.to_owned(),
        path: path.to_string_lossy().into_owned(),
        head_branch,
        head_commit: head_commit.map(|c| c.to_hex()),
        detached,
        intent_to_add: marks.intent_to_add.clone(),
        skip_worktree: marks.skip_worktree.clone(),
        conflicts: marks.conflicts.clone(),
    })
}

/// The main worktree and the linked ones, with the branch their `HEAD` file names.
fn registered_worktrees(main: &RepoReader) -> Result<Vec<RegisteredWorktree>, CaptureError> {
    let branch_of = |git_dir: &Path| {
        std::fs::read_to_string(git_dir.join("HEAD"))
            .ok()
            .and_then(|h| {
                h.trim()
                    .strip_prefix("ref: refs/heads/")
                    .map(ToOwned::to_owned)
            })
    };
    let common = main.common_dir().to_owned();
    let mut out = Vec::new();
    if let Some(parent) = common.parent().filter(|_| common.ends_with(".git")) {
        out.push(RegisteredWorktree {
            id: None,
            path: parent.to_string_lossy().into_owned(),
            branch: branch_of(&common),
            locked: false,
        });
    }
    for wt in main.worktrees()? {
        out.push(RegisteredWorktree {
            id: Some(wt.id),
            path: wt.path.to_string_lossy().into_owned(),
            branch: branch_of(&wt.git_dir),
            locked: wt.locked,
        });
    }
    Ok(out)
}

fn validate(req: &CaptureRequest) -> Result<(), CaptureError> {
    if req.worktrees.is_empty() {
        return Err(CaptureError::InvalidInput("no worktree".into()));
    }
    let mut keys = HashSet::new();
    for w in &req.worktrees {
        if !valid_key(&w.key) {
            return Err(CaptureError::InvalidInput(format!("bad key {:?}", w.key)));
        }
        if !keys.insert(&w.key) {
            return Err(CaptureError::InvalidInput("duplicate key".into()));
        }
        if !w.path.is_absolute() {
            return Err(CaptureError::InvalidInput(
                "worktree path must be absolute".into(),
            ));
        }
    }
    Ok(())
}

/// The validity point: `pending` before the ref, the ref, then `complete`. If the ref cannot be
/// created the row becomes `discarded`; a crash in between leaves `pending` + ref, which the
/// recovery discards (ADR-TMC-003 § 6.1).
fn record(
    oplog: &Mutex<Oplog>,
    handle: &StoreHandle,
    req: &CaptureRequest,
    commit: Oid,
    unique_bytes: u64,
    exclusions: &[Exclusion],
) -> Result<String, CaptureError> {
    let mut log = oplog.lock().unwrap_or_else(|p| p.into_inner());
    let id = log.begin_snapshot(
        &NewSnapshot {
            level: req.level,
            worktrees: req.worktrees.iter().map(|w| w.key.clone()).collect(),
            engine_mark: req.engine_mark,
            cause_operation: req.cause_operation.clone(),
            cause_event_seq: req.cause_event_seq,
        },
        now_ms(),
    )?;
    if let Err(e) = handle.create_ref(&id, commit) {
        let _ = log.set_snapshot_state(&id, SnapshotState::Discarded, now_ms());
        return Err(e.into());
    }
    log.complete_snapshot(
        &id,
        &CompleteInfo {
            unique_size_bytes: unique_bytes,
            exclusions: exclusions.to_vec(),
        },
        now_ms(),
    )?;
    Ok(id)
}
