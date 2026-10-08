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
//! - **Raw events** of a worktree, with the actor the engine attributed them to. Engine data is
//!   read through the daemon, never copied (ADR-TMC-003).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::GitEventKind;

use super::oplog::{Oplog, Requester};
use crate::profile::{ProfileDirs, fsperm};

/// Quiet time of a repo's Git directory: the fallback calm of a worktree without a `HEAD`
/// reflog (two windows of the observer, ADR-GRP-011 § 2).
pub const SETTLE: Duration = Duration::from_millis(150);

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
}

/// Marks and Git directory activity per repo. The daemon loop sets marks and reflog sizes after
/// each batch it persists (mark first); the observer's router reports the activity.
#[derive(Debug, Default)]
pub struct RepoMarks {
    repos: Mutex<HashMap<String, Arc<RepoMark>>>,
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
        // A repo never touched is quiet: on Windows the clock counts from this process's first
        // reading, so "long ago" cannot be told from the time alone (XP-02).
        let quiet = || {
            let touched = self.touched(repo_id);
            let settle_ns = u64::try_from(SETTLE.as_nanos()).unwrap_or(u64::MAX);
            touched == 0
                || gitraptor_api::clock::monotonic_ns().saturating_sub(touched) >= settle_ns
        };
        match self.head_log(repo_id, worktree) {
            Some(persisted) if on_disk > 0 => persisted == on_disk,
            _ => quiet(),
        }
    }

    /// Waits until every worktree is calm, up to `limit`; the mark then.
    pub fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        let end = Instant::now() + limit;
        loop {
            if worktrees.iter().all(|w| self.calm(repo_id, w)) {
                return Some(self.seq(repo_id));
            }
            if Instant::now() + CALM_POLL > end {
                return None;
            }
            std::thread::sleep(CALM_POLL);
        }
    }
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

fn head_log_len(git_dir: &Path) -> u64 {
    std::fs::symlink_metadata(git_dir.join("logs").join("HEAD"))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len())
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
