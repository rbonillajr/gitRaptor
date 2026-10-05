//! State of the worktrees of an observed repo (US-GRP-001).
//!
//! Every read goes through the read-only layer of `crates/git` (ADR-GRP-009):
//! no write, lock or user program touches the repo. [`reconcile`] reads
//! every worktree from scratch (ADR-GRP-010 § 6) and never touches the
//! profile: the daemon loop persists and publishes what it returns. The
//! watcher that keeps it fresh is [`crate::watch`] (US-GRP-002).

use std::path::{Path, PathBuf};

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    ChangeAreaView, ChangeCounts, ChangeKindView, FileChangeView, HeadView,
    MAX_WORKTREE_CHANGE_BYTES, MAX_WORKTREE_CHANGES, RepoRejection, UnavailableReason,
    WorktreeStatus, WorktreeView,
};
use gitraptor_git::{ChangeKind, ReadError, ReaderOptions, RepoReader, Status};
use sha2::{Digest, Sha256};

use crate::profile::{KnownState, WriteOp};

/// Finds the Git common directory of the repo or worktree at `path`, which
/// must be its root or its Git directory: nothing is searched upwards.
pub fn locate(path: &Path) -> Result<PathBuf, RepoRejection> {
    let reader = RepoReader::open(path, &ReaderOptions::default()).map_err(|err| match err {
        ReadError::Untrusted(_) => RepoRejection::Untrusted,
        ReadError::InvalidInput(_) | ReadError::NotARepository(_) => RepoRejection::NotARepo,
        ReadError::Unavailable(_) | ReadError::TemporarilyUnavailable(_) => {
            RepoRejection::Unreadable
        }
    })?;
    Ok(canonical(reader.common_dir()))
}

pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// One worktree as read, with what the store keeps of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRead {
    pub view: WorktreeView,
    /// Commit `HEAD` resolves to.
    pub head_commit: Option<String>,
    /// Digest of the full, untruncated status.
    pub fingerprint: Option<String>,
    /// An operation (rebase, merge…) is in progress: `HEAD` may be detached
    /// without the developer having switched branch.
    pub in_progress: bool,
}

/// The worktrees of one repo after a full reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRead {
    pub worktrees: Vec<WorktreeRead>,
    /// Tips of the local branches, one `name commit` per line, sorted.
    pub refs: String,
}

impl RepoRead {
    pub fn views(&self) -> Vec<WorktreeView> {
        self.worktrees.iter().map(|w| w.view.clone()).collect()
    }

    /// Store writes that record this reconciliation, the base US-GRP-002
    /// compares against: every worktree seen, its last known state, and
    /// the worktrees the store knew that are gone.
    pub fn store_ops(&self, known: &[PathBuf], now_ms: i64) -> Vec<WriteOp> {
        let mut ops = Vec::new();
        let seen: Vec<PathBuf> = self
            .worktrees
            .iter()
            .map(|w| PathBuf::from(w.view.path.raw()))
            .collect();
        for (read, path) in self.worktrees.iter().zip(&seen) {
            if !matches!(read.view.status, WorktreeStatus::Ready { .. }) {
                continue;
            }
            ops.push(WriteOp::UpsertWorktree {
                path: path.clone(),
                admin_name: read.view.admin_name.as_ref().map(|n| n.raw().to_owned()),
                seen_ms: now_ms,
            });
            ops.push(WriteOp::SetLastKnownState {
                worktree: path.clone(),
                state: KnownState {
                    head: read.head_commit.clone(),
                    refs: self.refs.clone(),
                    operation: None,
                    dirty_fingerprint: read.fingerprint.clone(),
                    updated_ms: now_ms,
                },
            });
        }
        for gone in known.iter().filter(|k| !seen.contains(k)) {
            ops.push(WriteOp::MarkWorktreeGone {
                path: gone.clone(),
                gone_ms: now_ms,
            });
        }
        ops
    }
}

/// Reads every worktree of the repo whose common directory is `common_dir`:
/// the main one first (none for a bare repo), then the linked ones by
/// path. One worktree that cannot be read is reported unavailable and the
/// others still are read (BR-EDGE-001); `Err` only if the repo itself
/// cannot be read.
pub fn reconcile(common_dir: &Path) -> Result<RepoRead, ReadError> {
    let reader = RepoReader::open(common_dir, &ReaderOptions::default())?;
    let mut worktrees = Vec::new();
    if !reader.is_bare()
        && let Some(main) = reader.workdir()
    {
        worktrees.push(read_worktree(&canonical(&main), true, None));
    }
    let mut linked: Vec<(PathBuf, String)> = reader
        .worktrees()?
        .into_iter()
        .map(|w| (canonical(&w.path), w.id))
        .collect();
    linked.sort();
    let common = canonical(common_dir);
    for (path, id) in &linked {
        // A missing folder is reported missing, not untrusted.
        if !path.exists() || linked_is_trusted(&common, id, path) {
            worktrees.push(read_worktree(path, false, Some(id)));
        } else {
            worktrees.push(untrusted_link(path, id));
        }
    }
    let mut tips: Vec<String> = reader
        .local_branches()?
        .into_iter()
        .map(|b| format!("{} {}", b.name, b.commit))
        .collect();
    tips.sort();
    Ok(RepoRead {
        worktrees,
        refs: tips.join("\n"),
    })
}

/// Whether a linked worktree may be read and watched (SEC-11, ADR-GRP-010
/// § 2): its `.git` file points back to `<common>/worktrees/<id>`, and its
/// root is not `/`, a drive root, the home folder or an ancestor of the
/// repo. Its `gitdir` is writable by an agent; this keeps the engine from
/// being pointed at the whole disk.
pub fn linked_is_trusted(common_dir: &Path, id: &str, root: &Path) -> bool {
    if root.parent().is_none() || common_dir.starts_with(root) {
        return false;
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
        && canonical(Path::new(&home)) == root
    {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(root.join(".git")) else {
        return false;
    };
    let Some(target) = text.trim().strip_prefix("gitdir:") else {
        return false;
    };
    let target = Path::new(target.trim());
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        root.join(target)
    };
    canonical(&target) == canonical(&common_dir.join("worktrees").join(id))
}

fn untrusted_link(path: &Path, id: &str) -> WorktreeRead {
    WorktreeRead {
        view: WorktreeView {
            path: Untrusted::from_os(path.as_os_str()),
            main: false,
            admin_name: Some(Untrusted::new(id)),
            status: WorktreeStatus::Unavailable {
                reason: UnavailableReason::Untrusted,
            },
        },
        head_commit: None,
        fingerprint: None,
        in_progress: false,
    }
}

/// Reads one worktree from scratch: `HEAD`, the full status and whether an
/// operation is in progress. Never fails: what cannot be read is reported
/// unavailable with its reason.
pub fn read_worktree(path: &Path, main: bool, admin_name: Option<&str>) -> WorktreeRead {
    let view = |status| WorktreeView {
        path: Untrusted::from_os(path.as_os_str()),
        main,
        admin_name: admin_name.map(Untrusted::new),
        status,
    };
    let read = || -> Result<(HeadView, Option<String>, Status, bool), ReadError> {
        let reader = RepoReader::open(path, &ReaderOptions::default())?;
        let head = reader.head()?;
        let status = reader.status()?;
        let in_progress = reader.in_progress().is_some();
        let name = || Untrusted::new(head.branch.clone().unwrap_or_default());
        let head_view = if head.detached {
            HeadView::Detached
        } else if head.unborn {
            HeadView::Unborn { name: name() }
        } else {
            HeadView::Branch { name: name() }
        };
        Ok((head_view, head.commit, status, in_progress))
    };
    match read() {
        Ok((head, head_commit, status, in_progress)) => {
            let (counts, changes) = changes(&status);
            WorktreeRead {
                fingerprint: Some(fingerprint(&changes)),
                view: view(WorktreeStatus::Ready {
                    head,
                    counts,
                    changes: bounded(changes),
                }),
                head_commit,
                in_progress,
            }
        }
        Err(err) => WorktreeRead {
            view: view(WorktreeStatus::Unavailable {
                reason: match err {
                    ReadError::Untrusted(_) => UnavailableReason::Untrusted,
                    _ if !path.exists() => UnavailableReason::Missing,
                    _ => UnavailableReason::Unreadable,
                },
            }),
            head_commit: None,
            fingerprint: None,
            in_progress: false,
        },
    }
}

/// Every change, sorted by path and area, and the counts by area.
fn changes(status: &Status) -> (ChangeCounts, Vec<FileChangeView>) {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let counts = ChangeCounts {
        staged: count(status.staged.len()),
        unstaged: count(status.unstaged.len()),
        untracked: count(status.untracked.len()),
    };
    let change = |path: &str, area, kind| FileChangeView {
        path: Untrusted::new(path),
        area,
        kind,
    };
    let mut all: Vec<FileChangeView> = status
        .staged
        .iter()
        .map(|c| change(&c.path, ChangeAreaView::Staged, kind_view(c.kind)))
        .chain(
            status
                .unstaged
                .iter()
                .map(|c| change(&c.path, ChangeAreaView::Unstaged, kind_view(c.kind))),
        )
        .chain(
            status
                .untracked
                .iter()
                .map(|p| change(p, ChangeAreaView::Untracked, ChangeKindView::Added)),
        )
        .collect();
    all.sort_by(|a, b| (a.path.raw(), a.area).cmp(&(b.path.raw(), b.area)));
    (counts, all)
}

/// The first changes that fit both per-worktree bounds.
fn bounded(mut changes: Vec<FileChangeView>) -> Vec<FileChangeView> {
    let mut bytes = 0;
    let keep = changes
        .iter()
        .take(MAX_WORKTREE_CHANGES)
        .take_while(|c| {
            bytes += c.path.raw().len();
            bytes <= MAX_WORKTREE_CHANGE_BYTES
        })
        .count();
    changes.truncate(keep);
    changes
}

fn kind_view(kind: ChangeKind) -> ChangeKindView {
    match kind {
        ChangeKind::Added => ChangeKindView::Added,
        ChangeKind::Deleted => ChangeKindView::Deleted,
        ChangeKind::Modified => ChangeKindView::Modified,
        ChangeKind::TypeChanged => ChangeKindView::TypeChanged,
        ChangeKind::Conflicted => ChangeKindView::Conflicted,
    }
}

/// Digest of the full change list: it changes whenever the list does.
fn fingerprint(changes: &[FileChangeView]) -> String {
    let mut hasher = Sha256::new();
    for c in changes {
        hasher.update(c.path.raw().as_bytes());
        hasher.update([0, c.area as u8, c.kind as u8]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Drops the change lists of every worktree, keeping the counts: what a
/// message too large for the channel sends instead.
pub fn without_change_lists(worktrees: &mut [WorktreeView]) {
    for w in worktrees {
        if let WorktreeStatus::Ready { changes, .. } = &mut w.status {
            changes.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str) -> FileChangeView {
        FileChangeView {
            path: Untrusted::new(path),
            area: ChangeAreaView::Unstaged,
            kind: ChangeKindView::Modified,
        }
    }

    #[test]
    fn change_lists_are_bounded_by_count_and_bytes() {
        let many: Vec<_> = (0..500).map(|i| change(&format!("f{i:03}"))).collect();
        assert_eq!(bounded(many).len(), MAX_WORKTREE_CHANGES);
        let long = "x".repeat(4000);
        let big: Vec<_> = (0..20).map(|i| change(&format!("{long}{i}"))).collect();
        let kept = bounded(big);
        assert_eq!(kept.len(), MAX_WORKTREE_CHANGE_BYTES / 4002);
    }

    #[test]
    fn fingerprint_follows_the_full_list() {
        let a = vec![change("a"), change("b")];
        let b = vec![change("a")];
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert_eq!(fingerprint(&a), fingerprint(&a.clone()));
    }
}
