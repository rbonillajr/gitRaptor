//! Recovery when the daemon starts, before it accepts Time Machine
//! operations in a repo (ADR-TMC-003 § 6, ADR-TMC-007 § 4.5).
//!
//! Nothing is resumed or reverted on its own: the Time Machine only writes
//! when an actor asks (BR-TMC-CONS-004). Recovery only closes states:
//!
//! 1. `pending` snapshots become `discarded`, and their refs, if the store
//!    has them, are deleted. A ref the oplog does not know is reported and
//!    kept (NFR-01).
//! 2. Operations in `intent`, `prior-snapshot` or `ready` become `aborted`:
//!    the repo was not touched.
//! 3. Operations in `applying` become `interrupted`, with one notice per
//!    worktree of their scope; their prior snapshot is the way back.
//! 4. A Git lock the journal says an unfinished or interrupted operation
//!    took, and never released, is released if it is still the same file
//!    and no annotated child is alive. This is the only write recovery makes in the user's repo, a declared exception
//!    to BR-TMC-CONS-004; it lives in [`release_own_lock`] alone. A lock the
//!    journal does not name is never touched.
//! 5. A half-done purge is settled: ref present, the snapshot is available
//!    again; ref gone, it is purged.
//!
//! Running it twice changes nothing the second time.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::model::{NoticeKind, OperationState, SnapshotState};
use super::query::entries_of;
use super::{OperationTransition, Oplog, advance, notice};
use crate::profile::Result;

/// What the recovery needs from the snapshot store (TS-TMC-001).
pub trait SnapshotRefs {
    /// Ids of the snapshots whose ref exists in the store, or `None` when
    /// there is no store to ask: then nothing that depends on refs is
    /// decided.
    fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>>;
    /// Deletes the refs of these snapshots in one transaction of the store.
    fn delete(&mut self, snapshot_ids: &[String]) -> io::Result<()>;
}

/// No snapshot store is available yet: recovery decides nothing about refs.
#[derive(Debug, Clone, Copy, Default)]
pub struct AbsentStore;

impl SnapshotRefs for AbsentStore {
    fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>> {
        Ok(None)
    }

    fn delete(&mut self, _snapshot_ids: &[String]) -> io::Result<()> {
        Ok(())
    }
}

/// Whether a process still runs.
pub trait ProcessProbe {
    fn is_alive(&self, pid: u32) -> bool;
}

/// The OS answer. When it cannot tell (PID reused, no permission, Windows)
/// it says alive: the lock is then kept, never wrongly deleted.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProbe;

impl ProcessProbe for SystemProbe {
    #[cfg(unix)]
    fn is_alive(&self, pid: u32) -> bool {
        let Some(pid) = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        else {
            return true;
        };
        !matches!(
            rustix::process::test_kill_process(pid),
            Err(rustix::io::Errno::SRCH)
        )
    }

    #[cfg(not(unix))]
    fn is_alive(&self, _pid: u32) -> bool {
        true
    }
}

/// Inputs of the recovery of one repo.
pub struct RecoveryOptions<'a> {
    /// Git common directory of the repo, as validated by the index. Locks
    /// outside it are never deleted.
    pub git_dir: &'a Path,
    /// Shared by every repo of one start: past it, a lock held by a live
    /// child is kept and reported.
    pub deadline: Instant,
    pub poll: Duration,
    pub probe: &'a dyn ProcessProbe,
}

/// Why a lock annotated in the journal was not released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptLockReason {
    /// An annotated child is still alive past the deadline.
    ChildAlive,
    /// The file there now is not the one the operation took.
    InodeChanged,
    /// Not a regular file named `*.lock`.
    NotALockFile,
    /// Outside the repo's Git directory.
    OutsideGitDir,
    /// The journal entry is at a break of the chain.
    Tampered,
    /// This OS cannot check the file's identity yet.
    Unsupported,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptLock {
    pub path: PathBuf,
    pub reason: KeptLockReason,
}

/// What the recovery did, for the log and the client notices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Whether a snapshot store answered; without it, refs were not
    /// inspected.
    pub store_available: bool,
    pub discarded_snapshots: Vec<String>,
    pub deleted_refs: Vec<String>,
    /// Refs of the store that no snapshot row knows: kept.
    pub unknown_refs: Vec<String>,
    /// Snapshots `complete` whose ref is gone: a gap, never offered.
    pub missing_refs: Vec<String>,
    pub aborted_operations: Vec<String>,
    pub interrupted_operations: Vec<String>,
    pub notices: Vec<String>,
    pub released_locks: Vec<PathBuf>,
    pub kept_locks: Vec<KeptLock>,
    pub purges_cancelled: Vec<String>,
    pub purges_completed: Vec<String>,
}

impl RecoveryReport {
    /// Whether anything was found to recover.
    pub fn is_clean(&self) -> bool {
        self.discarded_snapshots.is_empty()
            && self.deleted_refs.is_empty()
            && self.aborted_operations.is_empty()
            && self.interrupted_operations.is_empty()
            && self.released_locks.is_empty()
            && self.kept_locks.is_empty()
            && self.purges_cancelled.is_empty()
            && self.purges_completed.is_empty()
            && self.missing_refs.is_empty()
            && self.unknown_refs.is_empty()
    }
}

impl Oplog {
    /// Recovers the repo after a start (see the module docs).
    pub fn recover(
        &mut self,
        refs: &mut dyn SnapshotRefs,
        options: &RecoveryOptions<'_>,
        now_ms: i64,
    ) -> Result<RecoveryReport> {
        let mut report = RecoveryReport::default();
        let present: Option<HashSet<String>> =
            refs.snapshot_ids()?.map(|ids| ids.into_iter().collect());
        report.store_available = present.is_some();

        self.recover_snapshots(refs, present.as_ref(), &mut report, now_ms)?;

        // Locks and children of operations that had not ended, read before
        // their states change.
        // An interrupted operation keeps its locks until they are released:
        // a child alive at one start may be gone at the next.
        let mut open_ops: HashMap<String, OperationState> = HashMap::new();
        let mut lock_ops: HashSet<String> = HashSet::new();
        for op in self.operations(&Default::default())? {
            if !op.state.is_terminal() {
                open_ops.insert(op.record.operation_id.clone(), op.state);
            }
            if !op.state.is_terminal() || op.state == OperationState::Interrupted {
                lock_ops.insert(op.record.operation_id);
            }
        }
        self.recover_operations(&open_ops, &mut report, now_ms)?;
        self.release_locks(&lock_ops, options, &mut report, now_ms)?;
        Ok(report)
    }

    fn recover_snapshots(
        &mut self,
        refs: &mut dyn SnapshotRefs,
        present: Option<&HashSet<String>>,
        report: &mut RecoveryReport,
        now_ms: i64,
    ) -> Result<()> {
        let snapshots = self.snapshots(&Default::default())?;
        for s in &snapshots {
            if s.state == SnapshotState::Pending {
                self.set_snapshot_state(&s.record.snapshot_id, SnapshotState::Discarded, now_ms)?;
                report
                    .discarded_snapshots
                    .push(s.record.snapshot_id.clone());
            }
        }
        let Some(present) = present else {
            return Ok(());
        };
        let known: HashMap<&str, SnapshotState> = snapshots
            .iter()
            .map(|s| (s.record.snapshot_id.as_str(), s.state))
            .collect();

        // Refs of snapshots that never completed: the operation they were
        // for never started.
        let mut to_delete: Vec<String> = Vec::new();
        for id in present {
            match known.get(id.as_str()) {
                Some(SnapshotState::Pending | SnapshotState::Discarded) => {
                    to_delete.push(id.clone())
                }
                Some(_) => {}
                None => report.unknown_refs.push(id.clone()),
            }
        }
        to_delete.sort();
        report.unknown_refs.sort();
        if !to_delete.is_empty() {
            refs.delete(&to_delete)?;
            report.deleted_refs = to_delete;
        }

        for s in &snapshots {
            let id = &s.record.snapshot_id;
            let has_ref = present.contains(id);
            match s.state {
                SnapshotState::PurgeIntent if has_ref => {
                    self.set_snapshot_state(id, SnapshotState::PurgeCancelled, now_ms)?;
                    report.purges_cancelled.push(id.clone());
                }
                SnapshotState::PurgeIntent => {
                    self.set_snapshot_state(id, SnapshotState::Purged, now_ms)?;
                    report.purges_completed.push(id.clone());
                }
                state if state.is_available() && !has_ref => report.missing_refs.push(id.clone()),
                _ => {}
            }
        }
        Ok(())
    }

    fn recover_operations(
        &mut self,
        open_ops: &HashMap<String, OperationState>,
        report: &mut RecoveryReport,
        now_ms: i64,
    ) -> Result<()> {
        let mut ids: Vec<&String> = open_ops.keys().collect();
        ids.sort();
        for id in ids {
            match open_ops[id] {
                OperationState::Applying => {
                    let scope = self
                        .operation(id)?
                        .map(|op| op.record.scope.worktrees)
                        .unwrap_or_default();
                    let worktrees: Vec<Option<&str>> = if scope.is_empty() {
                        vec![None]
                    } else {
                        scope.iter().map(|w| Some(w.as_str())).collect()
                    };
                    let detail = serde_json::json!({ "state": "interrupted" }).to_string();
                    let notices = self.write(|batch| {
                        advance(batch, id, OperationTransition::Interrupted, now_ms)?;
                        worktrees
                            .iter()
                            .map(|w| {
                                notice(
                                    batch,
                                    NoticeKind::Interruption,
                                    *w,
                                    Some(id),
                                    &detail,
                                    now_ms,
                                )
                            })
                            .collect::<Result<Vec<_>>>()
                    })?;
                    report.interrupted_operations.push(id.clone());
                    report.notices.extend(notices);
                }
                OperationState::Intent | OperationState::PriorSnapshot | OperationState::Ready => {
                    self.advance_operation(
                        id,
                        OperationTransition::Aborted {
                            reason: "recovered-at-start",
                        },
                        now_ms,
                    )?;
                    report.aborted_operations.push(id.clone());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn release_locks(
        &mut self,
        lock_ops: &HashSet<String>,
        options: &RecoveryOptions<'_>,
        report: &mut RecoveryReport,
        now_ms: i64,
    ) -> Result<()> {
        let taken = entries_of(&self.conn, "lock-taken")?;
        let released = entries_of(&self.conn, "lock-released")?;
        let started = entries_of(&self.conn, "child-started")?;
        let ended = entries_of(&self.conn, "child-ended")?;
        let ended: HashSet<(Option<String>, Option<i64>)> =
            ended.into_iter().map(|e| (e.subject_id, e.pid)).collect();

        for lock in taken {
            let (Some(op), Some(path)) = (lock.subject_id.clone(), lock.path.clone()) else {
                continue;
            };
            if !lock_ops.contains(&op) {
                continue;
            }
            let already = released.iter().any(|r| {
                r.seq > lock.seq
                    && r.subject_id.as_deref() == Some(&op)
                    && r.path.as_deref() == Some(&path)
            });
            if already {
                continue;
            }
            let path = PathBuf::from(path);
            if self.tampered.contains(&lock.seq) {
                report.kept_locks.push(KeptLock {
                    path,
                    reason: KeptLockReason::Tampered,
                });
                continue;
            }

            // Wait, within the shared deadline, for annotated children.
            let children: Vec<u32> = started
                .iter()
                .filter(|c| c.subject_id.as_deref() == Some(&op))
                .filter(|c| !ended.contains(&(c.subject_id.clone(), c.pid)))
                .filter_map(|c| c.pid.and_then(|p| u32::try_from(p).ok()))
                .collect();
            let mut alive: Vec<u32> = children.clone();
            loop {
                alive.retain(|pid| options.probe.is_alive(*pid));
                if alive.is_empty() || Instant::now() >= options.deadline {
                    break;
                }
                std::thread::sleep(options.poll);
            }
            for pid in children.iter().filter(|p| !alive.contains(p)) {
                self.record_child_ended(&op, *pid, now_ms)?;
            }
            if !alive.is_empty() {
                report.kept_locks.push(KeptLock {
                    path,
                    reason: KeptLockReason::ChildAlive,
                });
                continue;
            }

            let inode = lock.inode.and_then(|i| u64::try_from(i).ok());
            match release_own_lock(options.git_dir, &path, inode) {
                Ok(LockOutcome::Released) => {
                    self.record_lock_released(&op, &path, now_ms)?;
                    report.released_locks.push(path);
                }
                Ok(LockOutcome::Gone) => self.record_lock_released(&op, &path, now_ms)?,
                Ok(LockOutcome::Kept(reason)) => report.kept_locks.push(KeptLock { path, reason }),
                Err(_) => report.kept_locks.push(KeptLock {
                    path,
                    reason: KeptLockReason::Io,
                }),
            }
        }
        Ok(())
    }
}

/// Result of trying to release an annotated lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockOutcome {
    Released,
    /// No file there any more: nothing to do.
    Gone,
    Kept(KeptLockReason),
}

/// Identity of a file, to annotate a lock when it is taken. `None` where
/// the OS gives no stable identity through std (Windows: pending).
pub fn file_inode(path: &Path) -> io::Result<Option<u64>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(std::fs::symlink_metadata(path)?.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::symlink_metadata(path)?;
        Ok(None)
    }
}

/// The only write recovery makes in the user's repo: deletes the lock at
/// `path` if, and only if, it is a regular file named `*.lock`, inside
/// `git_dir`, with the inode the journal recorded. The check and the
/// deletion go through a descriptor of the parent folder and never follow a
/// symbolic link (SEC-TMC-04).
pub(crate) fn release_own_lock(
    git_dir: &Path,
    path: &Path,
    inode: Option<u64>,
) -> io::Result<LockOutcome> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(LockOutcome::Kept(KeptLockReason::NotALockFile));
    };
    if !name.ends_with(".lock") || name == ".lock" {
        return Ok(LockOutcome::Kept(KeptLockReason::NotALockFile));
    }
    let Some(parent) = path.parent().filter(|p| p.is_absolute()) else {
        return Ok(LockOutcome::Kept(KeptLockReason::OutsideGitDir));
    };
    let (Ok(parent), Ok(git_dir)) = (parent.canonicalize(), git_dir.canonicalize()) else {
        return Ok(LockOutcome::Gone);
    };
    if !parent.starts_with(&git_dir) {
        return Ok(LockOutcome::Kept(KeptLockReason::OutsideGitDir));
    }
    let Some(inode) = inode else {
        return Ok(LockOutcome::Kept(KeptLockReason::Unsupported));
    };
    unlink_if_same(&parent, name, inode)
}

#[cfg(unix)]
fn unlink_if_same(parent: &Path, name: &str, inode: u64) -> io::Result<LockOutcome> {
    use rustix::fs::{AtFlags, Mode, OFlags};
    let dir = rustix::fs::open(
        parent,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let stat = match rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(LockOutcome::Gone),
        Err(err) => return Err(err.into()),
    };
    #[allow(clippy::unnecessary_cast)]
    let mode = stat.st_mode as u32;
    if mode & 0o170000 != 0o100000 {
        return Ok(LockOutcome::Kept(KeptLockReason::NotALockFile));
    }
    #[allow(clippy::unnecessary_cast)]
    if stat.st_ino as u64 != inode {
        return Ok(LockOutcome::Kept(KeptLockReason::InodeChanged));
    }
    match rustix::fs::unlinkat(&dir, name, AtFlags::empty()) {
        Ok(()) => Ok(LockOutcome::Released),
        Err(rustix::io::Errno::NOENT) => Ok(LockOutcome::Gone),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(unix))]
fn unlink_if_same(_parent: &Path, _name: &str, _inode: u64) -> io::Result<LockOutcome> {
    // Pending: cross-platform validation stage (Windows file identity).
    Ok(LockOutcome::Kept(KeptLockReason::Unsupported))
}
