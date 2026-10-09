//! The five sources of one evaluation (team floor, confirmed floor and worktree, profile and
//! local) read once and combined.
//!
//! One loader for the whole evaluation: the protected branches, the forbidden paths, the
//! authorship policy, the permissions and the ignored relaxations all come from the same read,
//! so they can never disagree about which sources were in force. The team levels come from
//! committed objects (never the working tree); the personal ones only harden (BR-CONS-001).

use std::sync::LazyLock;

use gitraptor_api::guard::Level;
use gitraptor_git::RepoReader;
use gitraptor_policy::guard::authorship::{self, Effective};
use gitraptor_policy::guard::policies::{self, Policies};
use gitraptor_policy::layers::{self, IgnoredRelaxation, Personal};
use gitraptor_policy::settings::Parsed;
use gitraptor_policy::settings::document::SourceStatus;
use gitraptor_policy::settings::model::{CommitAuthorship, Settings};
use gitraptor_policy::team::{
    Confirmed, ConfirmedFloor, EffectivePermissions, TeamConfig, TeamLoader, TeamSource,
};

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
    confirmed: Option<Confirmed>,
}

/// Reads every level for the worktree `reader` was opened on. `repo_id` is the registry key
/// (never the client's text); `None` means no local level. Errors only when the team level
/// cannot be read at all: the callers keep their fail-closed answers.
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

/// The floor relaxes only when it is the confirmed one and fully readable (Q-GRD-20).
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

fn commit_authorship(s: Option<&Settings>) -> Option<&CommitAuthorship> {
    s?.policies.as_ref()?.commit_authorship.as_ref()
}

impl Layers {
    fn may_relax(&self) -> bool {
        floor_may_relax(&self.team.floor, self.confirmed.as_ref())
    }

    /// Team permissions hardened by the personal levels: never below the team's.
    pub fn permissions(&self) -> EffectivePermissions {
        layers::harden(
            &self.team.permissions,
            Personal {
                profile: self.profile.applicable(),
                local: self.local.applicable(),
            },
        )
    }

    /// Protected branches and forbidden paths: floor, confirmed floor, worktree, profile, local.
    /// The union: no level removes a pattern of another. What cannot be read adds nothing.
    pub fn policies(&self) -> Policies {
        let source = |level, settings| policies::Source { level, settings };
        policies::combine(&[
            source(Level::Floor, self.team.floor.parsed.applicable()),
            // The confirmed floor too, so a removal waits for its confirmation like any
            // relaxation.
            source(
                Level::Floor,
                self.team
                    .confirmed_floor
                    .as_ref()
                    .and_then(|f| f.parsed.applicable()),
            ),
            source(Level::Worktree, self.team.worktree.parsed.applicable()),
            source(Level::Profile, self.profile.applicable()),
            source(Level::Local, self.local.applicable()),
        ])
    }

    /// `commitAuthorship` in force: floor (may relax only if confirmed and readable), worktree,
    /// personal = local ?? profile.
    pub fn authorship(&self) -> Effective {
        let (personal_level, personal) = match commit_authorship(self.local.applicable()) {
            Some(s) => (Level::Local, Some(s)),
            None => (Level::Profile, commit_authorship(self.profile.applicable())),
        };
        authorship::combine(&[
            authorship::Source {
                level: Level::Floor,
                setting: commit_authorship(self.team.floor.parsed.applicable()),
                may_relax: self.may_relax(),
            },
            authorship::Source {
                level: Level::Worktree,
                setting: commit_authorship(self.team.worktree.parsed.applicable()),
                may_relax: false,
            },
            authorship::Source {
                level: personal_level,
                setting: personal,
                may_relax: false,
            },
        ])
        .effective
    }

    /// The relaxations the levels that only harden declared and that were ignored. Only
    /// observation: nothing here changes a decision.
    pub fn ignored(&self) -> Vec<IgnoredRelaxation> {
        layers::ignored_relaxations(&self.team, &self.profile, &self.local, self.may_relax())
    }
}
