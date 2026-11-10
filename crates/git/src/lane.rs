//! The commits of a branch over the base, for the cockpit graph: newest first, bounded, and
//! counted with the same walk as the ahead count of the fleet (see `RepoReader::only_in_walk`).
//!
//! Read-only and without filters: only gitoxide, no process. Neither author nor email is read.

use crate::ReadError;
use crate::reader::{Count, RepoReader};

/// How many commits are always visited between two looks at the stop signal.
const STOP_EVERY: u64 = 64;

/// Bounds of one [`RepoReader::lane`] call.
#[derive(Debug, Clone, Copy)]
pub struct LaneLimits {
    /// Commits kept in the window.
    pub max_commits: usize,
    /// Commits visited to count the total; past it the total is "at least".
    pub max_walk: u64,
    /// Bytes of a summary, cut on a character boundary.
    pub max_summary_bytes: usize,
}

/// A branch over the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLane {
    /// The first merge-base of the base tip and the branch tip, as hex.
    pub merge_base: String,
    /// The newest commits, at most `max_commits`.
    pub commits: Vec<GitLaneCommit>,
    /// Every commit of the branch over the base, up to `max_walk`.
    pub total: Count,
}

/// One commit of a lane. Committer time only: no author, no email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLaneCommit {
    pub id: String,
    /// First line of the message, untrusted text.
    pub summary: String,
    pub committed_s: i64,
    /// Two or more parents.
    pub merge: bool,
}

/// Why a lane has no window. Not a read failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLaneGap {
    /// The branch and the base share no history.
    NoMergeBase,
    /// The stop signal fired before the walk ended.
    TooSlow,
}

impl RepoReader {
    /// The commits of `head` that `base_tip` does not reach, newest commit time first, and the
    /// merge-base they start from. Only the first `limits.max_commits` are kept; the rest are
    /// counted up to `limits.max_walk`. `stop` is looked at every 64 commits: when it says
    /// `true` the answer is [`GitLaneGap::TooSlow`], never a window cut short.
    ///
    /// # Errors
    /// [`ReadError::InvalidInput`] for an id that is not hex and [`ReadError::Unavailable`] when
    /// an object cannot be read.
    pub fn lane(
        &self,
        head: &str,
        base_tip: &str,
        limits: &LaneLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<Result<GitLane, GitLaneGap>, ReadError> {
        let id = |hex: &str| {
            gix::ObjectId::from_hex(hex.as_bytes())
                .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
        };
        let (tip, hidden) = (id(head)?, id(base_tip)?);
        let Some(merge_base) = self.merge_bases(base_tip, head)?.into_iter().next() else {
            return Ok(Err(GitLaneGap::NoMergeBase));
        };
        let walk = self
            .only_in_walk(tip, hidden)
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
        let mut commits = Vec::with_capacity(limits.max_commits.min(64));
        let mut visited: u64 = 0;
        for info in walk {
            let info = info.map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
            if visited.is_multiple_of(STOP_EVERY) && stop() {
                return Ok(Err(GitLaneGap::TooSlow));
            }
            if visited == limits.max_walk {
                return Ok(Ok(GitLane {
                    merge_base,
                    commits,
                    total: Count::AtLeast(limits.max_walk),
                }));
            }
            if commits.len() < limits.max_commits {
                commits.push(self.lane_commit(info.id, limits.max_summary_bytes)?);
            }
            visited += 1;
        }
        Ok(Ok(GitLane {
            merge_base,
            commits,
            total: Count::Exact(visited),
        }))
    }

    fn lane_commit(
        &self,
        id: gix::ObjectId,
        max_summary_bytes: usize,
    ) -> Result<GitLaneCommit, ReadError> {
        let bad = |e: &dyn std::fmt::Display| ReadError::Unavailable(format!("commit: {e}"));
        let commit = self.repo.find_commit(id).map_err(|e| bad(&e))?;
        let committed_s = commit.time().map_err(|e| bad(&e))?.seconds;
        let merge = commit.parent_ids().nth(1).is_some();
        let message = commit.message_raw().map_err(|e| bad(&e))?;
        let first = message.split(|b| *b == b'\n').next().unwrap_or_default();
        let first = String::from_utf8_lossy(first);
        Ok(GitLaneCommit {
            id: id.to_string(),
            summary: cut_on_boundary(first.trim_end_matches('\r'), max_summary_bytes).to_owned(),
            committed_s,
            merge,
        })
    }
}

/// `text` cut to at most `max` bytes without splitting a UTF-8 character.
fn cut_on_boundary(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
