//! The full `mcp.status` of the daemon: whether the caller's repo and worktree can be read, the
//! scope of a caller whose worktree was deleted, the pages of worktrees and paths and the table
//! of cursors of a connection (ADR-MCP-001 § 2, § 6).
//!
//! Signatures only: the behaviour is built against the tests of [`tests`].
// The daemon side of the status is not wired to anything yet.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use gitraptor_api::UntrustedName;
use gitraptor_api::messages::{ChangeCounts, FileChangeView, RepoView};
use gitraptor_api::methods::{McpPage, McpUnavailable};

/// What the OS says about a worktree, read once per call: the only I/O of the availability
/// check, so the rest is decided from facts a test can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorktreeFacts {
    /// The root of the worktree is there.
    pub exists: bool,
    /// The root and the common dir belong to the user running the daemon. `None` when the OS
    /// has no owner to compare (Windows): the engine's own verdict rules then.
    pub owned_by_me: Option<bool>,
    /// A linked worktree passes SEC-11 (its `.git` points back to the repo, its root is not
    /// `$HOME` or a drive root); a main one always does.
    pub trusted_link: bool,
}

/// Reads the [`WorktreeFacts`] of worktree `w` of `repo`; `home` is the folder SEC-11 refuses
/// as a root.
pub(crate) fn facts(repo: &RepoView, w: usize, home: Option<&Path>) -> WorktreeFacts {
    let _ = (repo, w, home);
    todo!("US-MCP-004: read the facts of a worktree")
}

/// Whether worktree `w` of `repo` can be read now: its root, its owner, SEC-11 and what the
/// engine published about it and about the repo.
pub(crate) fn availability(
    repo: &RepoView,
    w: usize,
    facts: &WorktreeFacts,
) -> Result<(), McpUnavailable> {
    let _ = (repo, w, facts);
    todo!("US-MCP-004: decide the availability of a worktree")
}

/// Why a read is refused before any data: the allowlist first, then availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotAllowlisted,
    Unavailable(McpUnavailable),
}

/// The checks of an `mcp.status` after the scope, in their order: a repo outside the allowlist
/// is refused as such whatever its state, so nothing of it is told before the developer enabled
/// it.
pub(crate) fn admit(
    allowed: bool,
    repo: &RepoView,
    w: usize,
    facts: &WorktreeFacts,
) -> Result<(), Refusal> {
    let _ = (allowed, repo, w, facts);
    todo!("US-MCP-004: the allowlist before availability")
}

/// Where the caller is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Located {
    /// In worktree `worktree` of repo `repo` (indexes into the repos and its worktrees).
    Worktree { repo: usize, worktree: usize },
    /// Nowhere any more, but the last scope this connection served was in repo `repo` and its
    /// root is gone: the worktree was deleted during the session.
    Missing { repo: usize },
    /// In no observed worktree, or not known to be where it was.
    Outside,
}

/// The scope of a call. `cwd` is `None` when the working folder cannot be read or made
/// canonical; `last` is the repo id and root of the last scope served on this connection.
pub(crate) fn locate_with_last(
    cwd: Option<&Path>,
    repos: &[RepoView],
    last: Option<&(String, PathBuf)>,
) -> Located {
    let _ = (cwd, repos, last);
    todo!("US-MCP-004: the scope, with the last one served")
}

/// One page of the paths of a worktree: the unique paths after `after`, at most
/// `MCP_PATHS_PAGE`, in order, with the placeholder cursor while there are more. `changes` is the
/// full list as `(path, area)` sorted; `counts` gives the total.
pub(crate) fn paths_page(
    of: UntrustedName,
    counts: ChangeCounts,
    changes: &[FileChangeView],
    after: Option<&str>,
) -> McpPage {
    let _ = (of, counts, changes, after);
    todo!("US-MCP-004: a page of paths")
}

/// What a cursor stands for: the list and where it went on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorEntry {
    Worktrees {
        repo_id: String,
        after: Option<PathBuf>,
    },
    Paths {
        repo_id: String,
        root: PathBuf,
        after: Option<String>,
    },
}

/// The cursors of one connection: opaque handles into the daemon's memory, at most
/// `MCP_MAX_CURSORS`, the oldest dropped first.
#[derive(Debug, Default)]
pub(crate) struct McpCursors {
    entries: VecDeque<(String, CursorEntry)>,
}

impl McpCursors {
    /// A new cursor for `entry`; `None` when the system cannot give random bytes.
    pub(crate) fn mint(&mut self, entry: CursorEntry) -> Option<String> {
        let _ = (&mut self.entries, entry);
        todo!("US-MCP-004: mint a cursor")
    }

    /// The entry of a cursor of this table; `None` for any other.
    pub(crate) fn get(&self, id: &str) -> Option<&CursorEntry> {
        let _ = (&self.entries, id);
        todo!("US-MCP-004: look a cursor up")
    }
}

#[cfg(test)]
#[path = "mcp_status_tests.rs"]
mod tests;
