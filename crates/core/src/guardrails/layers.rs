//! The five sources of one evaluation (team floor, confirmed floor and worktree, profile and
//! local) read once and combined.
//!
//! stub: replaced by the implementation slice. The signatures are the contract; the bodies
//! read every level but combine nothing yet, which is today's behavior.

use std::sync::LazyLock;

use gitraptor_git::RepoReader;
use gitraptor_policy::guard::authorship::Effective;
use gitraptor_policy::guard::policies::Policies;
use gitraptor_policy::layers::IgnoredRelaxation;
use gitraptor_policy::settings::Parsed;
use gitraptor_policy::team::{Confirmed, EffectivePermissions, TeamConfig, TeamLoader};

use crate::profile::ProfileDirs;
use crate::profile::settings::{local_settings, profile_settings};

/// The one team loader of the evaluation (bounded cache, shared by the connection threads).
pub(crate) static LOADER: LazyLock<TeamLoader> = LazyLock::new(TeamLoader::default);

/// The five sources of one evaluation: team (floor, confirmed floor, worktree), profile and
/// local.
#[derive(Debug, Clone)]
pub struct Layers {
    pub team: TeamConfig,
    pub profile: Parsed,
    pub local: Parsed,
    #[allow(dead_code)] // stub: read by the implementation slice
    confirmed: Option<Confirmed>,
}

/// Reads every level for the worktree `reader` was opened on. `repo_id` is the registry key
/// (never the client's text); `None` means no local level. Errors only when the team level
/// cannot be read at all.
pub fn load(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&ProfileDirs>,
    repo_id: Option<&str>,
) -> Result<Layers, gitraptor_git::ReadError> {
    let team = LOADER.load(reader, confirmed)?;
    Ok(Layers {
        team,
        profile: profile.map_or_else(Parsed::absent, profile_settings),
        local: match (profile, repo_id) {
            (Some(dirs), Some(id)) => local_settings(dirs, id),
            _ => Parsed::absent(),
        },
        confirmed: confirmed.cloned(),
    })
}

impl Layers {
    /// Team permissions hardened by the personal levels.
    pub fn permissions(&self) -> EffectivePermissions {
        // stub: replaced by the implementation slice
        self.team.permissions.clone()
    }

    /// Protected branches and forbidden paths: floor, confirmed floor, worktree, profile, local.
    pub fn policies(&self) -> Policies {
        // stub: replaced by the implementation slice
        Policies::default()
    }

    /// `commitAuthorship` in force: floor (may relax only if confirmed and readable), worktree,
    /// personal = local ?? profile.
    pub fn authorship(&self) -> Effective {
        // stub: replaced by the implementation slice
        Effective::default()
    }

    /// The relaxations the levels that only harden declared and that were ignored.
    pub fn ignored(&self) -> Vec<IgnoredRelaxation> {
        // stub: replaced by the implementation slice
        Vec::new()
    }
}
