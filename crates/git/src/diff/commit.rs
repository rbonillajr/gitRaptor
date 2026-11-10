//! The diff of a commit and between two commits, for `diff.commit` and `diff.between`.
//!
//! A commit is served only when a ref of the repository reaches it; everything else (nonexistent,
//! unreachable, not a commit, walk cut) is the same [`Lookup::Missing`], with no existence oracle
//! (DS-TS-CKP-006 D5).

use super::{DiffLimits, GitSection};
use crate::{ReadError, RepoReader};

/// The outcome of looking up commits by oid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    Found(T),
    /// Nonexistent, unreachable, not a commit, or the walk was cut: indistinguishable on purpose.
    Missing,
}

/// The diff of one commit against its first parent, or the empty tree for a root commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitDiff {
    pub parents: usize,
    pub section: GitSection,
}

/// `true` for 40 or 64 lowercase hex characters, nothing else (D4).
#[must_use]
pub fn is_full_oid(hex: &str) -> bool {
    matches!(hex.len(), 40 | 64) && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Splits the literal path of `diff.between` into its components: the only validation of that
/// path (L-01).
///
/// # Errors
///
/// [`ReadError::InvalidInput`] for an empty path, NUL or control characters, a leading `/`, more
/// than 4,096 bytes, or a component the worktree diff rejects.
pub fn relative_components(_path: &str) -> Result<Vec<&str>, ReadError> {
    Err(ReadError::Unavailable("not implemented".into()))
}

impl RepoReader {
    /// The diff of `commit` against its first parent (or the empty tree).
    ///
    /// # Errors
    ///
    /// [`ReadError`] when the repository cannot be read; a commit that is not served is
    /// `Ok(Lookup::Missing)`.
    pub fn commit_diff(
        &self,
        _commit: &str,
        _limits: &DiffLimits,
        _stop: &dyn Fn() -> bool,
    ) -> Result<Lookup<GitCommitDiff>, ReadError> {
        Err(ReadError::Unavailable("not implemented".into()))
    }

    /// `old → new`, tree to tree, with no merge-base (D6); with `path`, only that file.
    ///
    /// # Errors
    ///
    /// [`ReadError::InvalidInput`] for an invalid `path`; [`ReadError`] when the repository cannot
    /// be read. A side that is not served is `Ok(Lookup::Missing)`.
    pub fn between_diff(
        &self,
        _old: &str,
        _new: &str,
        _path: Option<&str>,
        _limits: &DiffLimits,
        _stop: &dyn Fn() -> bool,
    ) -> Result<Lookup<GitSection>, ReadError> {
        Err(ReadError::Unavailable("not implemented".into()))
    }
}
