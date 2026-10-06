//! What the user-operation executor checks before preparing and again, under the repo's write
//! lock, before running (ADR-CKP-002 § 1 "Precondiciones" and § 5 "Revalidación", M-05).
//!
//! A read: it opens the worktree with gitoxide and looks at marker files with `lstat`. It never
//! writes, never removes a lock and never spawns anything.

use std::path::{Path, PathBuf};

use crate::ReadError;
use crate::reader::{Head, InProgress, ReaderOptions, RepoReader};

/// `(device, inode)` of a file, to notice a replaced `.git` or worktree root (M-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

/// The facts the executor needs about one worktree and its repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    /// Canonical root of the worktree.
    pub root: PathBuf,
    /// Private Git directory of the worktree (the common one for the main worktree).
    pub git_dir: PathBuf,
    /// Common Git directory of the repo.
    pub common_dir: PathBuf,
    /// A linked worktree (`.git` is a file).
    pub linked: bool,
    /// Identity of the root and of its `.git` entry; `None` where the OS gives none.
    pub root_id: Option<FileId>,
    pub dot_git_id: Option<FileId>,
    /// For a linked worktree: its `.git` names a Git directory whose `gitdir` file names the same
    /// `.git` back (SEC-11). Always true for a main worktree.
    pub gitdir_linked_back: bool,
    pub head: Head,
    pub in_progress: Option<InProgress>,
    /// Git lock files present in the scope, relative to their Git directory. Never removed.
    pub git_locks: Vec<String>,
    /// Locked with `git worktree lock`.
    pub locked: bool,
    /// The repo has `info/grafts` (L-04).
    pub grafts: bool,
    /// Branches checked out in the *other* worktrees of the repo.
    pub branches_elsewhere: Vec<String>,
}

#[cfg(unix)]
fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| FileId {
        dev: m.dev(),
        ino: m.ino(),
    })
}

/// Pendiente: etapa de validación multiplataforma (file index on Windows).
#[cfg(not(unix))]
fn file_id(_path: &Path) -> Option<FileId> {
    None
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// The branch a Git directory's `HEAD` names, read as text (no symlink following).
fn head_branch(git_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    text.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Reads the preflight facts of the worktree rooted at `worktree` (no upward discovery).
pub fn preflight(worktree: &Path) -> Result<Preflight, ReadError> {
    crate::paths::validate(worktree)?;
    let root = crate::paths::canonicalize(worktree)
        .map_err(|e| ReadError::Unavailable(format!("worktree root: {e}")))?;
    let dot_git = root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git)
        .map_err(|_| ReadError::NotARepository("no .git in the worktree root".into()))?;
    if meta.file_type().is_symlink() {
        return Err(ReadError::Untrusted(".git is a symbolic link".into()));
    }
    let linked = meta.is_file();

    let reader = RepoReader::open(&root, &ReaderOptions::default())?;
    let canonical = |p: &Path| crate::paths::canonicalize(p).unwrap_or_else(|_| p.to_owned());
    let git_dir = canonical(reader.repo.git_dir());
    let common_dir = canonical(reader.repo.common_dir());

    let gitdir_linked_back = if linked {
        let named = std::fs::read_to_string(&dot_git)
            .ok()
            .and_then(|t| t.trim().strip_prefix("gitdir: ").map(PathBuf::from));
        let back = named
            .as_deref()
            .and_then(|d| std::fs::read_to_string(d.join("gitdir")).ok())
            .map(|t| PathBuf::from(t.trim()));
        match (named, back) {
            (Some(named), Some(back)) => same_path(&named, &git_dir) && same_path(&back, &dot_git),
            _ => false,
        }
    } else {
        true
    };

    let head = reader.head()?;
    let in_progress = reader.in_progress();

    let mut git_locks = Vec::new();
    for (dir, name) in [
        (&git_dir, "index.lock"),
        (&git_dir, "HEAD.lock"),
        (&common_dir, "packed-refs.lock"),
        (&common_dir, "config.lock"),
    ] {
        if exists(&dir.join(name)) {
            git_locks.push(name.to_owned());
        }
    }
    if let Some(branch) = &head.branch {
        let rel = format!("refs/heads/{branch}.lock");
        if exists(&common_dir.join(&rel)) {
            git_locks.push(rel);
        }
    }

    let locked = linked && exists(&git_dir.join("locked"));
    let grafts = exists(&common_dir.join("info").join("grafts"));

    let mut branches_elsewhere = Vec::new();
    if git_dir != common_dir
        && let Some(b) = head_branch(&common_dir)
    {
        branches_elsewhere.push(b);
    }
    for wt in reader.worktrees()? {
        if same_path(&wt.git_dir, &git_dir) {
            continue;
        }
        if let Some(b) = head_branch(&wt.git_dir) {
            branches_elsewhere.push(b);
        }
    }

    Ok(Preflight {
        root_id: file_id(&root),
        dot_git_id: file_id(&dot_git),
        root,
        git_dir,
        common_dir,
        linked,
        gitdir_linked_back,
        head,
        in_progress,
        git_locks,
        locked,
        grafts,
        branches_elsewhere,
    })
}
