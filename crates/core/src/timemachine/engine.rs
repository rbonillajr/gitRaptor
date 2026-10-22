//! What the Time Machine reads from the engine (US-TMC-004, ADR-TMC-004 § 2).
//!
//! - **Mark**: the last event sequence persisted in a repo's engine store (ADR-GRP-013). Every
//!   operation, undo and capture records it, so the undo stack can place them among raw Git
//!   events. Marks only compare within one **generation** of the engine store: a store recreated
//!   after a loss numbers again from 1 (ADR-TMC-003 § 1, Q26).
//! - **Calm, by state**: the engine persists a Git event some time after the `git` that caused it.
//!   A worktree is calm when the size of its `HEAD` reflog on disk is the one the engine read in
//!   its last persisted batch, and no `git` holds its index. Every `git` that changes a worktree's
//!   state writes there (commit, checkout, merge, rebase, reset, stash), so a calm worktree has no
//!   such `git` the mark does not cover yet. Its worktree task names branch switches from `HEAD`,
//!   so the `HEAD` it read for its last persisted batch must be the one on disk too. Without a
//!   reflog (`core.logAllRefUpdates=false`),
//!   the fallback is time: no change in the repo's Git directory for [`SETTLE`].
//! - **Calm, by refs**: the reflog is not enough. `git` appends to the `HEAD` reflog before it
//!   renames the ref, and `branch -D` or the applier's `update-ref` do not write it at all. So the
//!   repo is also calm only if its local branches, read from the common directory the daemon
//!   registered, are those of the last batch the engine persisted with branches. The check is
//!   per repo: another worktree's commits delay this one's calm. A repo whose refs cannot be read
//!   is not calm; a repo whose engine never published branches falls back to time.
//! - **Memory order**: the writer publishes a batch's mark and branches in one critical section,
//!   and the reader compares the branches and reads the mark in that same section, so branches
//!   that match batch N come with a mark of at least N's.
//! - **Raw events** of a worktree, with the actor the engine attributed them to. Engine data is
//!   read through the daemon, never copied (ADR-TMC-003).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gitraptor_api::messages::GitEventKind;
use gitraptor_git::{ReaderOptions, RepoReader};

use super::oplog::{Oplog, Requester};
use super::store::ValidityGuard;
use crate::daemon::{Field, Logger};
use crate::detect::{GitCall, ProcLister};
use crate::profile::{ProfileDirs, fsperm};

/// Quiet time of a repo's Git directory: the fallback calm of a worktree without a `HEAD`
/// reflog (two windows of the observer, ADR-GRP-011 § 2).
pub const SETTLE: Duration = Duration::from_millis(150);

/// How long a live `git` whose command line or folder cannot be read holds the calm: the longest
/// wait of a `settle` (`ANCHOR_SETTLE_LIMIT`). Past it, it is logged and the other checks
/// (reflog, refs, hook calls in flight) decide.
const UNREADABLE_GRACE: Duration = Duration::from_secs(2);

/// How often the calm is checked again while waiting.
const CALM_POLL: Duration = Duration::from_millis(10);

/// A raw Git event of a worktree, as the engine stored it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawGitEvent {
    pub seq: i64,
    pub kind: GitEventKind,
    /// The branch the event names, if any (untrusted text from the repo).
    pub branch: Option<String>,
    /// The session the engine attributed it to, or "unattributed".
    pub actor: Requester,
}

/// The engine as the Time Machine sees it.
pub trait EngineLink: Send + Sync {
    /// The repo's mark now.
    fn mark(&self, repo_id: &str) -> i64;
    /// Waits until every worktree in `worktrees` is calm, up to `limit`; the mark then.
    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64>;
    /// The raw Git events of `worktree`, oldest first; `None` if they cannot be read.
    fn raw_events(&self, repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>>;
    /// First oplog sequence of the current generation of the repo's engine store: rows before it
    /// carry marks of another numbering (or of the bus, before US-TMC-004).
    fn generation_floor(&self, repo_id: &str) -> i64;
    /// A counter of the repo's file activity as the engine published it: a capture that sees it
    /// move gives way (US-TMC-004).
    fn activity(&self, repo_id: &str) -> u64;
    /// [`EngineLink::settle`] without counting what `ex` names: the `git` that is waiting for this
    /// very answer and its own hook call. The default ignores `ex` (doubles without processes).
    fn settle_excluding(
        &self,
        repo_id: &str,
        worktrees: &[PathBuf],
        limit: Duration,
        ex: &CalmExclusions<'_>,
    ) -> Option<i64> {
        let _ = ex;
        self.settle(repo_id, worktrees, limit)
    }
    /// A hook call in flight on `worktree`: no calm there until the guard drops, except for the
    /// `settle` that names its token. The default is inert (doubles).
    fn hook_begin(&self, repo_id: &str, worktree: &Path) -> HookInFlight {
        let _ = (repo_id, worktree);
        HookInFlight::inert()
    }
}

/// What a `settle` does not count as a `git` in its way.
#[derive(Debug, Clone, Copy, Default)]
pub struct CalmExclusions<'a> {
    /// The `git` ancestors of the hook's client: the command waiting for this answer.
    pub pids: &'a [u32],
    /// The hook call this `settle` answers.
    pub own_hook: Option<HookToken>,
}

/// Names one hook call in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookToken(u64);

/// A hook call in flight, counted until it drops.
#[derive(Debug)]
pub struct HookInFlight {
    token: HookToken,
    registry: Option<Arc<Mutex<HashMap<u64, HookCall>>>>,
}

/// Where a hook call in flight runs.
#[derive(Debug)]
struct HookCall {
    repo_id: String,
    worktree: PathBuf,
}

impl HookInFlight {
    /// A guard that counts nothing.
    pub fn inert() -> Self {
        Self {
            token: HookToken(0),
            registry: None,
        }
    }

    pub fn token(&self) -> HookToken {
        self.token
    }
}

impl Drop for HookInFlight {
    fn drop(&mut self) {
        if let Some(registry) = &self.registry {
            registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.token.0);
        }
    }
}

/// A running `git` that holds the calm: its pid and its subcommand, sanitized.
#[derive(Debug, Clone)]
struct GitBlocker {
    pid: u32,
    subcommand: String,
}

#[derive(Debug, Default)]
struct RepoMark {
    seq: AtomicI64,
    touch_ns: AtomicU64,
    activity: AtomicU64,
    floor: AtomicI64,
    /// Size of each worktree's `HEAD` reflog in the last persisted batch, by canonical root.
    head_logs: Mutex<HashMap<PathBuf, u64>>,
    /// Each worktree's `HEAD` as its task read it for the last persisted batch.
    heads: Mutex<HashMap<PathBuf, Vec<u8>>>,
    /// The last batch that carried the repo's local branches; `None` until the engine publishes
    /// one. Its sequence lives here with the branches so a reader that sees them sees it too.
    persisted: Mutex<Option<Persisted>>,
}

/// A persisted batch as the refs barrier compares it.
#[derive(Debug)]
struct Persisted {
    seq: i64,
    /// The common directory the daemon registered: the refs are read there, never through a
    /// worktree's `.git`.
    common_dir: PathBuf,
    branches: BTreeMap<String, String>,
}

/// Marks and Git directory activity per repo. The daemon loop sets marks and reflog sizes after
/// each batch it persists (mark first); the observer's router reports the activity.
#[derive(Default)]
pub struct RepoMarks {
    repos: Mutex<HashMap<String, Arc<RepoMark>>>,
    /// The user's processes; without them the calm does not look at running `git`s.
    procs: OnceLock<Arc<dyn ProcLister>>,
    logger: OnceLock<Logger>,
    /// `tm_calm_probe_unsupported` was logged.
    unsupported_logged: AtomicBool,
    /// Hook calls in flight, by token.
    hooks: Arc<Mutex<HashMap<u64, HookCall>>>,
    next_hook: AtomicU64,
    /// What the process probe already knows, by process identity.
    probe: Mutex<ProbeCache>,
}

/// The process probe's memory, by `(pid, start_us)`: an identity that survives pid reuse. Kept
/// only for processes still in the table.
#[derive(Debug, Default)]
struct ProbeCache {
    /// `None`: not a `git`; `Some(call)`: a `git`, with its command line if it could be read.
    calls: HashMap<(u32, u64), Option<Option<GitCall>>>,
    /// When a live `git` was first found unreadable.
    unreadable: HashMap<(u32, u64), Instant>,
    /// Unreadable ones already logged.
    logged: std::collections::HashSet<(u32, u64)>,
}

impl ProbeCache {
    fn prune(&mut self, alive: &std::collections::HashSet<(u32, u64)>) {
        self.calls.retain(|k, _| alive.contains(k));
        self.unreadable.retain(|k, _| alive.contains(k));
        self.logged.retain(|k| alive.contains(k));
    }
}

impl std::fmt::Debug for RepoMarks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoMarks")
            .field("repos", &self.repos)
            .field("procs", &self.procs.get().is_some())
            .finish_non_exhaustive()
    }
}

impl RepoMarks {
    fn get(&self, repo_id: &str) -> Arc<RepoMark> {
        let mut repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(repos.entry(repo_id.to_owned()).or_default())
    }

    /// The repo's last persisted sequence is at least `seq`.
    pub fn set_seq(&self, repo_id: &str, seq: i64) {
        self.get(repo_id).seq.fetch_max(seq, Ordering::AcqRel);
    }

    /// The engine persisted a batch up to `seq`, read with these local branches of the repo whose
    /// common directory is `common_dir`. The mark and the branches are published in one critical
    /// section: a calm that sees these branches returns a mark of at least `seq`.
    pub fn set_persisted(
        &self,
        repo_id: &str,
        seq: i64,
        common_dir: &Path,
        branches: BTreeMap<String, String>,
    ) {
        let mark = self.get(repo_id);
        let mut persisted = mark.persisted.lock().unwrap_or_else(|e| e.into_inner());
        mark.seq.fetch_max(seq, Ordering::AcqRel);
        *persisted = Some(Persisted {
            seq,
            common_dir: common_dir.to_path_buf(),
            branches,
        });
    }

    pub fn seq(&self, repo_id: &str) -> i64 {
        self.get(repo_id).seq.load(Ordering::Acquire)
    }

    /// The engine persisted what it read of these `HEAD` reflogs.
    pub fn set_head_logs(&self, repo_id: &str, logs: &[(PathBuf, u64)]) {
        let mark = self.get(repo_id);
        let mut map = mark.head_logs.lock().unwrap_or_else(|e| e.into_inner());
        for (root, len) in logs {
            map.insert(root.clone(), *len);
        }
    }

    /// The engine persisted what its worktree tasks read with these `HEAD`s.
    pub fn set_heads(&self, repo_id: &str, heads: &[(PathBuf, Vec<u8>)]) {
        let mark = self.get(repo_id);
        let mut map = mark.heads.lock().unwrap_or_else(|e| e.into_inner());
        for (root, head) in heads {
            map.insert(root.clone(), head.clone());
        }
    }

    fn head(&self, repo_id: &str, root: &Path) -> Option<Vec<u8>> {
        let mark = self.get(repo_id);
        let map = mark.heads.lock().unwrap_or_else(|e| e.into_inner());
        map.get(root).cloned()
    }

    pub fn head_log(&self, repo_id: &str, root: &Path) -> Option<u64> {
        let mark = self.get(repo_id);
        let map = mark.head_logs.lock().unwrap_or_else(|e| e.into_inner());
        map.get(root).copied()
    }

    /// A file of the repo's Git directory changed at monotonic `t_ns`.
    pub fn touch(&self, repo_id: &str, t_ns: u64) {
        self.get(repo_id).touch_ns.fetch_max(t_ns, Ordering::AcqRel);
    }

    pub fn touched(&self, repo_id: &str) -> u64 {
        self.get(repo_id).touch_ns.load(Ordering::Acquire)
    }

    /// A worktree of the repo changed (published by the engine).
    pub fn bump_activity(&self, repo_id: &str) {
        self.get(repo_id).activity.fetch_add(1, Ordering::AcqRel);
    }

    pub fn activity(&self, repo_id: &str) -> u64 {
        self.get(repo_id).activity.load(Ordering::Acquire)
    }

    pub fn set_floor(&self, repo_id: &str, floor: i64) {
        self.get(repo_id).floor.store(floor, Ordering::Release);
    }

    pub fn floor(&self, repo_id: &str) -> i64 {
        self.get(repo_id).floor.load(Ordering::Acquire)
    }

    pub fn remove(&self, repo_id: &str) {
        self.repos
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(repo_id);
    }

    /// Whether `worktree` is calm now (see the module docs).
    pub fn calm(&self, repo_id: &str, worktree: &Path) -> bool {
        let one = std::slice::from_ref(&worktree);
        self.files_calm(repo_id, worktree)
            && !self.hook_in_flight(repo_id, one, None)
            && self.git_blocker(repo_id, one, &[]).is_none()
    }

    /// The cheap part of the calm, read from the worktree's Git files only.
    fn files_calm(&self, repo_id: &str, worktree: &Path) -> bool {
        let Some(git_dir) = git_dir_of(worktree) else {
            // Gone or unreadable: nothing of it can be captured anyway.
            return true;
        };
        if git_dir.join("index.lock").symlink_metadata().is_ok() {
            return false;
        }
        // The worktree's own task names a branch switch from `HEAD`: the one
        // it read for its last persisted batch must be the one on disk.
        if let Some(head) = self.head(repo_id, worktree)
            && std::fs::read(git_dir.join("HEAD")).unwrap_or_default() != head
        {
            return false;
        }
        let on_disk = head_log_len(&git_dir);
        match self.head_log(repo_id, worktree) {
            Some(persisted) if on_disk > 0 => persisted == on_disk,
            _ => self.quiet(repo_id),
        }
    }

    /// No change in the repo's Git directory for [`SETTLE`]. A repo never touched is quiet: on
    /// Windows the clock counts from this process's first reading, so "long ago" cannot be told
    /// from the time alone (XP-02).
    fn quiet(&self, repo_id: &str) -> bool {
        let touched = self.touched(repo_id);
        let settle_ns = u64::try_from(SETTLE.as_nanos()).unwrap_or(u64::MAX);
        touched == 0 || gitraptor_api::clock::monotonic_ns().saturating_sub(touched) >= settle_ns
    }

    /// The repo's refs barrier: the mark, if the local branches on disk are those of the last
    /// persisted batch (see the module docs). Without a published batch, the time rule. A repo
    /// whose refs cannot be read is not calm.
    fn refs_calm(&self, repo_id: &str) -> Option<i64> {
        let mark = self.get(repo_id);
        let common_dir = {
            let persisted = mark.persisted.lock().unwrap_or_else(|e| e.into_inner());
            persisted.as_ref().map(|p| p.common_dir.clone())
        };
        let Some(common_dir) = common_dir else {
            return self
                .quiet(repo_id)
                .then(|| mark.seq.load(Ordering::Acquire));
        };
        // The disk is read without the lock, so a slow listing never holds up the writer. The
        // branches are then compared and the mark read under the lock the writer publishes both
        // with, so the mark returned covers the batch whose branches matched.
        let reader = RepoReader::open(&common_dir, &ReaderOptions::default()).ok()?;
        let on_disk = crate::watch::read_local_branches(&reader).ok()?;
        let persisted = mark.persisted.lock().unwrap_or_else(|e| e.into_inner());
        let p = persisted.as_ref()?;
        (on_disk == p.branches).then(|| p.seq.max(mark.seq.load(Ordering::Acquire)))
    }

    /// The user's processes: with them, no worktree is calm while a `git` runs in it. Set once;
    /// a later call is ignored.
    pub fn set_procs(&self, procs: Arc<dyn ProcLister>) {
        let _ = self.procs.set(procs);
    }

    /// Where a `settle` blocked by a running `git` says so. Set once.
    pub fn set_logger(&self, logger: Logger) {
        let _ = self.logger.set(logger);
    }

    /// A hook call in flight on `worktree`, counted until the guard drops.
    pub fn hook_begin(&self, repo_id: &str, worktree: &Path) -> HookInFlight {
        let token = self.next_hook.fetch_add(1, Ordering::AcqRel) + 1;
        self.hooks.lock().unwrap_or_else(|e| e.into_inner()).insert(
            token,
            HookCall {
                repo_id: repo_id.to_owned(),
                worktree: worktree.to_path_buf(),
            },
        );
        HookInFlight {
            token: HookToken(token),
            registry: Some(Arc::clone(&self.hooks)),
        }
    }

    /// Whether a hook call other than `own` is in flight in one of `worktrees`.
    fn hook_in_flight(&self, repo_id: &str, worktrees: &[&Path], own: Option<HookToken>) -> bool {
        let hooks = self.hooks.lock().unwrap_or_else(|e| e.into_inner());
        hooks.iter().any(|(token, call)| {
            own != Some(HookToken(*token))
                && call.repo_id == repo_id
                && worktrees
                    .iter()
                    .any(|w| call.worktree.starts_with(w) || w.starts_with(&call.worktree))
        })
    }

    /// The first `git` running in one of `worktrees`, leaving out the daemon's own children,
    /// `excluded` and the long-lived helpers. A `git` counts by its folder, or, working
    /// elsewhere, by a redirection (`-C`, `--git-dir`, `GIT_DIR`…) that resolves into this repo.
    /// One the reader cannot read counts while it is alive, for [`UNREADABLE_GRACE`] at most, and
    /// is then logged once and left to the other checks; one that exited does not count. A
    /// process table that cannot be read fails closed. `None` without a lister.
    fn git_blocker(
        &self,
        repo_id: &str,
        worktrees: &[&Path],
        excluded: &[u32],
    ) -> Option<GitBlocker> {
        let procs = self.procs.get()?;
        if !crate::detect::detection_supported() {
            if !self.unsupported_logged.swap(true, Ordering::AcqRel)
                && let Some(logger) = self.logger.get()
            {
                logger.warn("tm_calm_probe_unsupported", &[("repo", Field::id(repo_id))]);
            }
            return None;
        }
        let Some(table) = procs.list_bare() else {
            return Some(GitBlocker {
                pid: 0,
                subcommand: "unlisted".to_owned(),
            });
        };
        let me = std::process::id();
        let scopes: Vec<(&Path, Option<PathBuf>)> =
            worktrees.iter().map(|w| (*w, git_dir_of(w))).collect();
        // Where this repo is, for a redirection: its common directory and every registered
        // worktree root. Read only if a `git` elsewhere redirects.
        let repo_paths = std::cell::OnceCell::new();
        let in_repo = |target: &Path| {
            let (common, roots) = repo_paths.get_or_init(|| self.repo_paths(repo_id, worktrees));
            common
                .as_ref()
                .is_some_and(|c: &PathBuf| target.starts_with(c))
                || roots.iter().any(|r: &PathBuf| target.starts_with(r))
        };
        let mut probe = self.probe.lock().unwrap_or_else(|e| e.into_inner());
        let alive: std::collections::HashSet<(u32, u64)> =
            table.iter().map(|e| (e.pid, e.start_us)).collect();
        probe.prune(&alive);
        let now = Instant::now();
        let mut found = None;
        for entry in table {
            if entry.ppid == me || excluded.contains(&entry.pid) {
                continue;
            }
            let key = (entry.pid, entry.start_us);
            let call = match probe.calls.get(&key) {
                Some(known) => known.clone(),
                None => {
                    // Gone, or its pid names another process now.
                    let Some(exe) = procs.exe(&entry) else {
                        continue;
                    };
                    let known = matches!(
                        exe.file_name().and_then(|n| n.to_str()),
                        Some("git" | "git.exe")
                    )
                    .then(|| procs.git_call(entry.pid));
                    probe.calls.insert(key, known.clone());
                    known
                }
            };
            // Not a `git`, or a long-lived helper that never changes a worktree.
            let Some(call) = call else {
                continue;
            };
            if call.as_ref().is_some_and(|c| c.long_lived) {
                continue;
            }
            let verdict = match procs.cwd(entry.pid) {
                Some(cwd)
                    if scopes.iter().any(|(w, g)| {
                        cwd.starts_with(w) || g.as_ref().is_some_and(|g| cwd.starts_with(g))
                    }) =>
                {
                    Some(true)
                }
                // Read on every pass: only `git`s working elsewhere get here, and a reader may
                // answer differently while the process lives (one of the tests' tables does).
                Some(_) => procs.git_redirect_into(entry.pid, &in_repo),
                None => None,
            };
            let counts = match verdict {
                Some(counts) => counts,
                // Unreadable: it exited, or it is alive and the reader cannot see it.
                None if procs.exe(&entry).is_none() => false,
                None => {
                    let since = *probe.unreadable.entry(key).or_insert(now);
                    if now.duration_since(since) < UNREADABLE_GRACE {
                        true
                    } else {
                        if probe.logged.insert(key)
                            && let Some(logger) = self.logger.get()
                        {
                            logger.warn(
                                "tm_calm_probe_unreadable",
                                &[
                                    ("repo", Field::id(repo_id)),
                                    ("pid", Field::Int(i64::from(entry.pid))),
                                ],
                            );
                        }
                        false
                    }
                }
            };
            if counts && found.is_none() {
                found = Some(GitBlocker {
                    pid: entry.pid,
                    subcommand: call.map_or_else(|| "unknown".to_owned(), |c| c.subcommand),
                });
            }
        }
        found
    }

    /// The repo's common directory (the one the daemon registered, else the first worktree's)
    /// and the roots of its registered worktrees plus `worktrees`, all canonical.
    fn repo_paths(&self, repo_id: &str, worktrees: &[&Path]) -> (Option<PathBuf>, Vec<PathBuf>) {
        let registered = {
            let mark = self.get(repo_id);
            let persisted = mark.persisted.lock().unwrap_or_else(|e| e.into_inner());
            persisted.as_ref().map(|p| p.common_dir.clone())
        };
        let common = registered
            .or_else(|| {
                worktrees
                    .first()
                    .and_then(|w| crate::observe::locate(w).ok())
            })
            .and_then(|c| gitraptor_git::paths::canonicalize(&c).ok());
        let mut roots: Vec<PathBuf> = worktrees.iter().map(|w| w.to_path_buf()).collect();
        if let Some(common) = &common {
            roots.extend(
                crate::observe::registered_worktrees(common)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(root, _)| root),
            );
        }
        (common, roots)
    }

    /// [`RepoMarks::settle`] without counting what `ex` names. Each poll checks, cheapest first:
    /// each worktree's Git files, the repo's refs, the hook calls in flight and, only if all of
    /// them say calm, one read of the process table for every worktree.
    pub fn settle_excluding(
        &self,
        repo_id: &str,
        worktrees: &[PathBuf],
        limit: Duration,
        ex: &CalmExclusions<'_>,
    ) -> Option<i64> {
        let end = Instant::now() + limit;
        let scope: Vec<&Path> = worktrees.iter().map(PathBuf::as_path).collect();
        loop {
            let mut blocked = None;
            if worktrees.iter().all(|w| self.files_calm(repo_id, w))
                && let Some(seq) = self.refs_calm(repo_id)
                && !self.hook_in_flight(repo_id, &scope, ex.own_hook)
            {
                match self.git_blocker(repo_id, &scope, ex.pids) {
                    None => return Some(seq),
                    Some(b) => blocked = Some(b),
                }
            }
            if Instant::now() + CALM_POLL > end {
                // A wait that ran out because of a running `git`: once per `settle`, with the pid
                // and the subcommand only. A single poll (`ZERO`, a confirmation) waits for
                // nothing and says nothing.
                if !limit.is_zero()
                    && let (Some(b), Some(logger)) = (blocked, self.logger.get())
                {
                    logger.warn(
                        "tm_calm_blocked_by_git",
                        &[
                            ("repo", Field::id(repo_id)),
                            ("pid", Field::Int(i64::from(b.pid))),
                            ("subcommand", Field::word(&b.subcommand)),
                        ],
                    );
                }
                return None;
            }
            std::thread::sleep(CALM_POLL);
        }
    }

    /// Waits until every worktree is calm and the repo's refs are those the engine persisted, up
    /// to `limit`; the mark then. The refs are checked once per poll, for the whole repo.
    pub fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        self.settle_excluding(repo_id, worktrees, limit, &CalmExclusions::default())
    }
}

/// The check that a capture sealed with `mark` still holds at its validity point: the engine is
/// calm now and its mark has not moved, so no event happened between the mark and the read. One
/// poll, never a wait.
///
/// With the calm by state and by refs, `settle` answering the same mark means: every `git` that
/// ended is persisted and nothing was persisted since the mark. Whatever the capture read between
/// the two then is the state the mark covers.
pub fn confirm_mark(
    engine: Arc<dyn EngineLink>,
    repo_id: &str,
    worktrees: &[PathBuf],
    mark: i64,
) -> ValidityGuard {
    confirm_mark_excluding(engine, repo_id, worktrees, mark, &CalmExclusions::default())
}

/// [`confirm_mark`] for a capture whose `settle` left out `ex`: its confirmation leaves out the
/// same.
pub fn confirm_mark_excluding(
    engine: Arc<dyn EngineLink>,
    repo_id: &str,
    worktrees: &[PathBuf],
    mark: i64,
    ex: &CalmExclusions<'_>,
) -> ValidityGuard {
    let (repo_id, worktrees) = (repo_id.to_owned(), worktrees.to_vec());
    let (pids, own_hook) = (ex.pids.to_vec(), ex.own_hook);
    ValidityGuard(Arc::new(move || {
        let ex = CalmExclusions {
            pids: &pids,
            own_hook,
        };
        engine.settle_excluding(&repo_id, &worktrees, Duration::ZERO, &ex) == Some(mark)
    }))
}

/// The Git directory of a worktree: `.git` itself, or the one its `.git` file names. Links are
/// not followed for `.git`.
pub fn git_dir_of(worktree: &Path) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let dir = text.trim().strip_prefix("gitdir:")?.trim();
    Some(worktree.join(dir))
}

/// The `HEAD` reflog up to its last complete line, as the engine publishes it.
fn head_log_len(git_dir: &Path) -> u64 {
    crate::watch::head_log_len(git_dir)
}

/// Whether a `git` holds the index of a worktree.
pub fn index_locked(worktree: &Path) -> bool {
    git_dir_of(worktree).is_some_and(|d| d.join("index.lock").symlink_metadata().is_ok())
}

/// What a `git` that changes a worktree touches: its `HEAD` reflog size, its `HEAD` and the
/// identity of its index. A capture compares it at its start and at its validity point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitState {
    head_log: u64,
    head: Vec<u8>,
    index: Option<(u64, u128, u64)>,
    locked: bool,
}

impl GitState {
    pub fn read(worktree: &Path) -> Option<Self> {
        let git_dir = git_dir_of(worktree)?;
        let index = std::fs::symlink_metadata(git_dir.join("index"))
            .ok()
            .map(|m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                (m.len(), mtime, inode(&m))
            });
        Some(Self {
            head_log: head_log_len(&git_dir),
            head: std::fs::read(git_dir.join("HEAD")).unwrap_or_default(),
            index,
            locked: git_dir.join("index.lock").symlink_metadata().is_ok(),
        })
    }

    pub fn locked(&self) -> bool {
        self.locked
    }
}

#[cfg(unix)]
fn inode(m: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(m)
}

#[cfg(not(unix))]
fn inode(_m: &std::fs::Metadata) -> u64 {
    0
}

/// The events of the timeline among a worktree's raw ones (ADR-TMC-003 § 4) that the stack can
/// undo: those that change its state. Pushes, reconciliations, and creating or deleting branches
/// and worktrees are not undone yet (US-TMC-009, US-TMC-014): as the latest, they stop the undo
/// with `raw-git-not-covered`.
pub fn is_undoable(kind: GitEventKind) -> bool {
    matches!(
        kind,
        GitEventKind::Commit
            | GitEventKind::Merge
            | GitEventKind::Rebase
            | GitEventKind::BranchUpdate
            | GitEventKind::BranchSwitch
            | GitEventKind::Reset
    )
}

/// File of the Time Machine that records where the current generation of the engine store
/// starts in the oplog: `<generation id> <first oplog seq>`.
const GENERATION_FILE: &str = "engine-generation";

/// The first oplog sequence of `generation` (ADR-TMC-003 § 1): read from the repo's folder of the
/// Time Machine, or, for a generation not seen before, the next row of the oplog, recorded then.
/// A generation id that cannot be recorded keeps nothing comparable: the floor is the next row.
pub fn generation_floor(dirs: &ProfileDirs, repo_id: &str, generation: &str, oplog: &Oplog) -> i64 {
    let next = oplog.last_seq().unwrap_or(i64::MAX - 1) + 1;
    let Ok(dir) = super::oplog::repo_dir(dirs, repo_id) else {
        return next;
    };
    let path = dir.join(GENERATION_FILE);
    let recorded = std::fs::symlink_metadata(&path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= 256)
        .and_then(|_| std::fs::read_to_string(&path).ok());
    if let Some(text) = recorded
        && let Some((id, seq)) = text.trim().split_once(' ')
        && id == generation
        && let Ok(seq) = seq.parse::<i64>()
        && seq <= next
    {
        return seq;
    }
    let tmp = dir.join(format!("{GENERATION_FILE}.tmp"));
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let _ = std::fs::remove_file(&tmp);
        let mut f = fsperm::create_private_file(&tmp).map_err(std::io::Error::other)?;
        f.write_all(format!("{generation} {next}\n").as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    let _ = written;
    next
}

#[cfg(test)]
#[path = "calm_git_alive_tests.rs"]
pub(crate) mod calm_git_alive_tests;

#[cfg(test)]
#[path = "calm_exclusions_tests.rs"]
mod calm_exclusions_tests;

#[cfg(test)]
#[path = "calm_redirect_tests.rs"]
mod calm_redirect_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_only_go_forward() {
        let marks = RepoMarks::default();
        marks.set_seq("r", 7);
        marks.set_seq("r", 3);
        assert_eq!(marks.seq("r"), 7);
        marks.touch("r", 10);
        marks.touch("r", 5);
        assert_eq!(marks.touched("r"), 10);
        assert_eq!(marks.seq("other"), 0);
    }

    #[test]
    fn a_worktree_is_calm_when_the_engine_read_its_head_reflog() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_path_buf();
        std::fs::create_dir_all(wt.join(".git/logs")).unwrap();
        std::fs::write(wt.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(wt.join(".git/logs/HEAD"), "a b c\tcommit: x\n").unwrap();
        let marks = RepoMarks::default();
        marks.set_seq("r", 4);
        marks.set_head_logs("r", &[(wt.clone(), 16)]);
        assert!(marks.calm("r", &wt));
        assert_eq!(
            marks.settle("r", std::slice::from_ref(&wt), Duration::ZERO),
            Some(4)
        );
        // A `git` the engine has not persisted yet.
        std::fs::write(
            wt.join(".git/logs/HEAD"),
            "a b c\tcommit: x\na b c\treset: y\n",
        )
        .unwrap();
        assert!(!marks.calm("r", &wt));
        assert_eq!(
            marks.settle("r", std::slice::from_ref(&wt), Duration::from_millis(30)),
            None
        );
        let len = std::fs::metadata(wt.join(".git/logs/HEAD")).unwrap().len();
        marks.set_head_logs("r", &[(wt.clone(), len)]);
        assert!(marks.calm("r", &wt));
        // A `git` holds the index.
        std::fs::write(wt.join(".git/index.lock"), "").unwrap();
        assert!(!marks.calm("r", &wt));
        assert!(index_locked(&wt));
    }

    #[test]
    fn without_a_reflog_the_calm_is_time() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_path_buf();
        std::fs::create_dir_all(wt.join(".git")).unwrap();
        let marks = RepoMarks::default();
        assert!(marks.calm("r", &wt));
        marks.touch("r", gitraptor_api::clock::monotonic_ns());
        assert!(!marks.calm("r", &wt));
        assert!(
            marks
                .settle("r", std::slice::from_ref(&wt), Duration::from_secs(2))
                .is_some()
        );
    }

    #[test]
    fn the_git_state_changes_with_a_git() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_path_buf();
        std::fs::create_dir_all(wt.join(".git/logs")).unwrap();
        std::fs::write(wt.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(wt.join(".git/index"), "i1").unwrap();
        let before = GitState::read(&wt).unwrap();
        assert_eq!(GitState::read(&wt).unwrap(), before);
        std::fs::write(wt.join(".git/logs/HEAD"), "a b c\treset: y\n").unwrap();
        assert_ne!(GitState::read(&wt).unwrap(), before);
    }

    #[test]
    fn only_state_changing_events_are_undoable() {
        assert!(is_undoable(GitEventKind::Reset));
        assert!(is_undoable(GitEventKind::Commit));
        assert!(!is_undoable(GitEventKind::Push));
        assert!(!is_undoable(GitEventKind::Reconciled));
        assert!(!is_undoable(GitEventKind::BranchDelete));
    }
}
