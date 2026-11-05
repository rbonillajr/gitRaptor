//! `mcp.conflicts`: where the caller is and the page of the pairs the predictor published for
//! it. Nothing is computed per request: only the published state is read.
// Not wired into the dispatch yet: the connection's arm comes with the daemon's side.
#![allow(dead_code)]

use std::path::Path;

use gitraptor_api::UntrustedName;
use gitraptor_api::messages::RepoView;
use gitraptor_api::methods::{McpPair, McpPairCounts};
use gitraptor_api::prediction::PairDetail;

/// Where the caller of `mcp.conflicts` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictsLocated {
    /// In worktree `worktree` of repo `repo`.
    Worktree { repo: usize, worktree: usize },
    /// Under the common directory of repo `repo` but under no worktree root.
    RepoOnly { repo: usize },
    /// The worktree this connection last served was deleted.
    Missing { repo: usize },
    /// In no observed repo.
    Outside,
}

/// The scope of a call: `cwd` canonical or `None`, `last` the repo id and root last served.
pub(crate) fn locate_conflicts(
    _cwd: Option<&Path>,
    _repos: &[RepoView],
    _last: Option<&(String, std::path::PathBuf)>,
) -> ConflictsLocated {
    unimplemented!("locate_conflicts")
}

/// The counts of the whole scope, the page after `after` and where the next one starts.
pub(crate) fn build_page(
    _details: &[PairDetail],
    _caller: Option<&Path>,
    _base: Option<&UntrustedName>,
    _after: Option<&(u8, String)>,
    _now_ms: i64,
) -> (McpPairCounts, Vec<McpPair>, Option<(u8, String)>) {
    unimplemented!("build_page")
}

#[cfg(test)]
#[path = "mcp_conflicts_tests.rs"]
mod tests;
