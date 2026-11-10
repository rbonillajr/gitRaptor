//! The diff of a commit and between two commits, for `diff.commit` and `diff.between`.
//!
//! A commit is served only when a ref of the repository reaches it; everything else (nonexistent,
//! unreachable, not a commit, walk cut) is the same [`Lookup::Missing`], with no existence oracle
//! (DS-TS-CKP-006 D5). Oids are never parsed as revspecs, and there is no merge-base: a
//! `diff.between` is tree to tree (D6).

use std::time::Instant;

use super::{
    CutKind, DiffLimits, GitChange, GitSection, Side, build_section, change_of, components,
};
use crate::{MAX_ANCESTRY_WALK, ReadError, RepoReader};

/// Longest path `diff.between` accepts, in bytes (L-01).
const MAX_PATH_BYTES: usize = 4_096;
/// Commits walked between two looks at the deadline and the stop signal (D5).
const WALK_CHECK_EVERY: usize = 1_000;

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
/// than 4,096 bytes, or an empty, `.`, `..` or `\`-carrying component.
pub fn relative_components(path: &str) -> Result<Vec<&str>, ReadError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.chars().any(char::is_control)
    {
        return Err(ReadError::InvalidInput("invalid path".into()));
    }
    components(path)
}

/// Parses an oid of `diff.commit` or `diff.between`: `Ok(None)` when its hash kind is not the
/// repository's, which only reveals the hash kind of the repository (I-01).
fn parse_oid(hex: &str, kind: gix::hash::Kind) -> Result<Option<gix::ObjectId>, ReadError> {
    if !is_full_oid(hex) {
        return Err(ReadError::InvalidInput("invalid object id".into()));
    }
    // A well-formed oid of a hash kind this build does not know (SHA-256 without the feature)
    // cannot be the repository's either.
    let Ok(id) = gix::ObjectId::from_hex(hex.as_bytes()) else {
        return Ok(None);
    };
    Ok((id.kind() == kind).then_some(id))
}

fn unavailable(e: &dyn std::fmt::Display) -> ReadError {
    ReadError::Unavailable(format!("reachability: {e}"))
}

impl RepoReader {
    /// The diff of `commit` against its first parent (or the empty tree, for a root commit),
    /// without rename detection. A merge reports its parent count and is diffed against its
    /// first parent only.
    ///
    /// # Errors
    ///
    /// [`ReadError::InvalidInput`] for an oid that is not 40 or 64 lowercase hex; [`ReadError`]
    /// when the repository cannot be read. A commit that is not served is `Ok(Lookup::Missing)`.
    pub fn commit_diff(
        &self,
        commit: &str,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<Lookup<GitCommitDiff>, ReadError> {
        let Some(id) = parse_oid(commit, self.repo.object_hash())? else {
            return Ok(Lookup::Missing);
        };
        // D5: reachability is decided before anything of the requested object is read.
        if !self.reachable(&[id], limits, stop)? {
            return Ok(Lookup::Missing);
        }
        let parents: Vec<String> = self
            .repo
            .find_commit(id)
            .map_err(|e| ReadError::Unavailable(format!("commit: {e}")))?
            .parent_ids()
            .map(|p| p.to_string())
            .collect();
        let section =
            self.tree_section(parents.first().map(String::as_str), commit, limits, stop)?;
        Ok(Lookup::Found(GitCommitDiff {
            parents: parents.len(),
            section,
        }))
    }

    /// `old → new`, tree to tree, with no merge-base (D6); with `path`, only that literal file.
    /// A directory or a path absent on both sides gives an empty section.
    ///
    /// # Errors
    ///
    /// [`ReadError::InvalidInput`] for an invalid `path` or a malformed oid; [`ReadError`] when
    /// the repository cannot be read. A side that is not served is `Ok(Lookup::Missing)`.
    pub fn between_diff(
        &self,
        old: &str,
        new: &str,
        path: Option<&str>,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<Lookup<GitSection>, ReadError> {
        // L-01: the channel validated the path already; it is validated again here.
        if let Some(path) = path {
            relative_components(path)?;
        }
        let kind = self.repo.object_hash();
        let (Some(old_id), Some(new_id)) = (parse_oid(old, kind)?, parse_oid(new, kind)?) else {
            return Ok(Lookup::Missing);
        };
        // D5: one walk for both sides, before anything of either object is read.
        if !self.reachable(&[old_id, new_id], limits, stop)? {
            return Ok(Lookup::Missing);
        }
        let section = match path {
            None => self.tree_section(Some(old), new, limits, stop)?,
            Some(path) => {
                let max = limits.max_blob_bytes;
                let mut budget = limits.max_bytes;
                build_section(
                    &[path.to_owned()],
                    1,
                    None,
                    limits,
                    stop,
                    &mut budget,
                    |p| {
                        let comps = relative_components(p)?;
                        let old = self.committed_file(old, &comps, max)?;
                        let new = self.committed_file(new, &comps, max)?;
                        Ok((Side::from_committed(old), Side::from_committed(new)))
                    },
                    |_| GitChange::Modified,
                )?
            }
        };
        Ok(Lookup::Found(section))
    }

    /// The section of the tree diff `old → new` (`old = None`: the empty tree), with the whole
    /// byte budget. Listing past the deadline gives an empty section cut by time: the commits
    /// exist, so it is never `Missing`.
    fn tree_section(
        &self,
        old: Option<&str>,
        new: &str,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<GitSection, ReadError> {
        let listed = match self.changed_paths(old, new, limits.max_files, limits.deadline) {
            Ok(listed) => listed,
            // The only wait that ends in this variant is the deadline.
            Err(ReadError::TemporarilyUnavailable(_)) => {
                return Ok(GitSection {
                    files: Vec::new(),
                    files_total: 0,
                    cut: Some(CutKind::Time),
                });
            }
            Err(e) => return Err(e),
        };
        let total = listed.total as usize;
        let cut = (total > listed.paths.len()).then_some(CutKind::Files);
        let max = limits.max_blob_bytes;
        let mut budget = limits.max_bytes;
        let side = |commit: Option<&str>, comps: &[&str], max: u64| match commit {
            Some(commit) => self
                .committed_file(commit, comps, max)
                .map(Side::from_committed),
            None => Ok(Side::Absent),
        };
        build_section(
            &listed.paths,
            total,
            cut,
            limits,
            stop,
            &mut budget,
            |path| {
                let comps = components(path)?;
                Ok((side(old, &comps, max)?, side(Some(new), &comps, max)?))
            },
            |path| {
                // Presence only: a size limit of zero never loads a blob.
                let probe = |commit: Option<&str>| {
                    components(path)
                        .and_then(|c| side(commit, &c, 0))
                        .unwrap_or(Side::Absent)
                };
                change_of(&probe(old), &probe(Some(new))).unwrap_or(GitChange::Modified)
            },
        )
    }

    /// Whether every commit of `wanted` is reached walking parents from the tips of
    /// `refs/heads/`, `refs/remotes/`, `refs/tags/` (peeled to a commit) and `HEAD`.
    ///
    /// D5 (no existence oracle): nothing of `wanted` is read before the walk ends — not its
    /// header, not its date — and the walk has no date cutoff and ignores the commit-graph. A walk
    /// cut by the deadline, `stop` or [`MAX_ANCESTRY_WALK`] answers `false`, like an unknown oid.
    fn reachable(
        &self,
        wanted: &[gix::ObjectId],
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<bool, ReadError> {
        let tips = self.reachability_tips()?;
        if tips.is_empty() {
            return Ok(false);
        }
        let mut left: Vec<gix::ObjectId> = wanted.to_vec();
        left.dedup();
        let walk = self
            .repo
            .rev_walk(tips)
            .use_commit_graph(false)
            .all()
            .map_err(|e| unavailable(&e))?;
        for (n, info) in walk.enumerate() {
            if n >= MAX_ANCESTRY_WALK {
                return Ok(false);
            }
            if n % WALK_CHECK_EVERY == 0 && (stop() || Instant::now() >= limits.deadline) {
                return Ok(false);
            }
            let id = info.map_err(|e| unavailable(&e))?.id;
            left.retain(|w| *w != id);
            if left.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The commits the graph shows: tips of branches, remote-tracking branches, tags and `HEAD`.
    /// Never `refs/stash`, `refs/notes` or the Time Machine's refs (D5).
    fn reachability_tips(&self) -> Result<Vec<gix::ObjectId>, ReadError> {
        let platform = self.repo.references().map_err(|e| unavailable(&e))?;
        let mut candidates = Vec::new();
        for reference in platform.all().map_err(|e| unavailable(&e))? {
            let mut reference = reference.map_err(|e| unavailable(&e))?;
            let name = reference.name().as_bstr().to_string();
            let shown = ["refs/heads/", "refs/remotes/", "refs/tags/"]
                .iter()
                .any(|p| name.starts_with(p));
            if !shown {
                continue;
            }
            if let Ok(id) = reference.peel_to_id() {
                candidates.push(id.detach());
            }
        }
        if let Ok(id) = self.repo.head_id() {
            candidates.push(id.detach());
        }
        candidates.sort();
        candidates.dedup();
        // Only the tips' headers are read, never the requested objects'.
        let mut tips = Vec::with_capacity(candidates.len());
        for id in candidates {
            let header = self.repo.try_find_header(id).map_err(|e| unavailable(&e))?;
            if header.is_some_and(|h| h.kind() == gix::object::Kind::Commit) {
                tips.push(id);
            }
        }
        Ok(tips)
    }
}
