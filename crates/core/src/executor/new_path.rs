//! The new path of `create-worktree` (ADR-CKP-002 § 1 "Ruta nueva", H-02, BR-CKP-VAL-001).
//!
//! Checked when the plan is prepared and again under the repo's write lock, right before `git`
//! runs (§ 5): the parent exists, no component is a symbolic link (`lstat`), the parent belongs to
//! the user and is not writable by the group or others, the path stays outside every `.git`, the
//! profile and every observed worktree, and its last component does not exist. The path given to
//! Git is the canonical one. The template that builds the path is US-CKP-018's.

use std::path::{Component, Path, PathBuf};

/// Why a new worktree path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewPathError {
    NotAbsolute,
    /// `..`, `.`, or no final name.
    NotNormal,
    ParentMissing,
    /// A component of the path is a symbolic link.
    Symlink,
    /// The parent is another user's or writable by the group or others.
    ParentNotPrivate,
    /// Inside a `.git`, the profile or an observed worktree.
    InsideProtected,
    /// Something already exists there.
    Exists,
}

impl NewPathError {
    /// The contract's reason, for prepare and for the check under the lock.
    pub fn reason(self) -> gitraptor_api::catalog::RejectReason {
        gitraptor_api::catalog::RejectReason::NewPathRefused
    }
}

#[cfg(unix)]
fn parent_is_private(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    meta.uid() == rustix::process::geteuid().as_raw() && meta.permissions().mode() & 0o022 == 0
}

/// Pendiente: etapa de validación multiplataforma (owner and ACL of the parent on Windows).
#[cfg(not(unix))]
fn parent_is_private(_meta: &std::fs::Metadata) -> bool {
    true
}

/// Checks `path` for a new worktree. `protected` are the roots it must stay out of: the common
/// Git directory of every observed repo, the profile and every observed worktree. Returns the
/// canonical path to give to Git.
pub fn check_new_worktree_path(
    path: &Path,
    protected: &[PathBuf],
) -> Result<PathBuf, NewPathError> {
    if !path.is_absolute() {
        return Err(NewPathError::NotAbsolute);
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(NewPathError::NotNormal);
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(NewPathError::NotNormal);
    };
    if name.to_string_lossy().eq_ignore_ascii_case(".git") {
        return Err(NewPathError::InsideProtected);
    }
    // Every existing component, the parent included, with `lstat`: never followed.
    let mut current = PathBuf::new();
    for c in parent.components() {
        current.push(c);
        let meta = std::fs::symlink_metadata(&current).map_err(|_| NewPathError::ParentMissing)?;
        if meta.file_type().is_symlink() {
            return Err(NewPathError::Symlink);
        }
    }
    let parent_meta = std::fs::symlink_metadata(parent).map_err(|_| NewPathError::ParentMissing)?;
    if !parent_meta.is_dir() {
        return Err(NewPathError::ParentMissing);
    }
    if !parent_is_private(&parent_meta) {
        return Err(NewPathError::ParentNotPrivate);
    }
    // No symlink on the way, so the canonical parent is the parent itself (modulo case on
    // case-insensitive file systems). Containment compares one form on both sides, the one of
    // `std::fs::canonicalize` (verbatim on Windows); Git gets the drive form when there is one.
    let key = std::fs::canonicalize(parent)
        .map_err(|_| NewPathError::ParentMissing)?
        .join(name);
    let canonical = gitraptor_git::paths::canonicalize(parent)
        .map_err(|_| NewPathError::ParentMissing)?
        .join(name);
    let inside = |root: &PathBuf| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        key.starts_with(&root)
    };
    if protected.iter().any(inside)
        || key
            .components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
    {
        return Err(NewPathError::InsideProtected);
    }
    if std::fs::symlink_metadata(&canonical).is_ok() {
        return Err(NewPathError::Exists);
    }
    Ok(canonical)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// H-02 (Validación 19).
    #[test]
    fn a_new_worktree_path_is_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let parent = root.join("work");
        let repo = parent.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let protected = vec![repo.clone(), repo.join(".git")];

        let ok = check_new_worktree_path(&parent.join("repo-feat-x"), &protected).unwrap();
        assert_eq!(ok, parent.join("repo-feat-x"));

        assert_eq!(
            check_new_worktree_path(Path::new("rel/x"), &protected),
            Err(NewPathError::NotAbsolute)
        );
        assert_eq!(
            check_new_worktree_path(&parent.join("a/../b"), &protected),
            Err(NewPathError::NotNormal)
        );
        assert_eq!(
            check_new_worktree_path(&repo.join("inside"), &protected),
            Err(NewPathError::InsideProtected)
        );
        assert_eq!(
            check_new_worktree_path(&repo.join(".git").join("x"), &[]),
            Err(NewPathError::InsideProtected)
        );
        assert_eq!(
            check_new_worktree_path(&root.join("missing").join("x"), &protected),
            Err(NewPathError::ParentMissing)
        );
        // A symbolic link on the way.
        let link = root.join("link");
        std::os::unix::fs::symlink(&parent, &link).unwrap();
        assert_eq!(
            check_new_worktree_path(&link.join("repo-feat-y"), &protected),
            Err(NewPathError::Symlink)
        );
        // A parent writable by the group.
        let open = root.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert_eq!(
            check_new_worktree_path(&open.join("x"), &protected),
            Err(NewPathError::ParentNotPrivate)
        );
        // A path that appears between prepare and run.
        let late = parent.join("repo-late");
        assert!(check_new_worktree_path(&late, &protected).is_ok());
        std::fs::create_dir(&late).unwrap();
        assert_eq!(
            check_new_worktree_path(&late, &protected),
            Err(NewPathError::Exists)
        );
    }
}
