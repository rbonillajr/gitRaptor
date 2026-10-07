//! `policies.commitAuthorship` in force for one commit (US-GRD-018, DS-US-GRD-018 § 4, D2): the
//! floor and the worktree from committed objects (TS-GRD-001), and the profile's `settings.json`.
//! Only a confirmed, fully readable floor relaxes to `flexible`; anything that cannot be read
//! leaves `agents-commit`, never less.
//!
//! The local level (`settings.local.json`, US-GRP-013) has no reader yet: it joins here when it
//! does.

use std::path::Path;
use std::sync::LazyLock;

use gitraptor_api::guard::Level;
use gitraptor_git::RepoReader;
use gitraptor_policy::guard::authorship::{Effective, Source, combine};
use gitraptor_policy::settings::document::SourceStatus;
use gitraptor_policy::settings::model::Settings;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, TeamLoader, TeamSource};

use super::hook::{HookEnv, common_of, transaction_git_dir};
use crate::guardrails::evaluate::open;

/// The one team loader of the evaluation (bounded cache, shared by the connection threads).
static LOADER: LazyLock<TeamLoader> = LazyLock::new(TeamLoader::default);

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

fn commit_authorship(
    s: Option<&Settings>,
) -> Option<&gitraptor_policy::settings::model::CommitAuthorship> {
    s?.policies.as_ref()?.commit_authorship.as_ref()
}

/// The floor relaxes only when it is the confirmed one and fully readable (D2, Q-GRD-20).
fn floor_may_relax(floor: &TeamSource, confirmed: Option<&Confirmed>) -> bool {
    let Some(confirmed) = confirmed else {
        return false;
    };
    let same = match &confirmed.floor {
        ConfirmedFloor::Absent => floor.blob.is_none(),
        ConfirmedFloor::Blob(id) => floor.blob.as_deref() == Some(id.as_str()),
    };
    same && floor.status() == SourceStatus::Readable
}

/// The policy in force for a commit in the worktree `reader` was opened on.
pub fn policy(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&Settings>,
) -> Effective {
    let Ok(team) = LOADER.load(reader, confirmed) else {
        return Effective::default();
    };
    let sources = [
        Source {
            level: Level::Floor,
            setting: commit_authorship(team.floor.parsed.applicable()),
            may_relax: floor_may_relax(&team.floor, confirmed),
        },
        Source {
            level: Level::Worktree,
            setting: commit_authorship(team.worktree.parsed.applicable()),
            may_relax: false,
        },
        Source {
            level: Level::Profile,
            setting: commit_authorship(profile),
            may_relax: false,
        },
    ];
    combine(&sources).effective
}

/// [`policy`] with the profile of the daemon.
pub fn policy_for(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&crate::profile::ProfileDirs>,
) -> Effective {
    let parsed = profile.map(crate::profile::settings::profile_settings);
    policy(
        reader,
        confirmed,
        parsed.as_ref().and_then(|p| p.applicable()),
    )
}
