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

    /// Whether `pid` is still the annotated child whose start time, in µs
    /// since the epoch as [`crate::channel::peer::ProcInfo::start_us`], was
    /// `start_us`: a live process with another start time is a reuse of the
    /// pid. Without a start time (rows annotated before it was recorded),
    /// as [`Self::is_alive`].
    fn is_same(&self, pid: u32, start_us: Option<u64>) -> bool {
        let _ = start_us;
        self.is_alive(pid)
    }
}

/// The OS answer. When it cannot tell (no permission, the list cannot be
/// read) it says alive: the lock is then kept, never wrongly deleted. A
/// reused pid is told apart from the child by its start time: Windows by
/// the creation time, Unix by the start time the channel reads
/// ([`crate::channel::peer::SystemProcs`]), compared with [`same_start`].
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

    #[cfg(unix)]
    fn is_same(&self, pid: u32, start_us: Option<u64>) -> bool {
        use crate::channel::peer::{ProcError, ProcSource, SystemProcs};
        let Some(start_us) = start_us else {
            return self.is_alive(pid);
        };
        match SystemProcs.read(pid) {
            Ok(info) => same_start(start_us, info.start_us, UNIX_START_RULE),
            // Linux reports any unreadable `/proc` entry as gone (too many
            // open files, `hidepid`): only `kill(0)` confirms it.
            Err(ProcError::Gone) => self.is_alive(pid),
            Err(_) => true,
        }
    }

    #[cfg(windows)]
    fn is_alive(&self, pid: u32) -> bool {
        !matches!(
            gitraptor_winsys::process::created_100ns(pid),
            Err(gitraptor_winsys::process::Error::Gone)
        )
    }

    #[cfg(windows)]
    fn is_same(&self, pid: u32, start_us: Option<u64>) -> bool {
        let Some(start_us) = start_us else {
            return self.is_alive(pid);
        };
        match gitraptor_winsys::process::created_100ns(pid) {
            Ok(created) => windows_epoch_us(created) == start_us,
            Err(gitraptor_winsys::process::Error::Gone) => false,
            Err(_) => true,
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn is_alive(&self, _pid: u32) -> bool {
        true
    }
}

/// How a stored start time is compared with the one read now.
#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartRule {
    /// The start time is fixed when the process is created (macOS).
    Exact,
    /// Linux: the start time is the boot time (whole seconds, recomputed
    /// from the wall clock, so it moves when the clock is stepped) plus the
    /// ticks since boot. Only the sub-second part, which comes from the
    /// ticks alone, tells two processes apart; equal sub-seconds with other
    /// seconds may be the same child after a clock step, so it is the same
    /// (fail-closed: a reuse within the same hundredth is not detected).
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    SubSecond,
}

#[cfg(target_os = "linux")]
const UNIX_START_RULE: StartRule = StartRule::SubSecond;
#[cfg(all(unix, not(target_os = "linux")))]
const UNIX_START_RULE: StartRule = StartRule::Exact;

/// Whether a process that started at `current` µs may be the one annotated
/// with `stored`; `false` only when it is surely another process.
#[cfg(any(unix, test))]
pub(crate) fn same_start(stored: u64, current: u64, rule: StartRule) -> bool {
    match rule {
        StartRule::Exact => stored == current,
        StartRule::SubSecond => stored % 1_000_000 == current % 1_000_000,
    }
}

/// A process creation time (100 ns since 1601) as µs since the Unix epoch,
/// truncated exactly as the channel's process reader does.
#[cfg(windows)]
fn windows_epoch_us(t_100ns: u64) -> u64 {
    const UNIX_EPOCH_100NS: u64 = 116_444_736_000_000_000;
    t_100ns.saturating_sub(UNIX_EPOCH_100NS) / 10
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
    /// The file there now is not the one the operation took: another inode
    /// or, for a reused inode, another birth time.
    InodeChanged,
    /// Not a regular file named `*.lock`.
    NotALockFile,
    /// Outside the repo's Git directory.
    OutsideGitDir,
    /// The journal entry is at a break of the chain.
    Tampered,
    /// The file's identity cannot be checked: this OS or file system gives
    /// no birth time or no stable index (Windows: anything but NTFS), or the
    /// lock was annotated without one.
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
            let children: Vec<(u32, Option<u64>)> = started
                .iter()
                .filter(|c| c.subject_id.as_deref() == Some(&op))
                .filter(|c| !ended.contains(&(c.subject_id.clone(), c.pid)))
                .filter_map(|c| {
                    let pid = c.pid.and_then(|p| u32::try_from(p).ok())?;
                    Some((pid, child_start_us(c.detail.as_deref())))
                })
                .collect();
            let mut alive = children.clone();
            loop {
                alive.retain(|(pid, start)| options.probe.is_same(*pid, *start));
                if alive.is_empty() || Instant::now() >= options.deadline {
                    break;
                }
                std::thread::sleep(options.poll);
            }
            for (pid, _) in children.iter().filter(|c| !alive.contains(c)) {
                self.record_child_ended(&op, *pid, now_ms)?;
            }
            if !alive.is_empty() {
                report.kept_locks.push(KeptLock {
                    path,
                    reason: KeptLockReason::ChildAlive,
                });
                continue;
            }

            let identity = lock.inode.map(|inode| FileIdentity {
                inode: inode_from_column(inode),
                birth_ns: lock.birth_ns,
            });
            match release_own_lock(options.git_dir, &path, identity) {
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

/// The start time a `child-started` row carries in its detail, if any.
fn child_start_us(detail: Option<&str>) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(detail?)
        .ok()?
        .get("start_us")?
        .as_u64()
}

/// The journal's `inode` column is a signed 64-bit integer: the inode is kept
/// bit for bit, so an NTFS file index with its high bit set (sequence number
/// 0x8000 and above) still fits. Inodes written before were all positive.
pub(crate) fn inode_to_column(inode: u64) -> i64 {
    i64::from_ne_bytes(inode.to_ne_bytes())
}

pub(crate) fn inode_from_column(inode: i64) -> u64 {
    u64::from_ne_bytes(inode.to_ne_bytes())
}

/// Result of trying to release an annotated lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockOutcome {
    #[cfg_attr(not(any(unix, windows)), allow(dead_code))]
    Released,
    /// No file there any more: nothing to do.
    Gone,
    Kept(KeptLockReason),
}

/// Identity of a file: its inode and its birth time. The inode alone is not
/// enough, because file systems such as ext4 hand a freed inode to the next
/// file created, so a foreign lock taken at the same path can get the inode
/// of ours (ADR-TMC-003 § 4). Windows: the NTFS file index, which carries
/// the MFT record's sequence number, and the creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub inode: u64,
    /// Nanoseconds since the epoch; `None` where the file system keeps no
    /// birth time, and then the lock is never released by recovery.
    pub birth_ns: Option<i64>,
}

/// Identity of a file, to annotate a lock when it is taken. Never follows a
/// symbolic link. `None` where the OS gives no stable identity.
pub fn file_identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path)?;
        let birth_ns = meta.created().ok().and_then(|t| {
            let since = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            i64::try_from(since.as_nanos()).ok()
        });
        Ok(Some(FileIdentity {
            inode: meta.ino(),
            birth_ns,
        }))
    }
    #[cfg(windows)]
    {
        let id = gitraptor_winsys::file_id::Entry::open(path, false)?.identity();
        Ok(Some(FileIdentity {
            inode: id.index,
            birth_ns: windows_epoch_ns(id.created_100ns),
        }))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = std::fs::symlink_metadata(path)?;
        Ok(None)
    }
}

/// A Windows file time (100 ns since 1601) as ns since the Unix epoch.
#[cfg(windows)]
fn windows_epoch_ns(t_100ns: i64) -> Option<i64> {
    const UNIX_EPOCH_100NS: i64 = 116_444_736_000_000_000;
    t_100ns.checked_sub(UNIX_EPOCH_100NS)?.checked_mul(100)
}

/// The only write recovery makes in the user's repo: deletes the lock at
/// `path` if, and only if, it is a regular file named `*.lock`, inside
/// `git_dir`, with the identity (inode and birth time) the journal recorded. The check and the
/// deletion go through a descriptor of the parent folder and never follow a
/// symbolic link (SEC-TMC-04).
pub(crate) fn release_own_lock(
    git_dir: &Path,
    path: &Path,
    identity: Option<FileIdentity>,
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
    let Some(identity) = identity else {
        return Ok(LockOutcome::Kept(KeptLockReason::Unsupported));
    };
    unlink_if_same(&parent, name, identity)
}

#[cfg(unix)]
fn unlink_if_same(parent: &Path, name: &str, identity: FileIdentity) -> io::Result<LockOutcome> {
    use rustix::fs::{AtFlags, Mode, OFlags};
    let dir = rustix::fs::open(
        parent,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let Some((mode, inode, birth_ns)) = stat_at(&dir, name)? else {
        return Ok(LockOutcome::Gone);
    };
    if mode & 0o170000 != 0o100000 {
        return Ok(LockOutcome::Kept(KeptLockReason::NotALockFile));
    }
    if inode != identity.inode {
        return Ok(LockOutcome::Kept(KeptLockReason::InodeChanged));
    }
    let (Some(recorded), Some(current)) = (identity.birth_ns, birth_ns) else {
        return Ok(LockOutcome::Kept(KeptLockReason::Unsupported));
    };
    if recorded != current {
        return Ok(LockOutcome::Kept(KeptLockReason::InodeChanged));
    }
    match rustix::fs::unlinkat(&dir, name, AtFlags::empty()) {
        Ok(()) => Ok(LockOutcome::Released),
        Err(rustix::io::Errno::NOENT) => Ok(LockOutcome::Gone),
        Err(err) => Err(err.into()),
    }
}

/// Mode, inode and birth time (ns since the epoch) of `name` in `dir`,
/// without following a symbolic link; `None` if there is no such file.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn stat_at(dir: &impl rustix::fd::AsFd, name: &str) -> io::Result<Option<(u32, u64, Option<i64>)>> {
    use rustix::fs::{AtFlags, StatxFlags};
    let stat = match rustix::fs::statx(
        dir,
        name,
        AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::TYPE | StatxFlags::INO | StatxFlags::BTIME,
    ) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let birth_ns = (stat.stx_mask & StatxFlags::BTIME.bits() != 0)
        .then(|| {
            stat.stx_btime
                .tv_sec
                .checked_mul(1_000_000_000)?
                .checked_add(i64::from(stat.stx_btime.tv_nsec))
        })
        .flatten();
    Ok(Some((u32::from(stat.stx_mode), stat.stx_ino, birth_ns)))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn stat_at(dir: &impl rustix::fd::AsFd, name: &str) -> io::Result<Option<(u32, u64, Option<i64>)>> {
    let stat = match rustix::fs::statat(dir, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    #[allow(clippy::unnecessary_cast)]
    let birth_ns = (stat.st_birthtime as i64)
        .checked_mul(1_000_000_000)
        .and_then(|s| s.checked_add(stat.st_birthtime_nsec as i64));
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let birth_ns = None;
    #[allow(clippy::unnecessary_cast)]
    Ok(Some((stat.st_mode as u32, stat.st_ino as u64, birth_ns)))
}

/// Windows has no `unlinkat`: the lock is opened by its path itself (never
/// following a link or junction) with `DELETE` access, and the handle pins
/// the entry. Only on NTFS, where the file index is stable. What is checked and what is deleted is that one entry,
/// whatever the path names afterwards; it is ours only with the index and
/// creation time the journal recorded. A process still holding it open
/// without `FILE_SHARE_DELETE` makes the open fail, and the lock is kept.
#[cfg(windows)]
fn unlink_if_same(parent: &Path, name: &str, identity: FileIdentity) -> io::Result<LockOutcome> {
    use gitraptor_winsys::file_id::Entry;
    let entry = match Entry::open(&parent.join(name), true) {
        Ok(entry) => entry,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(LockOutcome::Gone),
        Err(err) => return Err(err),
    };
    if !entry.is_regular_file() {
        return Ok(LockOutcome::Kept(KeptLockReason::NotALockFile));
    }
    if !entry.is_ntfs() {
        return Ok(LockOutcome::Kept(KeptLockReason::Unsupported));
    }
    let current = entry.identity();
    if current.index != identity.inode {
        return Ok(LockOutcome::Kept(KeptLockReason::InodeChanged));
    }
    let (Some(recorded), Some(current)) =
        (identity.birth_ns, windows_epoch_ns(current.created_100ns))
    else {
        return Ok(LockOutcome::Kept(KeptLockReason::Unsupported));
    };
    if recorded != current {
        return Ok(LockOutcome::Kept(KeptLockReason::InodeChanged));
    }
    // Marked for deletion through the handle. If another process holds the
    // file open with `FILE_SHARE_DELETE`, the name stays, pending, until it
    // closes: no one can open it any more and Git sees it gone once it does.
    entry.delete()?;
    Ok(LockOutcome::Released)
}

#[cfg(not(any(unix, windows)))]
fn unlink_if_same(_parent: &Path, _name: &str, _identity: FileIdentity) -> io::Result<LockOutcome> {
    Ok(LockOutcome::Kept(KeptLockReason::Unsupported))
}
