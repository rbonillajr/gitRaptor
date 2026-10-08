//! The task of one worktree (ADR-GRP-010 § 3 to § 6).
//!
//! Fixed debounce window: the first event opens it, and when it closes the
//! worktree is read again with every accumulated path; events that arrive
//! during the recompute open the next one, so a continuous burst publishes
//! once per window instead of waiting for the burst to end. The periodic
//! reconciliation runs here too, after the window is flushed, so events in
//! flight never look like gaps.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use gitraptor_api::UntrustedName;
use gitraptor_api::clock;
use gitraptor_api::messages::{GitEventDetails, GitEventKind, HeadView, WorktreeStatus};
use gitraptor_git::{ReaderOptions, RepoReader};

use super::{GapMark, Marks, ObservedBatch, RawEvent, Shared, WtMsg, wall_now};
use crate::observe::{self, WorktreeRead};
use crate::profile::GapCause;

struct Window {
    deadline: Instant,
    t_recv: u64,
    /// The watcher lost events: the flush is a reconciliation with a gap.
    overflow: bool,
}

struct Task {
    shared: Arc<Shared>,
    repo_id: String,
    root: PathBuf,
    /// Its Git directory: `HEAD` is read from it before each batch.
    git_dir: PathBuf,
    /// The `HEAD` of the last batch handed over (or read at the start).
    sent_head: Vec<u8>,
    main: bool,
    admin: Option<String>,
    last: WorktreeRead,
    /// Branch of `HEAD` outside any operation in progress.
    stable_branch: Option<String>,
    window: Option<Window>,
    /// Wall time of the last event received: where an overflow gap starts.
    last_event_ms: i64,
    overflow_from_ms: Option<i64>,
    ignore: IgnoreCache,
    next_periodic: Instant,
    last_periodic_ms: i64,
    degraded: bool,
    next_degraded: Instant,
    /// Files outside every ignored folder changed in the open window.
    touched: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    shared: Arc<Shared>,
    repo_id: String,
    initial: WorktreeRead,
    git_dir: PathBuf,
    head: Vec<u8>,
    degraded: bool,
    ignored: Arc<IgnoredPrefixes>,
    rx: Receiver<WtMsg>,
) {
    let config = shared.config;
    // The task compares reads without the ahead/behind, which the daemon
    // counts for the whole repo (US-GRP-012).
    let mut initial = initial;
    observe::set_divergence(
        &mut initial.view,
        gitraptor_api::messages::DivergenceView::Unreadable,
    );
    let root = PathBuf::from(initial.view.path.raw());
    let now = Instant::now();
    // Worktrees are staggered over the period so they do not all
    // reconcile at once.
    let stagger = stagger(&root, config.periodic);
    let mut task = Task {
        repo_id,
        main: initial.view.main,
        admin: initial.view.admin_name.as_ref().map(|n| n.raw().to_owned()),
        stable_branch: if initial.in_progress {
            None
        } else {
            branch_of(&initial)
        },
        last: initial,
        window: None,
        last_event_ms: wall_now().0,
        overflow_from_ms: None,
        ignore: IgnoreCache::with_prefixes(ignored),
        next_periodic: now + config.periodic.min(stagger + config.periodic / 2),
        last_periodic_ms: wall_now().0,
        degraded,
        next_degraded: now + config.degraded_poll,
        touched: false,
        shared,
        root,
        git_dir,
        sent_head: head,
    };
    // The watch is running: read once more, so a change written between
    // the reconciliation that started the task and the watch is not lost.
    task.open_window(clock::monotonic_ns(), Duration::ZERO);
    loop {
        let deadline = task.next_deadline();
        let wait = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(WtMsg::Paths(t_recv, paths)) => {
                let index = task.git_dir.join("index");
                if paths.contains(&index) {
                    task.ignore.revalidate(&task.root);
                }
                if !task.ignore.keep_any(&task.root, paths) {
                    continue;
                }
                task.last_event_ms = wall_now().0;
                task.touched = true;
                task.open_window(t_recv, task.window_len());
            }
            Ok(WtMsg::Rescan(t_recv)) => {
                task.overflow_from_ms.get_or_insert(task.last_event_ms);
                task.open_window(t_recv, task.window_len());
                if let Some(w) = task.window.as_mut() {
                    w.overflow = true;
                }
            }
            Ok(WtMsg::Reconcile(t_recv)) => task.open_window(t_recv, Duration::ZERO),
            Ok(WtMsg::IgnoreRules) => task.ignore.clear(),
            // Going dormant: what is in the window is handed over first
            // (ADR-GRP-010, Enmienda 2026-10-07, N1 step 1).
            Ok(WtMsg::Sleep(done)) => {
                if let Some(window) = task.window.take() {
                    task.flush(window);
                }
                let _ = done.send(super::Slept {
                    root: task.root.clone(),
                    main: task.main,
                    admin: task.admin.clone(),
                    fingerprint: task.last.fingerprint.clone(),
                });
                return;
            }
            Ok(WtMsg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if task.window.as_ref().is_some_and(|w| w.deadline <= now) {
            let window = task.window.take();
            if let Some(window) = window {
                task.flush(window);
            }
        } else if task.window.is_none() && task.next_periodic <= now {
            task.periodic();
        }
        if task.degraded && task.next_degraded <= Instant::now() {
            task.next_degraded = Instant::now() + task.shared.config.degraded_poll;
            if task.window.is_none() {
                task.open_window(clock::monotonic_ns(), Duration::ZERO);
            }
        }
    }
}

impl Task {
    fn window_len(&self) -> Duration {
        let c = self.shared.config;
        c.window.saturating_sub(c.timer_slack)
    }

    fn open_window(&mut self, t_recv: u64, len: Duration) {
        if self.window.is_none() {
            self.window = Some(Window {
                deadline: Instant::now() + len,
                t_recv,
                overflow: false,
            });
        }
    }

    fn next_deadline(&self) -> Instant {
        let mut next = match &self.window {
            Some(w) => w.deadline,
            None => self.next_periodic,
        };
        if self.degraded {
            next = next.min(self.next_degraded);
        }
        next
    }

    fn read(&self) -> WorktreeRead {
        self.shared
            .recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        observe::read_worktree(&self.root, self.main, self.admin.as_deref())
    }

    fn flush(&mut self, window: Window) {
        let t_flush = clock::monotonic_ns();
        let head = self.head();
        let read = self.read();
        let t_computed = clock::monotonic_ns();
        let (now_ms, offset_s) = wall_now();
        let mut events = Vec::new();
        if let Some(switch) = self.branch_switch(&read) {
            events.push(RawEvent {
                worktree: self.root.clone(),
                kind: GitEventKind::BranchSwitch,
                details: switch,
                observed_ms: now_ms,
                offset_s,
            });
        }
        let changed = read != self.last;
        // `HEAD` is read before the worktree: a `git` that renames it in
        // between leaves a batch with the old one and a read with the new
        // one. The next flush has nothing else to publish, but must hand the
        // new `HEAD` over, or the Time Machine never sees the worktree calm.
        let head_moved = head != self.sent_head;
        // An overflow always opens a gap, from the last event received
        // until this reconciliation ends (ADR-GRP-013 § 5).
        let gap = if window.overflow {
            let started_ms = self.overflow_from_ms.take().unwrap_or(now_ms);
            if changed {
                events.push(self.reconciled(now_ms, offset_s));
            }
            Some(GapMark {
                cause: GapCause::WatcherOverflow,
                started_ms,
                ended_ms: now_ms,
            })
        } else {
            None
        };
        if changed || head_moved || !events.is_empty() || gap.is_some() {
            self.send(
                read.clone(),
                head,
                changed,
                events,
                gap,
                Marks {
                    t_recv: window.t_recv,
                    t_flush,
                    t_computed,
                },
            );
        }
        self.last = read;
        if std::mem::take(&mut self.touched) || changed {
            self.changed();
        }
    }

    /// Tells the hooks the worktree changed, after its batch is handed over.
    fn changed(&self) {
        if let Some(hooks) = &self.shared.hooks {
            hooks.worktree_changed(&self.repo_id, &self.root);
        }
    }

    /// Full reconciliation every period (ADR-GRP-010 § 5): what it finds
    /// was missed without a mark, so it is published in a gap of its own.
    /// Not in degraded mode, which already reads the full state.
    fn periodic(&mut self) {
        let period = self.shared.config.periodic;
        self.next_periodic = Instant::now() + period;
        let (now_ms, offset_s) = wall_now();
        let from_ms = std::mem::replace(&mut self.last_periodic_ms, now_ms);
        if self.degraded {
            return;
        }
        // What the ignore cache decided may have changed with nobody noticing, e.g. a global
        // `core.excludesFile`: this is the bound (ADR-GRP-010, Enmienda 2026-10-08, E5).
        self.ignore.revalidate(&self.root);
        let t_flush = clock::monotonic_ns();
        let head = self.head();
        let read = self.read();
        let t_computed = clock::monotonic_ns();
        if read == self.last {
            return;
        }
        if !read.in_progress {
            self.stable_branch = branch_of(&read).or(self.stable_branch.take());
        }
        let event = self.reconciled(now_ms, offset_s);
        self.send(
            read.clone(),
            head,
            true,
            vec![event],
            Some(GapMark {
                cause: GapCause::PeriodicReconciliation,
                started_ms: from_ms,
                ended_ms: now_ms,
            }),
            Marks {
                t_recv: t_flush,
                t_flush,
                t_computed,
            },
        );
        self.last = read;
        self.changed();
    }

    fn reconciled(&self, observed_ms: i64, offset_s: i32) -> RawEvent {
        RawEvent {
            worktree: self.root.clone(),
            kind: GitEventKind::Reconciled,
            details: GitEventDetails::default(),
            observed_ms,
            offset_s,
        }
    }

    /// `HEAD` moved to another branch, outside any operation in progress: a
    /// rebase detaches `HEAD` without the developer switching branch.
    fn branch_switch(&mut self, read: &WorktreeRead) -> Option<GitEventDetails> {
        if read.in_progress || !matches!(read.view.status, WorktreeStatus::Ready { .. }) {
            return None;
        }
        let current = branch_of(read);
        if current == self.stable_branch {
            return None;
        }
        let from = std::mem::replace(&mut self.stable_branch, current.clone());
        let to = current?;
        Some(GitEventDetails {
            branch: Some(UntrustedName::new(to)),
            from: from.map(UntrustedName::new),
            old_commit: self.last.head_commit.clone(),
            new_commit: read.head_commit.clone(),
            worktree_inferred: false,
        })
    }

    /// `HEAD` as it is now, before the read: once the batch is persisted,
    /// the Time Machine knows the engine saw it (US-TMC-004).
    fn head(&self) -> Vec<u8> {
        std::fs::read(self.git_dir.join("HEAD")).unwrap_or_default()
    }

    fn send(
        &mut self,
        read: WorktreeRead,
        head: Vec<u8>,
        changed: bool,
        events: Vec<RawEvent>,
        gap: Option<GapMark>,
        marks: Marks,
    ) {
        self.sent_head.clone_from(&head);
        self.shared.send_from_worktree(
            &self.root,
            ObservedBatch {
                repo_id: self.repo_id.clone(),
                worktrees: if changed { vec![read] } else { Vec::new() },
                gone: Vec::new(),
                events,
                gap,
                refs: None,
                head_logs: Vec::new(),
                heads: vec![(self.root.clone(), head)],
                marks,
            },
        );
    }
}

/// The branch `HEAD` names, with or without commits.
pub(super) fn branch_of(read: &WorktreeRead) -> Option<String> {
    match &read.view.status {
        WorktreeStatus::Ready {
            head: HeadView::Branch { name } | HeadView::Unborn { name },
            ..
        } => Some(name.raw().to_owned()),
        _ => None,
    }
}

fn stagger(root: &Path, period: Duration) -> Duration {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut h);
    let millis = u64::try_from(period.as_millis()).unwrap_or(u64::MAX).max(1);
    Duration::from_millis(h.finish() % millis)
}

/// Most prefixes the router checks per path; past them it leaves the rest to the task.
const MAX_IGNORED_PREFIXES: usize = 32;

/// Events dropped under one ignored folder within [`HOT_WINDOW_NS`] that make it a candidate to
/// leave the OS stream (ADR-GRP-010, Enmienda 2026-10-08, E3). ⚠️ **ASSUMPTION**: calibrated by
/// the idle bench.
const HOT_EVENTS: u32 = 100;
const HOT_WINDOW_NS: u64 = 5_000_000_000;

/// One ignored folder the router drops events under.
#[derive(Debug)]
struct Prefix {
    /// Absolute, ending in `/`.
    bytes: Vec<u8>,
    dir: PathBuf,
    /// Events dropped in the window that began at `window_ns`.
    hits: AtomicU32,
    window_ns: AtomicU64,
    /// It reached [`HOT_EVENTS`]: sustained churn.
    hot: AtomicBool,
}

/// An ignored folder as the exclusion manager sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoredDir {
    pub dir: PathBuf,
    pub hot: bool,
    pub hits: u32,
}

/// The ignored directories a worktree task found, as absolute byte prefixes ending in `/`,
/// shared with the router so it drops their events before they reach the task (RES-01: a build
/// writes thousands of files per second under `target/`). Only directories the task asked Git
/// about, and cleared with its cache: the router never drops a path the task would keep.
///
/// It also counts what the router drops, and tells its watcher (`on_change`) when a folder
/// sustains churn or stops being ignored, so the OS stream can leave it out or take it back
/// (ADR-GRP-010, Enmienda 2026-10-08).
#[derive(Default)]
pub(crate) struct IgnoredPrefixes {
    list: RwLock<Vec<Prefix>>,
    on_change: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for IgnoredPrefixes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IgnoredPrefixes")
            .field("list", &self.list)
            .finish_non_exhaustive()
    }
}

impl IgnoredPrefixes {
    /// The router's check: whether `path` is under an ignored directory, counting it. `t_ns` is
    /// the monotonic time the event was received.
    pub(super) fn hit(&self, path: &Path, t_ns: u64) -> bool {
        let path = path.as_os_str().as_encoded_bytes();
        let list = self.list.read().unwrap_or_else(|e| e.into_inner());
        let Some(prefix) = list.iter().find(|p| path.starts_with(&p.bytes)) else {
            return false;
        };
        let began = prefix.window_ns.load(Ordering::Relaxed);
        if t_ns.saturating_sub(began) > HOT_WINDOW_NS {
            prefix.window_ns.store(t_ns, Ordering::Relaxed);
            prefix.hits.store(1, Ordering::Relaxed);
        } else if prefix.hits.fetch_add(1, Ordering::Relaxed) + 1 == HOT_EVENTS
            && !prefix.hot.swap(true, Ordering::Relaxed)
        {
            drop(list);
            self.changed();
        }
        true
    }

    /// Tells the watcher something changed. Set once, by whoever watches this worktree.
    pub(super) fn set_on_change(&self, f: Box<dyn Fn() + Send + Sync>) {
        let _ = self.on_change.set(f);
    }

    fn changed(&self) {
        if let Some(f) = self.on_change.get() {
            f();
        }
    }

    /// The folders now, with what the router counted under each.
    pub(crate) fn snapshot(&self) -> Vec<IgnoredDir> {
        self.list
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|p| IgnoredDir {
                dir: p.dir.clone(),
                hot: p.hot.load(Ordering::Relaxed),
                hits: p.hits.load(Ordering::Relaxed),
            })
            .collect()
    }

    fn insert(&self, dir: &Path) {
        let mut bytes = dir.as_os_str().as_encoded_bytes().to_vec();
        bytes.push(b'/');
        let mut all = self.list.write().unwrap_or_else(|e| e.into_inner());
        if all.len() < MAX_IGNORED_PREFIXES && !all.iter().any(|p| p.bytes == bytes) {
            all.push(Prefix {
                bytes,
                dir: dir.to_path_buf(),
                hits: AtomicU32::new(0),
                window_ns: AtomicU64::new(0),
                hot: AtomicBool::new(false),
            });
        }
    }

    /// Takes one folder out. The watcher is told, since it may have left it out of its stream.
    fn remove(&self, dir: &Path) {
        let removed = {
            let mut all = self.list.write().unwrap_or_else(|e| e.into_inner());
            let before = all.len();
            all.retain(|p| p.dir != dir);
            all.len() != before
        };
        if removed {
            self.changed();
        }
    }

    fn clear(&self) {
        let had = {
            let mut all = self.list.write().unwrap_or_else(|e| e.into_inner());
            let had = !all.is_empty();
            all.clear();
            had
        };
        if had {
            self.changed();
        }
    }
}

/// Directories ignored by Git, asked once each (ADR-GRP-010 § 2, Enmienda
/// 2026-10-05): events under them are dropped before the debounce, so a
/// build writing to an ignored `target/` costs no recompute. A directory with
/// tracked entries in the index is not ignored here: Git keeps following those
/// files (Enmienda 2026-10-08, E3).
#[derive(Default)]
pub(super) struct IgnoreCache {
    ignored: HashSet<String>,
    kept: HashSet<String>,
    /// What the router learns from it.
    prefixes: Arc<IgnoredPrefixes>,
}

impl IgnoreCache {
    pub(super) fn with_prefixes(prefixes: Arc<IgnoredPrefixes>) -> Self {
        Self {
            prefixes,
            ..Self::default()
        }
    }

    pub(super) fn clear(&mut self) {
        // The router first: from now on it hands every path over again.
        self.prefixes.clear();
        self.ignored.clear();
        self.kept.clear();
    }

    /// Checks again what the cache decided, against the rules and the index as they are now:
    /// a folder that holds a tracked entry, or that the rules no longer ignore, is dropped from
    /// the cache and from the router, and the watcher is told (E5). Called when the index
    /// changed and by the periodic reconciliation, which also covers `core.excludesFile`.
    pub(super) fn revalidate(&mut self, root: &Path) {
        if self.ignored.is_empty() {
            return;
        }
        let Some(reader) = RepoReader::open(root, &ReaderOptions::default()).ok() else {
            return;
        };
        let stale: Vec<String> = self
            .ignored
            .iter()
            .filter(|dir| {
                !reader.is_ignored(dir, true).unwrap_or(false)
                    || reader.has_tracked_under(dir).unwrap_or(true)
            })
            .cloned()
            .collect();
        for dir in stale {
            self.ignored.remove(&dir);
            // What was decided about the ones below may rest on this one.
            self.kept.clear();
            self.prefixes.remove(&root.join(&dir));
        }
    }

    /// Whether any path is outside every ignored directory.
    pub(super) fn keep_any(&mut self, root: &Path, paths: Vec<PathBuf>) -> bool {
        if paths
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == ".gitignore"))
        {
            self.clear();
            return true;
        }
        let mut reader: Option<Option<RepoReader>> = None;
        let mut keep = false;
        for path in paths {
            let Ok(rel) = path.strip_prefix(root) else {
                keep = true;
                continue;
            };
            if self.is_ignored(root, rel, &mut reader) {
                continue;
            }
            keep = true;
            break;
        }
        keep
    }

    fn is_ignored(
        &mut self,
        root: &Path,
        rel: &Path,
        reader: &mut Option<Option<RepoReader>>,
    ) -> bool {
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.first().is_some_and(|c| c == ".git") {
            return false;
        }
        // Every ancestor directory, top-down; the path itself may be a file.
        let mut dir = String::new();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(part);
            if self.ignored.contains(&dir) {
                return true;
            }
            if self.kept.contains(&dir) {
                continue;
            }
            let reader = reader
                .get_or_insert_with(|| RepoReader::open(root, &ReaderOptions::default()).ok());
            let Some(reader) = reader.as_ref() else {
                return false;
            };
            // A tracked entry below keeps the folder in sight; when the index cannot be read,
            // it is not ignored.
            if reader.is_ignored(&dir, true).unwrap_or(false)
                && !reader.has_tracked_under(&dir).unwrap_or(true)
            {
                // Only a name read without loss names the same folder for the router.
                if rel.to_str().is_some() {
                    self.prefixes.insert(&root.join(&dir));
                }
                self.ignored.insert(dir);
                return true;
            }
            self.kept.insert(dir.clone());
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn counted() -> (IgnoredPrefixes, Arc<AtomicUsize>) {
        let p = IgnoredPrefixes::default();
        p.insert(Path::new("/w/target"));
        let told = Arc::new(AtomicUsize::new(0));
        let t = Arc::clone(&told);
        p.set_on_change(Box::new(move || {
            t.fetch_add(1, Ordering::Relaxed);
        }));
        (p, told)
    }

    #[test]
    fn a_folder_becomes_hot_once_its_events_reach_the_threshold_in_the_window() {
        let (p, told) = counted();
        let path = Path::new("/w/target/debug/a");
        for i in 0..u64::from(HOT_EVENTS) - 1 {
            assert!(p.hit(path, 1_000 + i));
        }
        assert!(!p.snapshot()[0].hot);
        assert_eq!(told.load(Ordering::Relaxed), 0);
        assert!(p.hit(path, 2_000));
        assert!(p.snapshot()[0].hot);
        assert_eq!(told.load(Ordering::Relaxed), 1);
        // Told once, not for each event after it.
        for i in 0..50 {
            assert!(p.hit(path, 3_000 + i));
        }
        assert_eq!(told.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_slow_trickle_never_makes_a_folder_hot() {
        let (p, told) = counted();
        let path = Path::new("/w/target/debug/a");
        // One event per window: the count restarts each time.
        for i in 0..u64::from(HOT_EVENTS) * 2 {
            assert!(p.hit(path, i * (HOT_WINDOW_NS + 1)));
        }
        assert!(!p.snapshot()[0].hot);
        assert_eq!(told.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn paths_outside_the_folder_are_not_dropped_or_counted() {
        let (p, _) = counted();
        assert!(!p.hit(Path::new("/w/src/a.rs"), 1));
        assert!(!p.hit(Path::new("/w/target2/a"), 1));
        assert_eq!(p.snapshot()[0].hits, 0);
    }

    #[test]
    fn taking_a_folder_out_tells_the_watcher_once() {
        let (p, told) = counted();
        p.remove(Path::new("/w/other"));
        assert_eq!(told.load(Ordering::Relaxed), 0);
        p.remove(Path::new("/w/target"));
        assert_eq!(told.load(Ordering::Relaxed), 1);
        assert!(p.snapshot().is_empty());
        p.clear();
        assert_eq!(told.load(Ordering::Relaxed), 1);
    }
}
