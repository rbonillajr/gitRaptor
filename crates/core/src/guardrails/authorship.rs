//! `policies.commitAuthorship` in force for one commit (US-GRD-018, DS-US-GRD-018 § 4, D2): the
//! floor and the worktree from committed objects (TS-GRD-001), and the profile's `settings.json`.
//! Only a confirmed, fully readable floor relaxes to `flexible`; anything that cannot be read
//! leaves `agents-commit`, never less.
//!
//! The local level (`settings.local.json`) joins the profile's through `layers`: between the two
//! personal levels the local one wins.

use std::path::Path;

use gitraptor_git::RepoReader;
use gitraptor_policy::guard::authorship::Effective;
use gitraptor_policy::team::Confirmed;

use super::hook::{HookEnv, common_of, transaction_git_dir};
use super::layers;
use crate::guardrails::evaluate::open;

/// The worktree of the commit, from the hook client's working directory, when it belongs to the
/// repo of the dispatcher (`common`). `None` otherwise: the common reader is used.
pub fn worktree_reader(cwd: &Path, common: &Path) -> Option<RepoReader> {
    let env = HookEnv {
        git_dir: None,
        cwd: cwd.to_path_buf(),
    };
    let git_dir = transaction_git_dir(&env)?;
    let ours = common.canonicalize().ok()?;
    let theirs = common_of(&git_dir)?;
    if theirs != gitraptor_policy::guard::fastpath::simplified(ours) {
        return None;
    }
    open(&git_dir)
}

/// The policy in force for a commit in the worktree `reader` was opened on. `repo_id` is the
/// registry key (never the client's text). What cannot be read leaves `agents-commit`.
pub fn policy_for(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&crate::profile::ProfileDirs>,
    repo_id: Option<&str>,
) -> Effective {
    layers::load(reader, confirmed, profile, repo_id)
        .map(|layers| layers.authorship())
        .unwrap_or_default()
}
