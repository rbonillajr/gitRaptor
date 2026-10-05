//! The facts of a worktree a plan is built on and compared against (ADR-CKP-002 § 1, § 2 and
//! § 5), and the common preconditions in their order (Q-MCP-30).

use std::path::{Path, PathBuf};

use gitraptor_api::catalog::{OperationClass, OperationId, RejectReason, entry};
use gitraptor_git::preflight::{FileId, preflight};
use serde::Serialize;

/// What the executor knows of the worktree and its repo. It enters the plan's fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoFacts {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub linked: bool,
    /// `(dev, inode)` of the root and of its `.git` entry.
    pub root_id: Option<(u64, u64)>,
    pub dot_git_id: Option<(u64, u64)>,
    pub gitdir_linked_back: bool,
    pub head_branch: Option<String>,
    pub head_commit: Option<String>,
    pub detached: bool,
    pub in_progress: Option<String>,
    pub git_locks: Vec<String>,
    pub locked: bool,
    pub grafts: bool,
    pub branches_elsewhere: Vec<String>,
}

fn id(f: Option<FileId>) -> Option<(u64, u64)> {
    f.map(|f| (f.dev, f.ino))
}

impl RepoFacts {
    /// Reads the facts of the worktree rooted at `worktree` with the read layer of `crates/git`.
    /// A repo that cannot be read, or is not trusted, counts as a changed identity.
    pub fn read(worktree: &Path) -> Result<Self, RejectReason> {
        let p = preflight(worktree).map_err(|_| RejectReason::RepoIdentityChanged)?;
        Ok(Self {
            root: p.root,
            git_dir: p.git_dir,
            common_dir: p.common_dir,
            linked: p.linked,
            root_id: id(p.root_id),
            dot_git_id: id(p.dot_git_id),
            gitdir_linked_back: p.gitdir_linked_back,
            head_branch: p.head.branch,
            head_commit: p.head.commit,
            detached: p.head.detached,
            in_progress: p.in_progress.map(|s| format!("{s:?}")),
            git_locks: p.git_locks,
            locked: p.locked,
            grafts: p.grafts,
            branches_elsewhere: p.branches_elsewhere,
        })
    }

    /// The identity part (M-05): paths, `(dev, inode)` and the `gitdir` back link.
    pub fn same_identity(&self, other: &Self) -> bool {
        self.root == other.root
            && self.git_dir == other.git_dir
            && self.common_dir == other.common_dir
            && self.root_id == other.root_id
            && self.dot_git_id == other.dot_git_id
            && other.gitdir_linked_back
    }
}

/// The common preconditions of `op`, in order: operation in progress, then detached HEAD, then
/// the rest (Q-MCP-30). Operations that do not write the repo only need a coherent identity.
pub fn check_common(op: OperationId, f: &RepoFacts) -> Result<(), RejectReason> {
    if !f.gitdir_linked_back {
        return Err(RejectReason::RepoIdentityChanged);
    }
    let writes = entry(op).class != OperationClass::NoRepoWrite;
    if !writes {
        return Ok(());
    }
    // Aborting needs the operation in progress; everything else refuses it.
    if op != OperationId::AbortInProgress && f.in_progress.is_some() {
        return Err(RejectReason::OperationInProgress);
    }
    if matches!(op, OperationId::Commit | OperationId::RebaseOntoBase) && f.detached {
        return Err(RejectReason::DetachedHead);
    }
    if !f.git_locks.is_empty() {
        return Err(RejectReason::GitBusy);
    }
    if f.locked {
        return Err(RejectReason::WorktreeLocked);
    }
    if f.head_branch
        .as_ref()
        .is_some_and(|b| f.branches_elsewhere.contains(b))
    {
        return Err(RejectReason::BranchCheckedOutElsewhere);
    }
    if f.grafts {
        return Err(RejectReason::Grafts);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn clean() -> RepoFacts {
        RepoFacts {
            root: "/w/a".into(),
            git_dir: "/w/a/.git".into(),
            common_dir: "/w/a/.git".into(),
            linked: false,
            root_id: Some((1, 2)),
            dot_git_id: Some((1, 3)),
            gitdir_linked_back: true,
            head_branch: Some("main".into()),
            head_commit: Some("0".repeat(40)),
            detached: false,
            in_progress: None,
            git_locks: Vec::new(),
            locked: false,
            grafts: false,
            branches_elsewhere: Vec::new(),
        }
    }

    /// Q-MCP-30: an operation in progress is reported before a detached HEAD.
    #[test]
    fn preconditions_keep_their_order() {
        let mut f = clean();
        f.in_progress = Some("Merge".into());
        f.detached = true;
        f.git_locks = vec!["index.lock".into()];
        assert_eq!(
            check_common(OperationId::Commit, &f),
            Err(RejectReason::OperationInProgress)
        );
        f.in_progress = None;
        assert_eq!(
            check_common(OperationId::Commit, &f),
            Err(RejectReason::DetachedHead)
        );
        // A detached HEAD does not stop a snapshot, nor a worktree from the base.
        assert_eq!(check_common(OperationId::Snapshot, &f), Ok(()));
        assert_eq!(
            check_common(OperationId::CreateWorktree, &f),
            Err(RejectReason::GitBusy)
        );
        f.git_locks.clear();
        f.grafts = true;
        assert_eq!(
            check_common(OperationId::CreateWorktree, &f),
            Err(RejectReason::Grafts)
        );
        let mut g = clean();
        g.in_progress = Some("Rebase".into());
        assert_eq!(check_common(OperationId::AbortInProgress, &g), Ok(()));
        g.gitdir_linked_back = false;
        assert_eq!(
            check_common(OperationId::Snapshot, &g),
            Err(RejectReason::RepoIdentityChanged)
        );
    }

    #[test]
    fn identity_compares_inodes_and_the_back_link() {
        let a = clean();
        let mut b = clean();
        assert!(a.same_identity(&b));
        b.dot_git_id = Some((1, 99));
        assert!(!a.same_identity(&b));
    }
}
