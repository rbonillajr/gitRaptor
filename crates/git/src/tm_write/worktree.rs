//! A worktree the applier may write to, and the preconditions of ADR-TMC-002 § 3, step 1.
//!
//! The root comes from the validated state of the daemon, never from a parameter of the request
//! or from `core.worktree`. The Git folders are found by reading `<root>/.git` (a folder, or a
//! `gitdir:` file for a linked worktree) without following links.

use std::path::{Path, PathBuf};

use super::{Result, WriteError};

/// Markers of a Git operation in progress in a worktree's Git folder (BR-TMC-EDGE-004).
pub const IN_PROGRESS_MARKERS: &[&str] = &[
    "rebase-merge",
    "rebase-apply",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "sequencer",
];

/// Locks of the common Git folder checked besides `refs/**/*.lock`.
const COMMON_LOCKS: &[&str] = &["packed-refs.lock", "config.lock", "shallow.lock"];
/// Locks of the worktree's own Git folder.
const WORKTREE_LOCKS: &[&str] = &["index.lock", "HEAD.lock"];

/// A worktree with its Git folders, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteWorktree {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

/// Why the applier may not start on a worktree. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Precondition {
    /// A Git operation is in progress.
    InProgress {
        worktree: PathBuf,
        marker: &'static str,
    },
    /// A Git lock is present ("Git busy"); it is left in place.
    GitBusy { lock: PathBuf },
}

impl WriteWorktree {
    /// Validates `root` (absolute, canonical, no links on the way) and finds its Git folders.
    pub fn open(root: &Path) -> Result<Self> {
        crate::paths::validate(root)?;
        let canonical = root.canonicalize()?;
        if canonical != root {
            return Err(WriteError::InvalidInput(
                "worktree root must be canonical".into(),
            ));
        }
        let dot_git = root.join(".git");
        let meta = dot_git.symlink_metadata()?;
        let git_dir = if meta.is_dir() {
            dot_git
        } else if meta.is_file() {
            let text = std::fs::read_to_string(&dot_git)?;
            let target = text
                .strip_prefix("gitdir: ")
                .map(str::trim_end)
                .ok_or_else(|| WriteError::Untrusted(".git file without gitdir".into()))?;
            let target = Path::new(target);
            let target = if target.is_absolute() {
                target.to_owned()
            } else {
                root.join(target)
            };
            target.canonicalize()?
        } else {
            return Err(WriteError::Untrusted(".git is a link".into()));
        };
        if git_dir.join("HEAD").symlink_metadata().is_err() {
            return Err(WriteError::Untrusted("git folder without HEAD".into()));
        }
        let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
            Ok(text) => {
                let rel = Path::new(text.trim_end());
                let dir = if rel.is_absolute() {
                    rel.to_owned()
                } else {
                    git_dir.join(rel)
                };
                dir.canonicalize()?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => git_dir.clone(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            root: root.to_owned(),
            git_dir,
            common_dir,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The worktree's own Git folder (`HEAD`, `index`).
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The shared Git folder (objects, refs).
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    pub fn is_linked(&self) -> bool {
        self.git_dir != self.common_dir
    }

    /// Path of the worktree's index.
    pub fn index_path(&self) -> PathBuf {
        self.git_dir.join("index")
    }

    /// Path of the worktree's `HEAD`.
    pub fn head_path(&self) -> PathBuf {
        self.git_dir.join("HEAD")
    }

    /// Every precondition that fails now. An empty list means the applier may start.
    pub fn preconditions(&self) -> Vec<Precondition> {
        let mut failed = Vec::new();
        for marker in IN_PROGRESS_MARKERS {
            if self.git_dir.join(marker).symlink_metadata().is_ok() {
                failed.push(Precondition::InProgress {
                    worktree: self.root.clone(),
                    marker,
                });
            }
        }
        for lock in WORKTREE_LOCKS {
            let path = self.git_dir.join(lock);
            if path.symlink_metadata().is_ok() {
                failed.push(Precondition::GitBusy { lock: path });
            }
        }
        for lock in COMMON_LOCKS {
            let path = self.common_dir.join(lock);
            if path.symlink_metadata().is_ok() {
                failed.push(Precondition::GitBusy { lock: path });
            }
        }
        let mut ref_locks = Vec::new();
        ref_lock_files(&self.common_dir.join("refs"), 0, &mut ref_locks);
        failed.extend(
            ref_locks
                .into_iter()
                .map(|lock| Precondition::GitBusy { lock }),
        );
        failed
    }

    /// The current content of `HEAD`.
    pub fn read_head(&self) -> Result<HeadValue> {
        HeadValue::parse(&std::fs::read(self.head_path())?)
    }
}

/// `*.lock` files under `refs/`, never following links and at a bounded depth.
fn ref_lock_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            ref_lock_files(&path, depth + 1, out);
        } else if path.extension().is_some_and(|e| e == "lock") {
            out.push(path);
        }
    }
}

/// The content of a `HEAD` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadValue {
    /// `ref: refs/heads/<name>`.
    Branch(crate::RefName),
    Detached(crate::Oid),
}

impl HeadValue {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| WriteError::Untrusted("HEAD is not UTF-8".into()))?
            .trim_end_matches('\n');
        if let Some(name) = text.strip_prefix("ref: ") {
            return Ok(Self::Branch(super::refs::branch_ref(name)?));
        }
        crate::Oid::from_hex(text)
            .map(Self::Detached)
            .ok_or_else(|| WriteError::Untrusted("HEAD is neither a ref nor an id".into()))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Branch(name) => format!("ref: {name}\n").into_bytes(),
            Self::Detached(id) => format!("{id}\n").into_bytes(),
        }
    }
}
