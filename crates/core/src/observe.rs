//! State of the worktrees of an observed repo (US-GRP-001).
//!
//! Every read goes through the read-only layer of `crates/git` (ADR-GRP-009):
//! no write, lock or user program touches the repo. [`reconcile`] reads
//! every worktree from scratch (ADR-GRP-010 § 6) and never touches the
//! profile: the daemon loop persists and publishes what it returns. The
//! watcher, the debounce and the periodic reconciliation belong to
//! US-GRP-002 (ADR-GRP-010 § 5, ADR-GRP-013 § 5).
//!
//! The ahead/behind of each worktree against the base branch of its repo
//! (US-GRP-012) is counted in the reconciliation and again in every
//! `engine.snapshot` ([`refresh_divergence`]), with the base branch as it is
//! then and the `HEAD` commits of the last reconciliation.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeAreaView, ChangeCounts, ChangeKindView, CommitCountView,
    DivergenceView, FileChangeView, HeadView, MAX_DIVERGENCE_WALK, MAX_WORKTREE_CHANGE_BYTES,
    MAX_WORKTREE_CHANGES, RepoRejection, RepoView, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_git::{ChangeKind, Count, ReadError, ReaderOptions, RefName, RepoReader, Status};
use gitraptor_policy::team::{BaseBranch, BaseStatus, Confirmed, DEFAULT_BRANCH};
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

fn canonical(path: &Path) -> PathBuf {
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
}

impl WorktreeRead {
    /// What its ahead/behind is counted from.
    pub fn head_ref(&self) -> HeadRef {
        let WorktreeStatus::Ready { head, .. } = &self.view.status else {
            return HeadRef::None;
        };
        match (head, &self.head_commit) {
            (HeadView::Branch { name }, Some(commit)) => HeadRef::Branch {
                name: name.raw().to_owned(),
                commit: commit.clone(),
            },
            (HeadView::Detached, Some(commit)) => HeadRef::Detached(commit.clone()),
            _ => HeadRef::None,
        }
    }
}

/// What the ahead/behind of a worktree is counted from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadRef {
    /// No commit: a branch without commits, or a worktree not read.
    None,
    /// On branch `name`, at `commit` when read. The snapshot follows the
    /// branch, the one its row names.
    Branch { name: String, commit: String },
    /// Directly at a commit.
    Detached(String),
}

/// The worktrees of one repo after a full reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRead {
    pub common_dir: PathBuf,
    pub worktrees: Vec<WorktreeRead>,
    /// Tips of the local branches, one `name commit` per line, sorted.
    pub refs: String,
    /// The base branch the ahead/behind was counted against.
    pub base: BaseBranch,
}

impl RepoRead {
    pub fn views(&self) -> Vec<WorktreeView> {
        self.worktrees.iter().map(|w| w.view.clone()).collect()
    }

    /// Counts the ahead/behind again against `base` (the one the profile
    /// keeps for this repo) if it is not the one it was counted against.
    pub fn set_base(&mut self, base: BaseBranch) {
        if base == self.base {
            return;
        }
        let heads: Vec<HeadRef> = self.worktrees.iter().map(WorktreeRead::head_ref).collect();
        let found = divergences(&self.common_dir, &base, &heads, false, None);
        for (w, d) in self.worktrees.iter_mut().zip(found) {
            set_divergence(&mut w.view, d);
        }
        self.base = base;
    }

    /// What [`refresh_divergence`] needs to count this repo again.
    pub fn divergence_inputs(&self) -> DivergenceInputs {
        DivergenceInputs {
            common_dir: self.common_dir.clone(),
            base: self.base.clone(),
            heads: self.worktrees.iter().map(WorktreeRead::head_ref).collect(),
        }
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
pub fn reconcile(common_dir: &Path, base: &BaseBranch) -> Result<RepoRead, ReadError> {
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
    for (path, id) in &linked {
        worktrees.push(read_worktree(path, false, Some(id)));
    }
    let mut tips: Vec<String> = reader
        .local_branches()?
        .into_iter()
        .map(|b| format!("{} {}", b.name, b.commit))
        .collect();
    tips.sort();
    let heads: Vec<HeadRef> = worktrees.iter().map(WorktreeRead::head_ref).collect();
    for (w, d) in worktrees
        .iter_mut()
        .zip(divergences(common_dir, base, &heads, false, None))
    {
        set_divergence(&mut w.view, d);
    }
    Ok(RepoRead {
        common_dir: common_dir.to_path_buf(),
        worktrees,
        refs: tips.join("\n"),
        base: base.clone(),
    })
}

/// The base branch of a repo (US-GRP-012, ADR-GRD-004 § 3.5): the one the
/// developer confirmed, kept in its store; until then `main`, unconfirmed.
/// US-GRP-016 resolves the unconfirmed one from the team settings here.
pub fn base_branch(confirmed: Option<&Confirmed>) -> BaseBranch {
    match confirmed {
        Some(c) => BaseBranch {
            name: Some(c.base_branch.clone()),
            status: BaseStatus::Confirmed,
        },
        None => BaseBranch {
            name: Some(RefName::new(DEFAULT_BRANCH).expect("the default branch name is valid")),
            status: BaseStatus::Unconfirmed,
        },
    }
}

/// The contract view of a base branch.
pub fn base_view(base: &BaseBranch) -> BaseBranchView {
    BaseBranchView {
        name: base.name.as_ref().map(|n| Untrusted::new(n.as_str())),
        status: match base.status {
            BaseStatus::Confirmed => BaseStatusView::Confirmed,
            BaseStatus::Unconfirmed => BaseStatusView::Unconfirmed,
            BaseStatus::Invalid => BaseStatusView::Invalid,
        },
    }
}

/// What the snapshot keeps of a repo to count its ahead/behind again: what
/// `HEAD` of each worktree was in the last reconciliation, in the order of
/// its worktree views, so a row never counts for a `HEAD` it does not show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivergenceInputs {
    pub common_dir: PathBuf,
    pub base: BaseBranch,
    pub heads: Vec<HeadRef>,
}

/// Counts already walked, by `(head, base)` commit pair. A commit id fixes
/// its whole history, so an entry is valid for any repo and never stale.
#[derive(Debug, Default)]
pub struct DivergenceCache {
    entries: Mutex<HashMap<(String, String), (Count, Count)>>,
}

/// Pairs the cache keeps before it starts again.
pub const DIVERGENCE_CACHE_CAPACITY: usize = 256;

impl DivergenceCache {
    fn count(
        &self,
        reader: &RepoReader,
        head: &str,
        tip: &str,
    ) -> Result<(Count, Count), ReadError> {
        let key = (head.to_owned(), tip.to_owned());
        if let Some(hit) = self.lock().get(&key) {
            return Ok(*hit);
        }
        let counts = reader.ahead_behind_commits(head, tip, MAX_DIVERGENCE_WALK)?;
        let mut entries = self.lock();
        if entries.len() >= DIVERGENCE_CACHE_CAPACITY {
            entries.clear();
        }
        entries.insert(key, counts);
        Ok(counts)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), (Count, Count)>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Counts the ahead/behind of every worktree of a repo against the base
/// branch as it is now (DS-US-GRP-012 D5). A worktree on a branch is
/// counted from that branch as it is now; a detached one, from the commit
/// of the last reconciliation. Read only: no event, no profile.
pub fn refresh_divergence(
    repos: &mut [RepoView],
    inputs: &BTreeMap<String, DivergenceInputs>,
    cache: &DivergenceCache,
) {
    for repo in repos {
        let Some(input) = inputs.get(&repo.repo_id) else {
            continue;
        };
        if input.heads.len() != repo.worktrees.len() {
            continue;
        }
        let found = divergences(
            &input.common_dir,
            &input.base,
            &input.heads,
            true,
            Some(cache),
        );
        for (w, d) in repo.worktrees.iter_mut().zip(found) {
            set_divergence(w, d);
        }
    }
}

/// The ahead/behind of each `HEAD` against the tip of the base branch:
/// `refs/heads/<base>` and nothing else (Q42). With `follow_branches`, a
/// `HEAD` on a branch counts from the branch's tip as it is now.
fn divergences(
    common_dir: &Path,
    base: &BaseBranch,
    heads: &[HeadRef],
    follow_branches: bool,
    cache: Option<&DivergenceCache>,
) -> Vec<DivergenceView> {
    let all = |d: DivergenceView| vec![d; heads.len()];
    let Some(name) = &base.name else {
        return all(DivergenceView::NoBase);
    };
    let Ok(reader) = RepoReader::open(common_dir, &ReaderOptions::default()) else {
        return all(DivergenceView::Unreadable);
    };
    let branch_tip = |name: &str| {
        RefName::new(&format!("refs/heads/{name}")).and_then(|r| reader.resolve_ref(&r))
    };
    let tip = match branch_tip(name.as_str()) {
        Ok(Some(tip)) => tip,
        Ok(None) => return all(DivergenceView::BaseMissing),
        Err(_) => return all(DivergenceView::Unreadable),
    };
    let fresh = DivergenceCache::default();
    let cache = cache.unwrap_or(&fresh);
    heads
        .iter()
        .map(|head| {
            let commit = match head {
                HeadRef::None => return DivergenceView::NoCommits,
                HeadRef::Detached(commit) => commit.clone(),
                // A branch gone since the reconciliation keeps its commit.
                HeadRef::Branch { name, commit } => match follow_branches {
                    true => branch_tip(name).ok().flatten().unwrap_or(commit.clone()),
                    false => commit.clone(),
                },
            };
            match cache.count(&reader, &commit, &tip) {
                Ok((ahead, behind)) => DivergenceView::Counted {
                    ahead: count_view(ahead),
                    behind: count_view(behind),
                },
                Err(_) => DivergenceView::Unreadable,
            }
        })
        .collect()
}

fn count_view(count: Count) -> CommitCountView {
    match count {
        Count::Exact(count) => CommitCountView { count, exact: true },
        Count::AtLeast(count) => CommitCountView {
            count,
            exact: false,
        },
    }
}

/// Sets the ahead/behind of a worktree that could be read.
fn set_divergence(view: &mut WorktreeView, found: DivergenceView) {
    if let WorktreeStatus::Ready { divergence, .. } = &mut view.status {
        *divergence = found;
    }
}

fn read_worktree(path: &Path, main: bool, admin_name: Option<&str>) -> WorktreeRead {
    let view = |status| WorktreeView {
        path: Untrusted::from_os(path.as_os_str()),
        main,
        admin_name: admin_name.map(Untrusted::new),
        status,
    };
    let read = || -> Result<(HeadView, Option<String>, Status), ReadError> {
        let reader = RepoReader::open(path, &ReaderOptions::default())?;
        let head = reader.head()?;
        let status = reader.status()?;
        let name = || Untrusted::new(head.branch.clone().unwrap_or_default());
        let head_view = if head.detached {
            HeadView::Detached
        } else if head.unborn {
            HeadView::Unborn { name: name() }
        } else {
            HeadView::Branch { name: name() }
        };
        Ok((head_view, head.commit, status))
    };
    match read() {
        Ok((head, head_commit, status)) => {
            let (counts, changes) = changes(&status);
            WorktreeRead {
                fingerprint: Some(fingerprint(&changes)),
                view: view(WorktreeStatus::Ready {
                    head,
                    counts,
                    changes: bounded(changes),
                    // Counted by `reconcile` once every head is read.
                    divergence: DivergenceView::Unreadable,
                }),
                head_commit,
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
