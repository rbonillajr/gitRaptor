//! Recreating a deleted linked worktree without checkout and without `git worktree add`, which is
//! porcelain (SEC-TMC-02). The administrative folder is written by hand, as Git lays it out, and
//! the worktree's `.git` file is written last: it is the commit point. If the process dies before
//! it, a stray administrative folder is left that `git worktree prune` cleans.
//!
//! A worktree is only recreated at its original path if it is absolute, does not exist or is an
//! empty folder, its parent exists, and it lies outside the profile and outside every `.git`
//! (SEC-TMC-04). Files and index come afterwards through [`super::files`] and [`super::index`].
//!
//! Nothing between the check and the write goes by path (H-01 of the security review of
//! US-TMC-009): `.git/worktrees`, the administrative folder and the target are opened beneath an
//! open folder, never through a link or a reparse point, and the `.git` file is written through
//! the target's own handle. If the recreation fails halfway, the administrative folder it made is
//! removed, and nothing outside it.

use std::path::{Component, Path};

use super::worktree::{HeadValue, WriteWorktree};
use super::{Result, WriteError};

/// Whether `id` can name `worktrees/<id>`: `[A-Za-z0-9._-]`, 1 to 64, not starting with `.`.
pub fn is_worktree_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Points of [`recreate_with`] where a test can act, as a concurrent process would.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    TargetChecked,
    BeforeCommit,
}

/// Recreates worktree `id` of `main` (the main worktree) at `path`, with `HEAD` set to `head`.
pub fn recreate(
    main: &WriteWorktree,
    id: &str,
    path: &Path,
    head: &HeadValue,
    profile_root: &Path,
) -> Result<WriteWorktree> {
    recreate_with(main, id, path, head, profile_root, &|_| {})
}

#[doc(hidden)]
pub fn recreate_with(
    main: &WriteWorktree,
    id: &str,
    path: &Path,
    head: &HeadValue,
    profile_root: &Path,
    hook: &dyn Fn(Stage),
) -> Result<WriteWorktree> {
    if !is_worktree_id(id) {
        return reject("invalid id");
    }
    crate::paths::validate(path)?;
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return reject("path not absolute");
    }
    if path.components().any(
        |c| matches!(c, Component::Normal(n) if super::tree_path::is_dotgit(n.as_encoded_bytes())),
    ) {
        return reject("path inside a .git folder");
    }
    let parent = path
        .parent()
        .ok_or_else(|| WriteError::InvalidInput("no parent".into()))?;
    let parent = crate::paths::canonicalize(parent)?;
    let leaf = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| WriteError::InvalidInput("no name".into()))?;
    let path = parent.join(leaf);
    let profile =
        crate::paths::canonicalize(profile_root).unwrap_or_else(|_| profile_root.to_owned());
    if path.starts_with(&profile) || path.starts_with(main.common_dir()) {
        return reject("path inside the profile or the Git folder");
    }
    let config = std::fs::read_to_string(main.common_dir().join("config")).unwrap_or_default();
    if config.to_ascii_lowercase().contains("relativeworktrees") {
        return reject("extensions.relativeWorktrees is not supported");
    }

    // From here on every entry is reached from an open folder, never through a link: the parent
    // of the target and the common Git folder are opened and checked to be where their canonical
    // paths say, then `worktrees`, the administrative folder and the target are opened beneath
    // them refusing links (and reparse points on Windows).
    let parent_dir = sys::Dir::open_canonical(&parent)?;
    hook(Stage::TargetChecked);
    let Some(target) = parent_dir.child_dir(leaf, true)? else {
        return reject("path taken by a file or link");
    };
    if !target.is_empty()? {
        return reject("folder not empty");
    }
    let common = sys::Dir::open_canonical(main.common_dir())?;
    let Some(worktrees) = common.child_dir("worktrees", true)? else {
        return reject("the worktrees folder is a link or not a folder");
    };
    match worktrees.mkdir_new(id) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return reject("administrative folder already exists");
        }
        Err(e) => return Err(e.into()),
    }
    let admin_path = main.common_dir().join("worktrees").join(id);
    let mut dot_git_written = false;
    let result = (|| {
        let Some(admin) = worktrees.child_dir(id, false)? else {
            return reject("the administrative folder was replaced");
        };
        admin.write_new("HEAD", &head.to_bytes())?;
        admin.write_new("commondir", b"../..\n")?;
        let dot_git = path.join(".git");
        admin.write_new("gitdir", format!("{}\n", dot_git.display()).as_bytes())?;
        admin.sync();
        worktrees.sync();
        drop(admin);
        hook(Stage::BeforeCommit);
        if !target.is_at(&parent_dir, leaf)? {
            return reject("the target folder moved");
        }
        // The commit point: from now on Git sees the worktree.
        target.write_new(
            ".git",
            format!("gitdir: {}\n", admin_path.display()).as_bytes(),
        )?;
        dot_git_written = true;
        target.sync();
        let wt = WriteWorktree::open(&path)?;
        if !target.is_at(&parent_dir, leaf)? {
            return reject("the target folder moved");
        }
        Ok(wt)
    })();
    if result.is_err() {
        if dot_git_written {
            let _ = target.remove_file(".git");
        }
        clean_admin(&worktrees, id);
    }
    result
}

fn reject<T>(why: &str) -> Result<T> {
    Err(WriteError::InvalidInput(format!(
        "cannot recreate worktree: {why}"
    )))
}

/// Removes the administrative folder this recreation made, and only its own files: nothing is
/// followed, and a folder someone else filled is left in place.
fn clean_admin(worktrees: &sys::Dir, id: &str) {
    if let Ok(Some(admin)) = worktrees.child_dir(id, false) {
        for name in ["HEAD", "commondir", "gitdir"] {
            let _ = admin.remove_file(name);
        }
        drop(admin);
        let _ = worktrees.remove_dir(id);
    }
    worktrees.sync();
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

#[cfg(not(any(unix, windows)))]
mod sys {
    use std::path::Path;

    use super::super::{Result, WriteError};

    /// Not supported yet on this OS (Pendiente: etapa de validación multiplataforma).
    pub struct Dir;

    impl Dir {
        pub fn open_canonical(_path: &Path) -> Result<Self> {
            Err(WriteError::Unsupported("recreating a worktree"))
        }
        pub fn child_dir(&self, _name: &str, _create: bool) -> Result<Option<Self>> {
            Ok(None)
        }
        pub fn is_empty(&self) -> Result<bool> {
            Ok(false)
        }
        pub fn mkdir_new(&self, _name: &str) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        pub fn write_new(&self, _name: &str, _bytes: &[u8]) -> Result<()> {
            Err(WriteError::Unsupported("recreating a worktree"))
        }
        pub fn is_at(&self, _parent: &Self, _name: &str) -> Result<bool> {
            Ok(false)
        }
        pub fn remove_file(&self, _name: &str) -> std::io::Result<()> {
            Ok(())
        }
        pub fn remove_dir(&self, _name: &str) -> std::io::Result<()> {
            Ok(())
        }
        pub fn sync(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_ids_are_plain_names() {
        for ok in ["w", "feat-1", "a.b_c"] {
            assert!(is_worktree_id(ok), "{ok}");
        }
        for bad in ["", ".", "..", ".hidden", "a/b", "a\\b", "../x", "a b"] {
            assert!(!is_worktree_id(bad), "{bad:?}");
        }
    }
}
