//! Recreating a deleted linked worktree without checkout and without `git worktree add`, which is
//! porcelain (SEC-TMC-02). The administrative folder is written by hand, as Git lays it out, and
//! the worktree's `.git` file is written last: it is the commit point. If the process dies before
//! it, a stray administrative folder is left that `git worktree prune` cleans.
//!
//! A worktree is only recreated at its original path if it is absolute, does not exist or is an
//! empty folder, its parent exists, and it lies outside the profile and outside every `.git`
//! (SEC-TMC-04). Files and index come afterwards through [`super::files`] and [`super::index`].

use std::io::Write;
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

/// Recreates worktree `id` of `main` (the main worktree) at `path`, with `HEAD` set to `head`.
pub fn recreate(
    main: &WriteWorktree,
    id: &str,
    path: &Path,
    head: &HeadValue,
    profile_root: &Path,
) -> Result<WriteWorktree> {
    let reject = |why: &str| {
        Err(WriteError::InvalidInput(format!(
            "cannot recreate worktree: {why}"
        )))
    };
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
    let parent = parent.canonicalize()?;
    let leaf = path
        .file_name()
        .ok_or_else(|| WriteError::InvalidInput("no name".into()))?;
    let path = parent.join(leaf);
    let profile = profile_root
        .canonicalize()
        .unwrap_or_else(|_| profile_root.to_owned());
    if path.starts_with(&profile) || path.starts_with(main.common_dir()) {
        return reject("path inside the profile or the Git folder");
    }
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => {
            if std::fs::read_dir(&path)?.next().is_some() {
                return reject("folder not empty");
            }
        }
        Ok(_) => return reject("path taken by a file or link"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let config = std::fs::read_to_string(main.common_dir().join("config")).unwrap_or_default();
    if config.to_ascii_lowercase().contains("relativeworktrees") {
        return reject("extensions.relativeWorktrees is not supported");
    }

    let worktrees = main.common_dir().join("worktrees");
    if worktrees.symlink_metadata().is_err() {
        std::fs::create_dir(&worktrees)?;
    }
    let admin = worktrees.join(id);
    match std::fs::create_dir(&admin) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return reject("administrative folder already exists");
        }
        Err(e) => return Err(e.into()),
    }
    write_new(&admin.join("HEAD"), &head.to_bytes())?;
    write_new(&admin.join("commondir"), b"../..\n")?;
    let dot_git = path.join(".git");
    write_new(
        &admin.join("gitdir"),
        format!("{}\n", dot_git.display()).as_bytes(),
    )?;
    if path.symlink_metadata().is_err() {
        std::fs::create_dir(&path)?;
    }
    write_new(
        &dot_git,
        format!("gitdir: {}\n", admin.display()).as_bytes(),
    )?;
    super::lock::sync_parent(&dot_git);
    WriteWorktree::open(&path)
}

/// Writes a new file, exclusively, and syncs it.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
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
