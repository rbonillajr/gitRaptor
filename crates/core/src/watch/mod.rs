//! The change observer (US-GRP-002, ADR-GRP-010, ADR-GRP-011).
//!
//! `notify` delivers file events (FSEvents, inotify, ReadDirectoryChangesW);
//! the router sends each path to the task it belongs to, by the longest
//! watched prefix (nested worktrees, such as Claude Code's
//! `.claude/worktrees/`, get their own). There is one task per worktree
//! (its working tree, `HEAD`, index and operation markers) and one per repo
//! (refs, reflogs and `.git/worktrees/`). Each task has its own fixed
//! debounce window, recomputes with the read-only layer of `crates/git`
//! (ADR-GRP-009) and hands an [`ObservedBatch`] to the daemon loop, the only
//! writer, which persists and then publishes it (ADR-GRP-013).
//!
//! Nothing here writes to a repo, and the router never reads Git.

mod repo;
mod watchers;
mod worktree;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use gitraptor_api::messages::{GitEventDetails, GitEventKind};

use crate::observe::{RepoRead, WorktreeRead};
use crate::profile::GapCause;

pub use repo::{EventPlace, RefsView, classify};
use watchers::Watchers;

/// Intervals of the observer. The defaults are those of ADR-GRP-010; reading
/// them from the profile and local levels belongs to US-GRP-013.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchConfig {
    /// Effective debounce window (ADR-GRP-011 § 2).
    pub window: Duration,
    /// How late the OS timer wakes up; the window is scheduled this much
    /// earlier so its effective length is `window` (ADR-GRP-010 § 3).
    pub timer_slack: Duration,
    /// Light backup poll of refs, `HEAD`, index and markers (§ 5).
    pub backup_poll: Duration,
    /// Full reconciliation of every worktree (§ 5; at least 60 s outside
    /// tests).
    pub periodic: Duration,
    /// Full poll of a worktree that cannot be watched (§ 5).
    pub degraded_poll: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_millis(75),
            // SPIKE-GRP-002 measured up to 10 ms on macOS; the other systems
            // are calibrated by INF-GRP-002. Pendiente: etapa de validación
            // multiplataforma.
            timer_slack: if cfg!(target_os = "macos") {
                Duration::from_millis(10)
            } else {
                Duration::ZERO
            },
            backup_poll: Duration::from_secs(30),
            periodic: Duration::from_secs(5 * 60),
            degraded_poll: Duration::from_secs(2),
        }
    }
}

/// Monotonic marks of one window (ADR-GRP-011 § 3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub t_recv: u64,
    pub t_flush: u64,
    pub t_computed: u64,
}

/// One Git event as observed, before it has a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    /// Root of the worktree it happened in.
    pub worktree: PathBuf,
    pub kind: GitEventKind,
    pub details: GitEventDetails,
    pub observed_ms: i64,
    pub offset_s: i32,
}

/// An interval the observer did not see (ADR-GRP-013 § 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapMark {
    pub cause: GapCause,
    pub started_ms: i64,
    pub ended_ms: i64,
}

/// What one task observed in one window: for the daemon loop to persist
/// and publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedBatch {
    pub repo_id: String,
    /// New state of these worktrees.
    pub worktrees: Vec<WorktreeRead>,
    /// Worktrees no longer registered in the repo.
    pub gone: Vec<PathBuf>,
    pub events: Vec<RawEvent>,
    /// The gap the batch closes; its events are linked to it.
    pub gap: Option<GapMark>,
    /// Tips of the local branches (as in [`RepoRead::refs`]), when read.
    pub refs: Option<String>,
    pub marks: Marks,
}

/// Where batches go: the daemon loop.
pub type Sink = Arc<dyn Fn(ObservedBatch) + Send + Sync>;

/// Message to a worktree task.
#[derive(Debug)]
pub(crate) enum WtMsg {
    Paths(u64, Vec<PathBuf>),
    /// The watcher lost events (overflow): reconcile, with a gap.
    Rescan(u64),
    /// Reconcile now, without a gap (backup poll, or its `HEAD` commit moved).
    Reconcile(u64),
    /// An ignore rule file outside the worktree changed.
    IgnoreRules,
    Stop,
}

/// Message to a repo task.
#[derive(Debug)]
pub(crate) enum RepoMsg {
    Paths(u64),
    Rescan(u64),
    Stop,
}

/// A worktree task, as the router and its repo know it.
#[derive(Clone)]
pub(crate) struct WtHandle {
    pub root: PathBuf,
    /// Its private Git directory (the common one for the main worktree).
    pub git_dir: PathBuf,
    pub tx: Sender<WtMsg>,
}

/// Files of a worktree's Git directory its own task recomputes on.
const WORKTREE_GIT_FILES: &[&str] = &[
    "HEAD",
    "index",
    "MERGE_HEAD",
    "ORIG_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

struct RepoEntry {
    common: PathBuf,
    tx: Sender<RepoMsg>,
    worktrees: Vec<WtHandle>,
}

/// State shared by the router, the tasks and the [`Observer`].
pub(crate) struct Shared {
    config: WatchConfig,
    sink: Sink,
    repos: RwLock<HashMap<String, RepoEntry>>,
    watchers: Mutex<Option<Watchers>>,
    /// Worktree reads done by the tasks (diagnostics: a burst in an ignored
    /// directory must not add any).
    recomputes: std::sync::atomic::AtomicU64,
    /// Test hook: the router drops every event, as an OS that lost them
    /// without a mark.
    drop_events: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn send(&self, batch: ObservedBatch) {
        (self.sink)(batch);
    }

    /// Sends the batch of a worktree task only while the worktree is still
    /// followed. The check and the send hold the lock that stopping it
    /// takes, so a read of a removed worktree never follows the batch that
    /// says it is gone.
    fn send_from_worktree(&self, root: &Path, batch: ObservedBatch) {
        let repos = self.repos.read().unwrap_or_else(|e| e.into_inner());
        let followed = repos
            .get(&batch.repo_id)
            .is_some_and(|r| r.worktrees.iter().any(|w| w.root == root));
        if followed {
            (self.sink)(batch);
        }
    }

    /// Routes the paths of one file event. Longest watched prefix first.
    fn route(&self, t_recv: u64, paths: Vec<PathBuf>, rescan: bool) {
        let repos = self.repos.read().unwrap_or_else(|e| e.into_inner());
        if rescan {
            // The OS does not say what was lost: every task reconciles.
            for repo in repos.values() {
                let _ = repo.tx.send(RepoMsg::Rescan(t_recv));
                for wt in &repo.worktrees {
                    let _ = wt.tx.send(WtMsg::Rescan(t_recv));
                }
            }
            return;
        }
        let mut per_wt: HashMap<PathBuf, (Sender<WtMsg>, Vec<PathBuf>)> = HashMap::new();
        let mut repo_hit: Vec<&Sender<RepoMsg>> = Vec::new();
        let mut ignore_rules: Vec<&RepoEntry> = Vec::new();
        for path in paths {
            let mut best: Option<(usize, Target)> = None;
            for repo in repos.values() {
                if path.starts_with(&repo.common) {
                    let len = repo.common.as_os_str().len();
                    if best.as_ref().is_none_or(|(l, _)| len > *l) {
                        best = Some((len, Target::Common(repo)));
                    }
                }
                for wt in &repo.worktrees {
                    if path.starts_with(&wt.root) {
                        let len = wt.root.as_os_str().len();
                        if best.as_ref().is_none_or(|(l, _)| len > *l) {
                            best = Some((len, Target::Worktree(wt)));
                        }
                    }
                }
            }
            match best {
                None => {}
                Some((_, Target::Worktree(wt))) => {
                    per_wt
                        .entry(wt.root.clone())
                        .or_insert_with(|| (wt.tx.clone(), Vec::new()))
                        .1
                        .push(path);
                }
                Some((_, Target::Common(repo))) => {
                    let Ok(rel) = path.strip_prefix(&repo.common) else {
                        continue;
                    };
                    if rel.starts_with("objects") {
                        continue;
                    }
                    if rel.starts_with("info/exclude") {
                        ignore_rules.push(repo);
                    }
                    // The worktree whose Git directory holds the file.
                    if let Some(wt) = repo
                        .worktrees
                        .iter()
                        .filter(|w| path.starts_with(&w.git_dir))
                        .max_by_key(|w| w.git_dir.as_os_str().len())
                        && let Ok(own) = path.strip_prefix(&wt.git_dir)
                        && own
                            .components()
                            .next()
                            .is_some_and(|c| WORKTREE_GIT_FILES.iter().any(|f| c.as_os_str() == *f))
                    {
                        per_wt
                            .entry(wt.root.clone())
                            .or_insert_with(|| (wt.tx.clone(), Vec::new()))
                            .1
                            .push(path.clone());
                    }
                    let index_only = path.file_name().is_some_and(|n| n == "index");
                    if !index_only && !repo_hit.iter().any(|t| std::ptr::eq(*t, &repo.tx)) {
                        repo_hit.push(&repo.tx);
                    }
                }
            }
        }
        for (_, (tx, paths)) in per_wt {
            let _ = tx.send(WtMsg::Paths(t_recv, paths));
        }
        for tx in repo_hit {
            let _ = tx.send(RepoMsg::Paths(t_recv));
        }
        for repo in ignore_rules {
            for wt in &repo.worktrees {
                let _ = wt.tx.send(WtMsg::IgnoreRules);
            }
        }
    }

    /// Starts the task of one worktree and its watch. A worktree that cannot
    /// be watched is polled instead (degraded mode).
    fn start_worktree(self: &Arc<Self>, repo_id: &str, initial: WorktreeRead, git_dir: PathBuf) {
        let root = PathBuf::from(initial.view.path.raw());
        let (tx, rx) = channel();
        let handle = WtHandle {
            root: root.clone(),
            git_dir,
            tx,
        };
        {
            let mut repos = self.repos.write().unwrap_or_else(|e| e.into_inner());
            let Some(repo) = repos.get_mut(repo_id) else {
                return;
            };
            if repo.worktrees.iter().any(|w| w.root == root) {
                return;
            }
            repo.worktrees.push(handle);
        }
        let degraded = !self.watch(std::slice::from_ref(&root));
        let shared = Arc::clone(self);
        let repo_id = repo_id.to_owned();
        let _ = std::thread::Builder::new()
            .name("raptor-watch-worktree".into())
            .spawn(move || worktree::run(shared, repo_id, initial, degraded, rx));
    }

    fn stop_worktree(&self, repo_id: &str, root: &Path) {
        let removed = {
            let mut repos = self.repos.write().unwrap_or_else(|e| e.into_inner());
            repos.get_mut(repo_id).and_then(|repo| {
                let pos = repo.worktrees.iter().position(|w| w.root == root)?;
                Some(repo.worktrees.remove(pos))
            })
        };
        if let Some(wt) = removed {
            let _ = wt.tx.send(WtMsg::Stop);
            self.unwatch(&[wt.root]);
        }
    }

    /// Adds watches; `false` if any failed.
    fn watch(&self, roots: &[PathBuf]) -> bool {
        let mut guard = self.watchers.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(w) => w.add(roots),
            None => false,
        }
    }

    fn unwatch(&self, roots: &[PathBuf]) {
        let mut guard = self.watchers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = guard.as_mut() {
            w.remove(roots);
        }
    }

    fn worktrees_of(&self, repo_id: &str) -> Vec<WtHandle> {
        let repos = self.repos.read().unwrap_or_else(|e| e.into_inner());
        repos
            .get(repo_id)
            .map(|r| r.worktrees.clone())
            .unwrap_or_default()
    }
}

enum Target<'a> {
    Common(&'a RepoEntry),
    Worktree(&'a WtHandle),
}

/// The running observer, owned by the daemon.
pub struct Observer {
    shared: Arc<Shared>,
}

impl Observer {
    /// Starts the file watcher. Batches go to `sink`.
    pub fn start(config: WatchConfig, sink: Sink) -> Self {
        Self::start_counted(config, sink, Arc::default())
    }

    /// Like [`Observer::start`], keeping in `roots` how many roots are
    /// watched (`engine.resources`, US-GRP-017).
    pub fn start_counted(
        config: WatchConfig,
        sink: Sink,
        roots: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        let shared = Arc::new(Shared {
            config,
            sink,
            repos: RwLock::new(HashMap::new()),
            watchers: Mutex::new(None),
            recomputes: std::sync::atomic::AtomicU64::new(0),
            drop_events: std::sync::atomic::AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&shared);
        let watchers = Watchers::new(
            Arc::new(move |result: notify::Result<notify::Event>| {
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                let t_recv = gitraptor_api::clock::monotonic_ns();
                if shared
                    .drop_events
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return;
                }
                match result {
                    Ok(event) => {
                        let rescan = event.need_rescan();
                        if !rescan && !is_change(&event.kind) {
                            return;
                        }
                        shared.route(t_recv, event.paths, rescan);
                    }
                    // An error of the watcher itself: reconcile everything.
                    Err(_) => shared.route(t_recv, Vec::new(), true),
                }
            }),
            roots,
        );
        *shared.watchers.lock().unwrap_or_else(|e| e.into_inner()) = Some(watchers);
        Self { shared }
    }

    /// Starts observing a repo from the reconciliation `read` of its common
    /// directory. Each worktree task re-reads once its watch is running, so
    /// nothing written between `read` and the watch is lost (ADR-GRP-010
    /// § 6).
    pub fn watch_repo(&self, repo_id: &str, common_dir: &Path, read: &RepoRead) {
        let common = crate::observe::canonical(common_dir);
        let (tx, rx) = channel();
        {
            let mut repos = self.shared.repos.write().unwrap_or_else(|e| e.into_inner());
            if repos.contains_key(repo_id) {
                return;
            }
            repos.insert(
                repo_id.to_owned(),
                RepoEntry {
                    common: common.clone(),
                    tx,
                    worktrees: Vec::new(),
                },
            );
        }
        // The common directory has a watch of its own unless a worktree
        // root already holds it (the main worktree's `.git`).
        let roots: Vec<PathBuf> = read
            .worktrees
            .iter()
            .map(|w| PathBuf::from(w.view.path.raw()))
            .collect();
        if !roots.iter().any(|r| common.starts_with(r)) {
            self.shared.watch(std::slice::from_ref(&common));
        }
        let refs = RefsView::read(&common);
        for w in &read.worktrees {
            if !w.view.status.is_watchable() {
                continue;
            }
            let git_dir = worktree_git_dir(&common, w);
            self.shared.start_worktree(repo_id, w.clone(), git_dir);
        }
        let shared = Arc::clone(&self.shared);
        let repo_id = repo_id.to_owned();
        let _ = std::thread::Builder::new()
            .name("raptor-watch-repo".into())
            .spawn(move || repo::run(shared, repo_id, common, refs, rx));
    }

    /// Stops observing a repo: its watches close and its tasks end. A batch
    /// already on its way is discarded by the daemon.
    pub fn forget_repo(&self, repo_id: &str) {
        let entry = self
            .shared
            .repos
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(repo_id);
        if let Some(entry) = entry {
            let _ = entry.tx.send(RepoMsg::Stop);
            let mut roots = vec![entry.common];
            for wt in entry.worktrees {
                let _ = wt.tx.send(WtMsg::Stop);
                roots.push(wt.root);
            }
            self.shared.unwatch(&roots);
        }
    }

    /// Test hook: lose every file event, without a mark, while `lost`.
    #[doc(hidden)]
    pub fn simulate_lost_events(&self, lost: bool) {
        self.shared
            .drop_events
            .store(lost, std::sync::atomic::Ordering::Relaxed);
    }

    /// Test hook: the watcher reports an overflow (events lost with a
    /// mark), as `IN_Q_OVERFLOW` or `MustScanSubDirs` would.
    #[doc(hidden)]
    pub fn simulate_overflow(&self) {
        self.shared
            .route(gitraptor_api::clock::monotonic_ns(), Vec::new(), true);
    }

    /// Worktree reads done so far by every task.
    pub fn recomputes(&self) -> u64 {
        self.shared
            .recomputes
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Worktree roots observed for a repo (diagnostics and tests).
    pub fn worktrees(&self, repo_id: &str) -> Vec<PathBuf> {
        self.shared
            .worktrees_of(repo_id)
            .into_iter()
            .map(|w| w.root)
            .collect()
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let ids: Vec<String> = self
            .shared
            .repos
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.forget_repo(&id);
        }
        // Dropping the watchers ends the OS streams.
        self.shared
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

/// Whether an event can mean a change. inotify also reports opens (its
/// mask includes `IN_OPEN` in `notify` 8.2): the engine's own reads would
/// wake the tasks that made them, in a loop. Only a close after writing is
/// an access that changes something.
fn is_change(kind: &notify::EventKind) -> bool {
    use notify::event::{AccessKind, AccessMode};
    match kind {
        notify::EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        notify::EventKind::Access(_) => false,
        _ => true,
    }
}

/// The private Git directory of a worktree.
fn worktree_git_dir(common: &Path, read: &WorktreeRead) -> PathBuf {
    match &read.view.admin_name {
        Some(name) if !read.view.main => common.join("worktrees").join(name.raw()),
        _ => common.to_path_buf(),
    }
}

/// Wall clock now, in UTC milliseconds, and the local offset.
pub(crate) fn wall_now() -> (i64, i32) {
    (crate::daemon::now_ms(), gitraptor_git::local_utc_offset_s())
}

trait Watchable {
    fn is_watchable(&self) -> bool;
}

impl Watchable for gitraptor_api::messages::WorktreeStatus {
    /// Unavailable worktrees are not watched: a missing one has nothing to
    /// watch and an untrusted link must never be (SEC-11).
    fn is_watchable(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};

    /// Reads never wake a task; writes and their close do.
    #[test]
    fn only_changes_are_routed() {
        assert!(!is_change(&EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(!is_change(&EventKind::Access(AccessKind::Read)));
        assert!(is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_change(&EventKind::Create(CreateKind::File)));
        assert!(is_change(&EventKind::Modify(ModifyKind::Any)));
        assert!(is_change(&EventKind::Any));
    }
}
