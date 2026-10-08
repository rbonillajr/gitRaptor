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
mod sweep;
mod watchers;
mod worktree;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventDetails, GitEventKind};

use crate::observe::{RepoRead, WorktreeRead};
use crate::profile::GapCause;

pub use repo::{EventPlace, RefsView, classify};
pub use sweep::Print;
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
    /// Metadata sweep of the dormant repos (Enmienda 2026-10-07, N3).
    pub dormant_poll: Duration,
    /// Shortest interval of the slow reconciliation of a dormant repo (N3);
    /// the CPU budget may make it longer.
    pub dormant_reconcile: Duration,
    /// Average share of one core the slow reconciliation may use, in parts
    /// per million (RES-11: 0.1 %, ⚠️ ASSUMPTION of the Enmienda).
    pub reconcile_budget_ppm: u32,
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
            dormant_poll: Duration::from_secs(120),
            dormant_reconcile: Duration::from_secs(60 * 60),
            reconcile_budget_ppm: 1_000,
        }
    }
}

/// The observation tier of a repo (ADR-GRP-010, Enmienda 2026-10-07, N1).
/// It is per repo, not per worktree: they share the common `.git`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Everything of § 1 to § 6 runs.
    Active,
    /// A trigger fired and the repo is being reconciled.
    Waking,
    /// Only the sentinel and the safety nets run.
    Dormant,
}

impl Tier {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Waking,
            2 => Self::Dormant,
            _ => Self::Active,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Active => 0,
            Self::Waking => 1,
            Self::Dormant => 2,
        }
    }
}

/// What woke a dormant repo (N4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeCause {
    /// Its watches saw a change that passed the ignore filter (N2): not a
    /// gap, the repo never stopped being watched (N5).
    Sentinel,
    /// The metadata sweep found a change the sentinel did not signal: a
    /// `dormant` gap from the previous check until now (N5).
    SafetyNet { since_ms: i64 },
}

/// Why a repo could not go dormant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepRefused {
    /// Not observed.
    Unknown,
    /// Already dormant or waking.
    NotActive,
    /// A worktree in degraded mode has no sentinel (N1).
    Degraded,
    /// A task did not hand its window over in time.
    Busy,
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
    /// Size of each worktree's `HEAD` reflog as the repo task read it, when
    /// it read them: once persisted, the Time Machine knows the engine saw
    /// every `git` that wrote there (US-TMC-004, calm by state).
    pub head_logs: Vec<(PathBuf, u64)>,
    /// `HEAD` of each worktree as its task read it before computing the
    /// batch (US-TMC-004, calm by state: its events are persisted with it).
    pub heads: Vec<(PathBuf, Vec<u8>)>,
    pub marks: Marks,
}

impl ObservedBatch {
    /// The worktrees whose state this batch read while closing a gap: what
    /// changed in them happened at some point inside it (ADR-GRP-013 § 6).
    pub fn gap_worktrees(&self) -> Vec<String> {
        if self.gap.is_none() {
            return Vec::new();
        }
        self.worktrees
            .iter()
            .map(|r| r.view.path.raw().to_owned())
            .collect()
    }
}

/// What a repo's tasks start from: each worktree's `HEAD` reflog size and
/// `HEAD` (US-TMC-004).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchStart {
    pub head_logs: Vec<(PathBuf, u64)>,
    pub heads: Vec<(PathBuf, Vec<u8>)>,
}

/// Where batches go: the daemon loop.
pub type Sink = Arc<dyn Fn(ObservedBatch) + Send + Sync>;

/// What the session detector learns from the router, as soon as a file
/// event arrives and before any debounce (US-GRP-007, ADR-GRP-012).
pub trait ObserverHooks: Send + Sync {
    /// A file under the root of a worktree changed: activity of its
    /// sessions (BR-WF-001). The worktree's own Git files (`index`,
    /// `HEAD`) do not count: an editor's `git status` rewrites them.
    fn worktree_touched(&self, repo_id: &str, root: &Path);
    /// A file of the repo's Git directory changed, `objects/` included: the
    /// first write of a commit. `t_recv` is the monotonic mark of the
    /// event, comparable with the batch's [`Marks`].
    fn git_dir_touched(&self, repo_id: &str, t_recv: u64);
    /// A worktree's window closed with files outside every ignored folder,
    /// or with a new state: after its batch is handed over, outside the
    /// budget (US-TMC-004, the Time Machine's continuous capture).
    fn worktree_changed(&self, _repo_id: &str, _root: &Path) {}
    /// A dormant repo must wake (N2, N3). Called once per sleep, from the
    /// sentinel or the sweep task; the daemon reconciles the repo and calls
    /// [`Observer::wake_repo`].
    fn repo_wake(&self, _repo_id: &str, _cause: WakeCause) {}
}

/// Several hooks, called in order.
pub struct FanoutHooks(pub Vec<Arc<dyn ObserverHooks>>);

impl ObserverHooks for FanoutHooks {
    fn worktree_touched(&self, repo_id: &str, root: &Path) {
        for h in &self.0 {
            h.worktree_touched(repo_id, root);
        }
    }

    fn git_dir_touched(&self, repo_id: &str, t_recv: u64) {
        for h in &self.0 {
            h.git_dir_touched(repo_id, t_recv);
        }
    }

    fn worktree_changed(&self, repo_id: &str, root: &Path) {
        for h in &self.0 {
            h.worktree_changed(repo_id, root);
        }
    }

    fn repo_wake(&self, repo_id: &str, cause: WakeCause) {
        for h in &self.0 {
            h.repo_wake(repo_id, cause);
        }
    }
}

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
    /// Flush the window, answer and end (the repo goes dormant).
    Sleep(Sender<Slept>),
    Stop,
}

/// What a worktree task leaves when its repo goes dormant: enough for the
/// slow reconciliation to read it again and compare (N3).
#[derive(Debug, Clone)]
pub(crate) struct Slept {
    pub root: PathBuf,
    pub main: bool,
    pub admin: Option<String>,
    /// Digest of its full status as last published.
    pub fingerprint: Option<String>,
}

/// Message to a repo task.
#[derive(Debug)]
pub(crate) enum RepoMsg {
    Paths(u64),
    Rescan(u64),
    /// Flush the window, answer with the view and end (going dormant).
    Sleep(Sender<RefsView>),
    Stop,
}

/// A worktree task, as the router and its repo know it.
#[derive(Clone)]
pub(crate) struct WtHandle {
    pub root: PathBuf,
    /// Its private Git directory (the common one for the main worktree).
    pub git_dir: PathBuf,
    pub tx: Sender<WtMsg>,
    /// Polled instead of watched: it has no sentinel (N1).
    pub degraded: bool,
    /// Ignored folders its task found: the router drops their events.
    pub ignored: Arc<worktree::IgnoredPrefixes>,
}

/// Whether `path` is `root` or under it: [`Path::starts_with`] on the bytes, which the router
/// runs for every path of every event against every root (RES-01, build churn).
fn under(path: &Path, root: &Path) -> bool {
    let (p, r) = (
        path.as_os_str().as_encoded_bytes(),
        root.as_os_str().as_encoded_bytes(),
    );
    // On Windows the separator is `\`: with only `/` no event path was ever under a root and
    // the observer saw nothing but its periodic reconciliation. On Unix `\` is a file name
    // character, so it never separates.
    let separator = |b: u8| b == b'/' || (cfg!(windows) && b == b'\\');
    p.starts_with(r)
        && (p.len() == r.len() || r.last().is_some_and(|b| separator(*b)) || separator(p[r.len()]))
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
    id: String,
    common: PathBuf,
    tx: Sender<RepoMsg>,
    worktrees: Vec<WtHandle>,
    /// [`Tier`], changed with a compare-and-set under the read lock so a
    /// burst wakes the repo once (N2).
    tier: AtomicU8,
    /// What a dormant repo keeps; `None` while active.
    asleep: Option<Asleep>,
}

impl RepoEntry {
    fn tier(&self) -> Tier {
        Tier::from_u8(self.tier.load(Ordering::Acquire))
    }

    /// Dormant to waking, once.
    fn start_waking(&self) -> bool {
        self.tier
            .compare_exchange(
                Tier::Dormant.as_u8(),
                Tier::Waking.as_u8(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

/// What a dormant repo keeps (N1): its watches stay registered, its tasks
/// are gone.
struct Asleep {
    /// The refs view of its last batch: the "processed up to" mark the wake
    /// classifies from, so commits made while dormant are one event each.
    refs: RefsView,
    /// Root of each worktree: where the sentinel applies the ignore filter.
    roots: Vec<PathBuf>,
    /// Private Git directory of each worktree, for the sweep.
    git_dirs: Vec<PathBuf>,
    /// Metadata print as of the last check (N3).
    print: Print,
    /// Wall time of the last check: where a `dormant` gap starts.
    checked_ms: i64,
    /// Each worktree as its task left it, for the slow reconciliation.
    slept: Vec<Slept>,
    /// When the slow reconciliation last read it (or it went dormant).
    reconciled: Instant,
    /// The same in wall time: where a gap the slow reconciliation finds
    /// starts.
    reconciled_ms: i64,
}

/// Paths of a dormant repo, for the sentinel task.
struct SentinelMsg {
    repo_id: String,
    root: Option<PathBuf>,
    paths: Vec<PathBuf>,
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
    /// Test hook: new watches fail, as when the OS runs out of them.
    fail_watches: std::sync::atomic::AtomicBool,
    hooks: Option<Arc<dyn ObserverHooks>>,
    /// The sentinel task of the dormant repos (N2).
    sentinel: Mutex<Option<Sender<SentinelMsg>>>,
    /// Ends the sweep task when dropped.
    sweep_stop: Mutex<Option<Sender<()>>>,
    /// Sweep cycles run (diagnostics and tests).
    sweeps: std::sync::atomic::AtomicU64,
    /// Messages the sentinel task decided on (tests).
    sentinel_seen: std::sync::atomic::AtomicU64,
    /// CPU budget of the slow reconciliation (N3).
    budget: Mutex<sweep::Budget>,
    /// Wall time spent in the safety nets of the dormant repos, and since
    /// when (`engine.resources`).
    net_ns: std::sync::atomic::AtomicU64,
    started: Instant,
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
            // The OS does not say what was lost: every task reconciles, and
            // every dormant repo wakes with a gap from its last check.
            for repo in repos.values() {
                if let Some(asleep) = &repo.asleep
                    && repo.start_waking()
                {
                    self.wake(
                        &repo.id,
                        WakeCause::SafetyNet {
                            since_ms: asleep.checked_ms,
                        },
                    );
                    continue;
                }
                let _ = repo.tx.send(RepoMsg::Rescan(t_recv));
                for wt in &repo.worktrees {
                    let _ = wt.tx.send(WtMsg::Rescan(t_recv));
                }
            }
            return;
        }
        let paths = self.route_dormant(&repos, paths);
        let mut per_wt: HashMap<PathBuf, (Sender<WtMsg>, Vec<PathBuf>)> = HashMap::new();
        let mut repo_hit: Vec<&Sender<RepoMsg>> = Vec::new();
        let mut ignore_rules: Vec<&RepoEntry> = Vec::new();
        let mut touched: Vec<PathBuf> = Vec::new();
        let mut probed: Vec<String> = Vec::new();
        for path in paths {
            let mut best: Option<(usize, Target)> = None;
            for repo in repos.values().filter(|r| r.tier() == Tier::Active) {
                if under(&path, &repo.common) {
                    let len = repo.common.as_os_str().len();
                    if best.as_ref().is_none_or(|(l, _)| len > *l) {
                        best = Some((len, Target::Common(repo)));
                    }
                }
                for wt in &repo.worktrees {
                    if under(&path, &wt.root) {
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
                    if let Some(hooks) = &self.hooks
                        && !touched.contains(&wt.root)
                    {
                        touched.push(wt.root.clone());
                        if let Some(repo) = repos
                            .values()
                            .find(|r| r.worktrees.iter().any(|w| w.root == wt.root))
                        {
                            hooks.worktree_touched(&repo.id, &wt.root);
                        }
                    }
                    // Under a folder its task already found ignored: dropped here, not one by
                    // one in the task. A `.gitignore` always reaches it (it clears the cache).
                    if path.file_name().is_none_or(|n| n != ".gitignore")
                        && wt.ignored.covers(&path)
                    {
                        continue;
                    }
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
                    // Before the `objects/` filter: a commit writes its
                    // objects first (S3 samples as early as possible).
                    if let Some(hooks) = &self.hooks
                        && !probed.contains(&repo.id)
                    {
                        probed.push(repo.id.clone());
                        hooks.git_dir_touched(&repo.id, t_recv);
                    }
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
                        .filter(|w| under(&path, &w.git_dir))
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

    /// Takes the paths of dormant and waking repos out of an event (N2).
    /// A dormant repo's worktree paths go to the sentinel task, which
    /// filters them; a write to its Git directory, `objects/` aside, wakes
    /// it here. A waking repo drops them: its tasks read everything once
    /// they are registered (N4).
    fn route_dormant(
        &self,
        repos: &HashMap<String, RepoEntry>,
        paths: Vec<PathBuf>,
    ) -> Vec<PathBuf> {
        if repos.values().all(|r| r.tier() == Tier::Active) {
            return paths;
        }
        let mut keep = Vec::with_capacity(paths.len());
        let mut sentinel: HashMap<(String, Option<PathBuf>), Vec<PathBuf>> = HashMap::new();
        for path in paths {
            // The repo of the longest prefix, active ones included.
            let mut best: Option<(usize, &RepoEntry, Option<&PathBuf>)> = None;
            for repo in repos.values() {
                let dormant_roots = repo.asleep.as_ref().map(|a| a.roots.as_slice());
                let roots = repo
                    .worktrees
                    .iter()
                    .map(|w| &w.root)
                    .chain(dormant_roots.unwrap_or_default());
                for root in roots {
                    if path.starts_with(root) {
                        let len = root.as_os_str().len();
                        if best.as_ref().is_none_or(|(l, _, _)| len > *l) {
                            best = Some((len, repo, Some(root)));
                        }
                    }
                }
                if path.starts_with(&repo.common) {
                    let len = repo.common.as_os_str().len();
                    if best.as_ref().is_none_or(|(l, _, _)| len > *l) {
                        best = Some((len, repo, None));
                    }
                }
            }
            match best {
                Some((_, repo, root)) if repo.tier() != Tier::Active => {
                    if repo.tier() != Tier::Dormant {
                        continue;
                    }
                    match root {
                        Some(root) => sentinel
                            .entry((repo.id.clone(), Some(root.clone())))
                            .or_default()
                            .push(path),
                        None => {
                            let objects = path
                                .strip_prefix(&repo.common)
                                .is_ok_and(|rel| rel.starts_with("objects"));
                            if !objects && repo.start_waking() {
                                self.wake(&repo.id, WakeCause::Sentinel);
                            }
                        }
                    }
                }
                _ => keep.push(path),
            }
        }
        if !sentinel.is_empty() {
            let tx = self.sentinel.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(tx) = tx.as_ref() {
                for ((repo_id, root), paths) in sentinel {
                    let _ = tx.send(SentinelMsg {
                        repo_id,
                        root,
                        paths,
                    });
                }
            }
        }
        keep
    }

    fn wake(&self, repo_id: &str, cause: WakeCause) {
        if let Some(hooks) = &self.hooks {
            hooks.repo_wake(repo_id, cause);
        }
    }

    /// Starts the task of one worktree and its watch. A worktree that cannot
    /// be watched is polled instead (degraded mode).
    /// Returns the worktree's `HEAD` as the task starts from it.
    fn start_worktree(
        self: &Arc<Self>,
        repo_id: &str,
        initial: WorktreeRead,
        git_dir: PathBuf,
    ) -> Option<(PathBuf, Vec<u8>)> {
        let root = PathBuf::from(initial.view.path.raw());
        let (tx, rx) = channel();
        // A woken repo's roots are still watched: adding them is a no-op.
        let degraded = !self.watch(std::slice::from_ref(&root));
        let ignored = Arc::new(worktree::IgnoredPrefixes::default());
        let handle = WtHandle {
            root: root.clone(),
            git_dir: git_dir.clone(),
            tx,
            degraded,
            ignored: Arc::clone(&ignored),
        };
        {
            let mut repos = self.repos.write().unwrap_or_else(|e| e.into_inner());
            let repo = repos.get_mut(repo_id)?;
            if repo.worktrees.iter().any(|w| w.root == root) {
                return None;
            }
            repo.worktrees.push(handle);
        }
        let head = std::fs::read(git_dir.join("HEAD")).unwrap_or_default();
        let shared = Arc::clone(self);
        let repo_id = repo_id.to_owned();
        let sent = head.clone();
        let _ = std::thread::Builder::new()
            .name("raptor-watch-worktree".into())
            .spawn(move || {
                worktree::run(
                    shared, repo_id, initial, git_dir, sent, degraded, ignored, rx,
                )
            });
        Some((root, head))
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
        if self.fail_watches.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
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
        Self::start_with_hooks(config, sink, None)
    }

    /// Like [`Observer::start`], and the router also tells `hooks` about
    /// activity and Git directory writes (US-GRP-007).
    pub fn start_with_hooks(
        config: WatchConfig,
        sink: Sink,
        hooks: Option<Arc<dyn ObserverHooks>>,
    ) -> Self {
        Self::start_counted(config, sink, hooks, Arc::default())
    }

    /// Like [`Observer::start_with_hooks`], keeping in `roots` how many
    /// roots are watched (`engine.resources`, US-GRP-017).
    pub fn start_counted(
        config: WatchConfig,
        sink: Sink,
        hooks: Option<Arc<dyn ObserverHooks>>,
        roots: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        let shared = Arc::new(Shared {
            config,
            sink,
            repos: RwLock::new(HashMap::new()),
            watchers: Mutex::new(None),
            recomputes: std::sync::atomic::AtomicU64::new(0),
            drop_events: std::sync::atomic::AtomicBool::new(false),
            fail_watches: std::sync::atomic::AtomicBool::new(false),
            hooks,
            sentinel: Mutex::new(None),
            sweep_stop: Mutex::new(None),
            sweeps: std::sync::atomic::AtomicU64::new(0),
            sentinel_seen: std::sync::atomic::AtomicU64::new(0),
            net_ns: std::sync::atomic::AtomicU64::new(0),
            started: Instant::now(),
            budget: Mutex::new(sweep::Budget::new(
                config.dormant_reconcile,
                config.reconcile_budget_ppm,
            )),
        });
        start_sentinel(&shared);
        start_sweep(&shared);
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
    /// Returns what its tasks start from (empty if the repo was already
    /// watched).
    pub fn watch_repo(&self, repo_id: &str, common_dir: &Path, read: &RepoRead) -> WatchStart {
        let common = crate::observe::canonical(common_dir);
        let (tx, rx) = channel();
        {
            let mut repos = self.shared.repos.write().unwrap_or_else(|e| e.into_inner());
            if repos.contains_key(repo_id) {
                return WatchStart::default();
            }
            repos.insert(
                repo_id.to_owned(),
                RepoEntry {
                    id: repo_id.to_owned(),
                    common: common.clone(),
                    tx,
                    worktrees: Vec::new(),
                    tier: AtomicU8::new(Tier::Active.as_u8()),
                    asleep: None,
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
        let head_logs = refs.head_logs();
        let mut heads = Vec::new();
        for w in &read.worktrees {
            if !w.view.status.is_watchable() {
                continue;
            }
            let git_dir = worktree_git_dir(&common, w);
            heads.extend(self.shared.start_worktree(repo_id, w.clone(), git_dir));
        }
        let shared = Arc::clone(&self.shared);
        let repo_id = repo_id.to_owned();
        let _ = std::thread::Builder::new()
            .name("raptor-watch-repo".into())
            .spawn(move || repo::run(shared, repo_id, common, refs, rx));
        WatchStart { head_logs, heads }
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
            roots.extend(entry.asleep.into_iter().flat_map(|a| a.roots));
            for wt in entry.worktrees {
                let _ = wt.tx.send(WtMsg::Stop);
                roots.push(wt.root);
            }
            self.shared.unwatch(&roots);
        }
    }

    /// The tier of an observed repo.
    pub fn tier(&self, repo_id: &str) -> Option<Tier> {
        let repos = self.shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos.get(repo_id).map(RepoEntry::tier)
    }

    /// Puts an active repo to sleep (ADR-GRP-010, Enmienda 2026-10-07, N1):
    /// its tasks hand their windows over and end, its watches stay as the
    /// sentinel, and it keeps only its refs view and its metadata print.
    /// The caller has already checked the threshold, sessions and clients;
    /// a repo with a degraded worktree is refused here.
    pub fn sleep_repo(&self, repo_id: &str) -> Result<(), SleepRefused> {
        // From now on its events go to the sentinel: a change that arrives
        // while the tasks flush wakes it again, it is never lost. The tier
        // changes under the write lock: a `route` in progress (read lock)
        // queues its paths to the tasks before their `Sleep`, and the next
        // one sees the repo dormant.
        let began_ms = wall_now().0;
        let (repo_tx, worktrees, print) = {
            let repos = self.shared.repos.write().unwrap_or_else(|e| e.into_inner());
            let repo = repos.get(repo_id).ok_or(SleepRefused::Unknown)?;
            if repo.tier() != Tier::Active {
                return Err(SleepRefused::NotActive);
            }
            if repo.worktrees.iter().any(|w| w.degraded) {
                return Err(SleepRefused::Degraded);
            }
            // The sweep's baseline is read before the switch, so it sees
            // anything written from now on.
            let git_dirs: Vec<PathBuf> = repo.worktrees.iter().map(|w| w.git_dir.clone()).collect();
            let print = Print::read(&repo.common, &git_dirs);
            repo.tier.store(Tier::Dormant.as_u8(), Ordering::Release);
            (repo.tx.clone(), repo.worktrees.clone(), print)
        };
        // The worktree tasks flush while still followed, so their last
        // batch is not discarded; then the repo task gives its view.
        let mut acks = Vec::new();
        for wt in &worktrees {
            let (done, ack) = channel();
            if wt.tx.send(WtMsg::Sleep(done)).is_ok() {
                acks.push(ack);
            }
        }
        let wait = Duration::from_secs(10);
        let slept: Vec<Slept> = acks
            .iter()
            .filter_map(|ack| ack.recv_timeout(wait).ok())
            .collect();
        let flushed = slept.len() == acks.len();
        let (done, ack) = channel();
        let refs = if repo_tx.send(RepoMsg::Sleep(done)).is_ok() {
            ack.recv_timeout(wait).ok()
        } else {
            None
        };
        let mut repos = self.shared.repos.write().unwrap_or_else(|e| e.into_inner());
        let Some(repo) = repos.get_mut(repo_id) else {
            return Err(SleepRefused::Unknown);
        };
        let roots: Vec<PathBuf> = repo.worktrees.drain(..).map(|w| w.root).collect();
        let git_dirs: Vec<PathBuf> = worktrees.iter().map(|w| w.git_dir.clone()).collect();
        let now = wall_now().0;
        let busy = refs.is_none() || !flushed;
        // A task that did not answer may have lost its window: the refs are
        // read now and the repo wakes at once, with a gap that covers it.
        let refs = match refs.filter(|_| flushed) {
            Some(refs) => refs,
            None => RefsView::read(&repo.common),
        };
        repo.asleep = Some(Asleep {
            refs,
            roots,
            git_dirs,
            print,
            checked_ms: now,
            slept,
            reconciled: Instant::now(),
            reconciled_ms: now,
        });
        if busy {
            for wt in &worktrees {
                let _ = wt.tx.send(WtMsg::Stop);
            }
            let _ = repo_tx.send(RepoMsg::Stop);
            let woke = repo.start_waking();
            drop(repos);
            // The window a task did not hand over started at most one
            // window before the sleep began.
            let since_ms =
                began_ms - i64::try_from(self.shared.config.window.as_millis()).unwrap_or(0);
            if woke {
                self.shared.wake(repo_id, WakeCause::SafetyNet { since_ms });
            }
            return Err(SleepRefused::Busy);
        }
        Ok(())
    }

    /// Wakes a dormant or waking repo from the reconciliation `read` of its
    /// common directory (N4). The repo is active before its tasks are
    /// registered, so no event is dropped: those before a task exists are
    /// in the read each task makes once it runs. The reconciled worktrees
    /// are handed over as one batch, with a `dormant` gap when a safety net
    /// woke it (N5), and the repo task classifies from the refs view it
    /// slept with, one event per reflog entry. Returns what its tasks start
    /// from, as [`Observer::watch_repo`] does.
    pub fn wake_repo(&self, repo_id: &str, read: &RepoRead, cause: WakeCause) -> WatchStart {
        let (asleep, common) = {
            let mut repos = self.shared.repos.write().unwrap_or_else(|e| e.into_inner());
            let Some(repo) = repos.get_mut(repo_id) else {
                return WatchStart::default();
            };
            let Some(asleep) = repo.asleep.take() else {
                return WatchStart::default();
            };
            repo.tier.store(Tier::Active.as_u8(), Ordering::Release);
            (asleep, repo.common.clone())
        };
        let (now_ms, offset_s) = wall_now();
        let t = gitraptor_api::clock::monotonic_ns();
        let gap = match cause {
            WakeCause::Sentinel => None,
            WakeCause::SafetyNet { since_ms } => Some(GapMark {
                cause: GapCause::Dormant,
                started_ms: since_ms.min(now_ms),
                ended_ms: now_ms,
            }),
        };
        let ready: Vec<WorktreeRead> = read
            .worktrees
            .iter()
            .filter(|w| w.view.status.is_watchable())
            .cloned()
            .collect();
        let events = if gap.is_some() {
            ready
                .iter()
                .map(|w| RawEvent {
                    worktree: PathBuf::from(w.view.path.raw()),
                    kind: GitEventKind::Reconciled,
                    details: GitEventDetails::default(),
                    observed_ms: now_ms,
                    offset_s,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.shared.send(ObservedBatch {
            repo_id: repo_id.to_owned(),
            worktrees: ready.clone(),
            gone: Vec::new(),
            events,
            gap,
            refs: None,
            head_logs: Vec::new(),
            heads: Vec::new(),
            marks: Marks {
                t_recv: t,
                t_flush: t,
                t_computed: t,
            },
        });
        // The repo task starts from the view it slept with and flushes at
        // once: what moved while dormant is classified now.
        let (tx, rx) = channel();
        let _ = tx.send(RepoMsg::Paths(t));
        if let Some(repo) = self
            .shared
            .repos
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(repo_id)
        {
            repo.tx = tx;
        }
        // Worktrees removed while it slept: their watches go.
        let gone: Vec<PathBuf> = asleep
            .roots
            .iter()
            .filter(|root| {
                !ready
                    .iter()
                    .any(|w| Path::new(w.view.path.raw()) == root.as_path())
            })
            .cloned()
            .collect();
        self.shared.unwatch(&gone);
        let head_logs = asleep.refs.head_logs();
        let mut heads = Vec::new();
        for w in &ready {
            let git_dir = worktree_git_dir(&common, w);
            heads.extend(self.shared.start_worktree(repo_id, w.clone(), git_dir));
        }
        let shared = Arc::clone(&self.shared);
        let id = repo_id.to_owned();
        let refs = asleep.refs;
        let _ = std::thread::Builder::new()
            .name("raptor-watch-repo".into())
            .spawn(move || repo::run(shared, id, common, refs, rx));
        if let Some(hooks) = &self.shared.hooks {
            for w in &ready {
                hooks.worktree_changed(repo_id, Path::new(w.view.path.raw()));
            }
        }
        WatchStart { head_logs, heads }
    }

    /// A wake the daemon could not complete (its store or its read failed):
    /// the repo is dormant again, so the sentinel and the safety nets keep
    /// watching it and the next trigger retries.
    pub fn abort_wake(&self, repo_id: &str) {
        let repos = self.shared.repos.read().unwrap_or_else(|e| e.into_inner());
        if let Some(repo) = repos.get(repo_id)
            && repo.asleep.is_some()
        {
            let _ = repo.tier.compare_exchange(
                Tier::Waking.as_u8(),
                Tier::Dormant.as_u8(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    /// Runs one cycle of the dormant sweep now (tests: the cycle the task
    /// would run at its next interval).
    #[doc(hidden)]
    pub fn sweep_now(&self) {
        sweep_once(&self.shared);
    }

    /// Messages of dormant repos the sentinel has decided on (tests).
    #[doc(hidden)]
    pub fn sentinel_seen(&self) -> u64 {
        self.shared
            .sentinel_seen
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// The effective interval of the slow reconciliation: the configured
    /// minimum, or longer when the dormant repos do not fit in the CPU
    /// budget (N3, exposed by `engine.resources`).
    pub fn reconcile_interval(&self) -> Duration {
        self.shared
            .budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .interval()
    }

    /// What `engine.resources` shows of the tiers (N8), read when asked.
    pub fn usage_source(&self) -> crate::resources::ObservationSource {
        let weak = Arc::downgrade(&self.shared);
        Arc::new(move || weak.upgrade().map(|shared| usage(&shared)))
    }

    /// Sweep cycles run so far.
    pub fn sweeps(&self) -> u64 {
        self.shared
            .sweeps
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Test hook: every new watch fails while `fail` (the worktrees it
    /// starts are polled, in degraded mode).
    #[doc(hidden)]
    pub fn simulate_watch_failure(&self, fail: bool) {
        self.shared
            .fail_watches
            .store(fail, std::sync::atomic::Ordering::Relaxed);
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
        self.shared
            .sentinel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.shared
            .sweep_stop
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        // Dropping the watchers ends the OS streams.
        self.shared
            .watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

/// Repos, worktrees and watches per tier, and the cost of the safety nets.
fn usage(shared: &Shared) -> gitraptor_api::resources::ObservationUsage {
    use gitraptor_api::resources::{
        DegradedUsage, DormantUsage, ObservationUsage, TierUsage, WakingUsage,
    };
    let mut active = TierUsage {
        repos: 0,
        worktrees: 0,
        watches: 0,
    };
    let mut waking = 0;
    let mut dormant = active;
    let mut degraded = 0;
    {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        for repo in repos.values() {
            let roots: Vec<&PathBuf> = match &repo.asleep {
                Some(a) => a.roots.iter().collect(),
                None => repo.worktrees.iter().map(|w| &w.root).collect(),
            };
            let common = u64::from(!roots.iter().any(|r| repo.common.starts_with(r)));
            let worktrees = roots.len() as u64;
            let watched = repo.worktrees.iter().filter(|w| !w.degraded).count() as u64;
            degraded += repo.worktrees.iter().filter(|w| w.degraded).count() as u64;
            match repo.tier() {
                Tier::Active => {
                    active.repos += 1;
                    active.worktrees += worktrees;
                    active.watches += watched + common;
                }
                Tier::Waking => waking += 1,
                Tier::Dormant => {
                    dormant.repos += 1;
                    dormant.worktrees += worktrees;
                    dormant.watches += worktrees + common;
                }
            }
        }
    }
    let life = shared.started.elapsed().as_nanos() as f64;
    let net = shared.net_ns.load(std::sync::atomic::Ordering::Relaxed) as f64;
    let reconcile = shared
        .budget
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .interval();
    ObservationUsage {
        active,
        waking: WakingUsage { repos: waking },
        dormant: DormantUsage {
            repos: dormant.repos,
            worktrees: dormant.worktrees,
            watches: dormant.watches,
            sweep_interval_s: shared.config.dormant_poll.as_secs(),
            reconcile_interval_s: reconcile.as_secs(),
            safety_net_cpu_pct: (life > 0.0).then(|| net / life * 100.0),
        },
        degraded: DegradedUsage {
            worktrees: degraded,
        },
    }
}

/// Starts the sentinel task of the dormant repos (N2): it applies the
/// ignore filter of § 2 to their worktree paths, and the first path that
/// passes wakes the repo. It never recomputes and never opens a store.
fn start_sentinel(shared: &Arc<Shared>) {
    let (tx, rx) = channel::<SentinelMsg>();
    *shared.sentinel.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let weak: Weak<Shared> = Arc::downgrade(shared);
    let _ = std::thread::Builder::new()
        .name("raptor-sentinel".into())
        .spawn(move || {
            let mut caches: HashMap<PathBuf, worktree::IgnoreCache> = HashMap::new();
            while let Ok(msg) = rx.recv() {
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                sentinel_check(&shared, &mut caches, msg);
                shared
                    .sentinel_seen
                    .fetch_add(1, std::sync::atomic::Ordering::Release);
            }
        });
}

/// The sentinel's decision on one message: wake its repo or not.
fn sentinel_check(
    shared: &Shared,
    caches: &mut HashMap<PathBuf, worktree::IgnoreCache>,
    msg: SentinelMsg,
) {
    let dormant = {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos
            .get(&msg.repo_id)
            .is_some_and(|r| r.tier() == Tier::Dormant)
    };
    if !dormant {
        return;
    }
    let keep = match &msg.root {
        Some(root) => caches
            .entry(root.clone())
            .or_default()
            .keep_any(root, msg.paths),
        None => true,
    };
    if !keep {
        return;
    }
    let woke = {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos.get(&msg.repo_id).is_some_and(RepoEntry::start_waking)
    };
    if woke {
        // A woken repo's tasks have their own cache; the rest is cheap to
        // ask again.
        caches.clear();
        shared.wake(&msg.repo_id, WakeCause::Sentinel);
    }
}

/// Starts the sweep task of the dormant repos (N3): one task for all of
/// them, one wake per interval.
fn start_sweep(shared: &Arc<Shared>) {
    let (tx, rx) = channel::<()>();
    *shared.sweep_stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let weak: Weak<Shared> = Arc::downgrade(shared);
    let every = shared.config.dormant_poll;
    let _ = std::thread::Builder::new()
        .name("raptor-dormant-sweep".into())
        .spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(every) {
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                sweep_once(&shared);
            }
        });
}

/// One cycle of the sweep: the print of every dormant repo against the one
/// it slept with. Metadata only: no `git`, no read-only layer.
fn sweep_once(shared: &Shared) {
    let started = Instant::now();
    sweep_and_reconcile(shared);
    let spent = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    shared
        .net_ns
        .fetch_add(spent, std::sync::atomic::Ordering::Relaxed);
}

fn sweep_and_reconcile(shared: &Shared) {
    shared
        .sweeps
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dormant: Vec<(String, PathBuf, Vec<PathBuf>)> = {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos
            .values()
            .filter(|r| r.tier() == Tier::Dormant)
            .filter_map(|r| {
                let a = r.asleep.as_ref()?;
                Some((r.id.clone(), r.common.clone(), a.git_dirs.clone()))
            })
            .collect()
    };
    let mut woken = Vec::new();
    for (repo_id, common, git_dirs) in dormant {
        let print = Print::read(&common, &git_dirs);
        let now = wall_now().0;
        let since = {
            let mut repos = shared.repos.write().unwrap_or_else(|e| e.into_inner());
            let Some(repo) = repos.get_mut(&repo_id) else {
                continue;
            };
            if repo.tier() != Tier::Dormant {
                continue;
            }
            let Some(asleep) = repo.asleep.as_mut() else {
                continue;
            };
            let since = std::mem::replace(&mut asleep.checked_ms, now);
            if asleep.print == print {
                continue;
            }
            asleep.print = print;
            if !repo.start_waking() {
                continue;
            }
            since
        };
        woken.push(repo_id.clone());
        shared.wake(&repo_id, WakeCause::SafetyNet { since_ms: since });
    }
    slow_reconcile(shared, &woken);
}

/// The slow reconciliation (N3): at most one dormant repo per sweep cycle,
/// the one that waited longest, once its effective interval has passed. It
/// reads every worktree in full, by stat, and compares with what its task
/// last published; a difference wakes the repo in a `dormant` gap. The time
/// it takes feeds the CPU budget, which stretches the interval when the
/// dormant repos do not fit.
fn slow_reconcile(shared: &Shared, skip: &[String]) {
    let interval = shared
        .budget
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .interval();
    let due = {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos
            .values()
            .filter(|r| r.tier() == Tier::Dormant && !skip.contains(&r.id))
            .filter_map(|r| {
                let a = r.asleep.as_ref()?;
                (a.reconciled.elapsed() >= interval)
                    .then(|| (a.reconciled, r.id.clone(), a.slept.clone()))
            })
            .min_by_key(|(at, _, _)| *at)
    };
    let Some((_, repo_id, slept)) = due else {
        return;
    };
    let started = Instant::now();
    let changed = slept.iter().any(|w| {
        shared
            .recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let read = crate::observe::read_worktree(&w.root, w.main, w.admin.as_deref());
        read.fingerprint != w.fingerprint
    });
    let cost = started.elapsed();
    let dormant = {
        let repos = shared.repos.read().unwrap_or_else(|e| e.into_inner());
        repos.values().filter(|r| r.tier() == Tier::Dormant).count()
    };
    shared
        .budget
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .record(&repo_id, cost, dormant);
    let now = wall_now().0;
    let since = {
        let mut repos = shared.repos.write().unwrap_or_else(|e| e.into_inner());
        let Some(repo) = repos.get_mut(&repo_id) else {
            return;
        };
        let Some(asleep) = repo.asleep.as_mut() else {
            return;
        };
        asleep.reconciled = Instant::now();
        asleep.checked_ms = now;
        // What it found may be as old as its previous read.
        let since = std::mem::replace(&mut asleep.reconciled_ms, now);
        if !changed || !repo.start_waking() {
            return;
        }
        since
    };
    shared.wake(&repo_id, WakeCause::SafetyNet { since_ms: since });
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

    #[test]
    fn under_is_a_prefix_of_whole_components() {
        let root = Path::new("/w/repo");
        assert!(under(Path::new("/w/repo"), root));
        assert!(under(Path::new("/w/repo/src/a.rs"), root));
        assert!(!under(Path::new("/w/repo2/a.rs"), root));
        assert!(!under(Path::new("/w/rep"), root));
        assert!(under(Path::new("/w/repo/x"), Path::new("/w/repo/")));
    }

    /// The router found no event under any root on Windows: its separator is `\\`.
    #[cfg(windows)]
    #[test]
    fn under_follows_the_windows_separator() {
        let root = Path::new(r"C:\src\repo");
        assert!(under(Path::new(r"C:\src\repo"), root));
        assert!(under(Path::new(r"C:\src\repo\a.rs"), root));
        assert!(under(Path::new(r"C:\src\repo\sub\a.rs"), root));
        assert!(!under(Path::new(r"C:\src\repo2\a.rs"), root));
        assert!(under(
            Path::new(r"C:\src\repo\x"),
            Path::new(r"C:\src\repo\")
        ));
    }

    /// On Unix a backslash is part of a name, never a separator.
    #[cfg(unix)]
    #[test]
    fn under_does_not_treat_a_backslash_as_a_separator_on_unix() {
        assert!(!under(Path::new("/w/repo\\a.rs"), Path::new("/w/repo")));
    }

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
