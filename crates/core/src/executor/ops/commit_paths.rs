//! Which paths a `commit` takes: the named ones, checked against the worktree, or everything
//! staged. A path is accepted only if it is the same entry Git already knows or a file
//! gitoxide lists on the disk; nothing is followed as a link and case is never folded.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gitraptor_api::catalog::{CommitArgs, RejectReason};
use gitraptor_api::commit_paths::{is_dotgit_alias, is_git_special_name};
use gitraptor_git::{ChangeKind, EntryKind, RepoReader, UntrackedKind};
use serde::{Deserialize, Serialize};

/// What a commit takes from the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Selection {
    /// Literal paths for `commit -- <paths>` (`--only`), or empty for "all staged".
    pub paths: Vec<String>,
    pub all_staged: bool,
    /// Named untracked files, added before the commit and unstaged if it does not land.
    pub add_first: Vec<String>,
}

/// The marks of an index entry that make it unusable as a named path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EntryFlags {
    gitlink: bool,
    skip_worktree: bool,
    assume_unchanged: bool,
}

/// What the worktree knows of its paths, read once per plan with gitoxide.
#[derive(Debug)]
pub(super) struct WorktreeView {
    root: PathBuf,
    /// Index entries, and the `HEAD` entries a staged deletion removed from the index.
    tracked: BTreeMap<String, EntryFlags>,
    /// Untracked, not ignored files, one by one.
    untracked: BTreeSet<String>,
    /// Paths with a staged or an unstaged change.
    changed: BTreeSet<String>,
    /// Paths staged against `HEAD`, for "all staged".
    pub(super) staged: Vec<String>,
    /// `core.precomposeUnicode`: names compare in NFC, as Git compares them.
    precompose: bool,
    reader: RepoReader,
}

impl WorktreeView {
    /// Fails with `repo-identity-changed` when the worktree cannot be read.
    pub(super) fn read(root: &Path, reader: RepoReader) -> Result<Self, RejectReason> {
        let unreadable = |_| RejectReason::RepoIdentityChanged;
        let status = reader.status().map_err(unreadable)?;
        let assumed: BTreeSet<String> = reader
            .assume_unchanged_paths()
            .map_err(unreadable)?
            .into_iter()
            .collect();
        let mut tracked = BTreeMap::new();
        for e in reader.index_view().map_err(unreadable)?.entries {
            let path = e.path.to_string();
            let flags = EntryFlags {
                gitlink: e.kind == EntryKind::Gitlink,
                skip_worktree: e.skip_worktree,
                assume_unchanged: assumed.contains(&path),
            };
            tracked.insert(path, flags);
        }
        for c in &status.staged {
            if c.kind == ChangeKind::Deleted {
                tracked.entry(c.path.clone()).or_default();
            }
        }
        let untracked = reader
            .untracked()
            .map_err(unreadable)?
            .into_iter()
            .filter(|u| matches!(u.kind, UntrackedKind::File | UntrackedKind::Symlink))
            .map(|u| u.path.to_string())
            .collect();
        let changed = status
            .staged
            .iter()
            .chain(&status.unstaged)
            .map(|c| c.path.clone())
            .collect();
        Ok(Self {
            root: root.to_path_buf(),
            tracked,
            untracked,
            changed,
            staged: status.staged.iter().map(|c| c.path.clone()).collect(),
            precompose: reader.precompose_unicode(),
            reader,
        })
    }

    /// The name Git knows a path by.
    fn git_name(&self, path: &str) -> String {
        if self.precompose {
            gitraptor_git::paths::precomposed(path)
        } else {
            path.to_owned()
        }
    }

    fn is_untracked(&self, name: &str) -> bool {
        self.untracked.contains(name)
            || (self.precompose
                && self
                    .untracked
                    .iter()
                    .any(|u| gitraptor_git::paths::precomposed(u) == name))
    }
}

/// Every parent that exists must be a plain folder of this repo: no link, no `.git` of its own.
fn check_parents(root: &Path, components: &[&str]) -> Result<(), RejectReason> {
    let mut at = root.to_path_buf();
    for c in components {
        at.push(c);
        let Ok(meta) = std::fs::symlink_metadata(&at) else {
            return Ok(());
        };
        if meta.file_type().is_symlink() {
            return Err(RejectReason::PathOutsideWorktree);
        }
        if meta.is_dir() && std::fs::symlink_metadata(at.join(".git")).is_ok() {
            return Err(RejectReason::PathOutsideWorktree);
        }
    }
    Ok(())
}

/// A link is committed as a link only if its target, resolved without the disk, stays inside
/// the worktree and outside `.git`.
fn check_link(full: &Path, parents: &[&str]) -> Result<(), RejectReason> {
    let target = std::fs::read_link(full).map_err(|_| RejectReason::PathInvalid)?;
    if target.has_root() || target.is_absolute() {
        return Err(RejectReason::PathOutsideWorktree);
    }
    let mut at: Vec<String> = parents.iter().map(|c| (*c).to_owned()).collect();
    for c in target.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if at.pop().is_none() {
                    return Err(RejectReason::PathOutsideWorktree);
                }
            }
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                if is_dotgit_alias(&name) {
                    return Err(RejectReason::PathOutsideWorktree);
                }
                at.push(name.into_owned());
            }
            _ => return Err(RejectReason::PathOutsideWorktree),
        }
    }
    Ok(())
}

/// One named path: `Ok(true)` when it must be added first, `Ok(false)` when it is tracked.
fn check_one(view: &WorktreeView, path: &str, name: &str) -> Result<bool, RejectReason> {
    let components: Vec<&str> = path.split('/').collect();
    let (last, parents) = components.split_last().ok_or(RejectReason::PathInvalid)?;
    check_parents(&view.root, parents)?;
    let full = view.root.join(path);
    let on_disk = std::fs::symlink_metadata(&full).ok();
    if let Some(meta) = &on_disk {
        if meta.is_dir() {
            return Err(RejectReason::PathInvalid);
        }
        if meta.file_type().is_symlink() {
            if is_git_special_name(last) {
                return Err(RejectReason::PathInvalid);
            }
            check_link(&full, parents)?;
        }
    }
    if let Some(flags) = view.tracked.get(name) {
        if flags.gitlink || flags.skip_worktree || flags.assume_unchanged {
            return Err(RejectReason::PathInvalid);
        }
        return Ok(false);
    }
    if view.is_untracked(name) {
        return Ok(true);
    }
    if on_disk.is_some() && view.reader.is_ignored(name, false).unwrap_or(true) {
        return Err(RejectReason::PathIgnored);
    }
    Err(RejectReason::PathNotFound)
}

/// Explicit paths: each one the same entry Git knows (index or `HEAD`), or an untracked file;
/// otherwise the refusal and the path. None with a change → `nothing-to-commit`.
/// "All staged": no staged entry → `nothing-to-commit`; untracked files are never added.
pub(super) fn select(
    args: &CommitArgs,
    view: &WorktreeView,
) -> Result<Selection, (RejectReason, Option<String>)> {
    let Some(named) = &args.paths else {
        if view.staged.is_empty() {
            return Err((RejectReason::NothingToCommit, None));
        }
        return Ok(Selection {
            paths: Vec::new(),
            all_staged: true,
            add_first: Vec::new(),
        });
    };
    let mut paths = Vec::new();
    let mut add_first = Vec::new();
    let mut any_change = false;
    for path in named {
        let name = view.git_name(path);
        let add = check_one(view, path, &name).map_err(|r| (r, Some(path.clone())))?;
        if paths.contains(&name) {
            continue;
        }
        any_change |= add || view.changed.contains(&name);
        if add {
            add_first.push(name.clone());
        }
        paths.push(name);
    }
    if !any_change {
        return Err((RejectReason::NothingToCommit, None));
    }
    Ok(Selection {
        paths,
        all_staged: false,
        add_first,
    })
}
