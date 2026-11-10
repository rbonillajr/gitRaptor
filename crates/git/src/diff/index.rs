//! The diff of the index of a worktree, for `diff.index`: staged (`HEAD` against the index),
//! unstaged (the index against the working tree, untracked included), the content id of each
//! side and the conflicted paths (DS-TS-CKP-006 D7, D8).

use std::collections::BTreeMap;

use super::{DiffLimits, GitSection};
use crate::{ReadError, RepoReader};

/// The content id of each side of a file: a lowercase hex blob oid, or `None` when the side is
/// absent, too large, a link, a submodule or unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitContentIds {
    pub old: Option<String>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexSection {
    pub section: GitSection,
    /// By path, from the same read of the index as the section (L-02).
    pub ids: BTreeMap<String, GitContentIds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexDiff {
    pub staged: GitIndexSection,
    pub unstaged: GitIndexSection,
    /// Paths with entries of stages 1 to 3; out of both sections.
    pub conflicts: Vec<String>,
    pub conflicts_total: usize,
}

impl RepoReader {
    /// The index of this worktree against `HEAD` and against the working tree.
    ///
    /// # Errors
    ///
    /// [`ReadError::TemporarilyUnavailable`] when the index keeps changing while it is read;
    /// [`ReadError`] when the repository cannot be read.
    pub fn index_diff(
        &self,
        _limits: &DiffLimits,
        _stop: &dyn Fn() -> bool,
    ) -> Result<GitIndexDiff, ReadError> {
        Err(ReadError::Unavailable("not implemented".into()))
    }
}
