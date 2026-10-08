//! The protected branches and forbidden paths in force for one operation (US-GRD-008,
//! DS-US-GRD-008 D2): the floor (and the confirmed floor, so a removal waits for its
//! confirmation like any relaxation), the worktree of the operation and the profile's
//! `settings.json`, from committed objects (TS-GRD-001). They only harden: the effective rules
//! are the union. What cannot be read adds nothing, and never removes anything.
//!
//! The local level (`settings.local.json`, US-GRP-013) has no reader yet.

use std::sync::LazyLock;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Level, Operation, RefValue};
use gitraptor_git::{Hide, PathLimits, RefName, RepoReader};
use gitraptor_policy::guard::policies::{Policies, Source, Touched, combine};
use gitraptor_policy::settings::model::Settings;
use gitraptor_policy::team::{Confirmed, TeamLoader};

/// The one team loader of this evaluation (bounded cache, shared by the connection threads).
static LOADER: LazyLock<TeamLoader> = LazyLock::new(TeamLoader::default);

/// New commits one evaluation may read in all, across the updates of a transaction or a push.
const MAX_COMMITS: usize = 4_096;
/// Updates one evaluation may read the commits of.
const MAX_READS: usize = 256;

/// The rules in force for an operation in the worktree `reader` was opened on.
pub fn policies(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&Settings>,
) -> Policies {
    let Ok(team) = LOADER.load(reader, confirmed) else {
        return Policies::default();
    };
    combine(&[
        Source {
            level: Level::Floor,
            settings: team.floor.parsed.applicable(),
        },
        Source {
            level: Level::Floor,
            settings: team
                .confirmed_floor
                .as_ref()
                .and_then(|f| f.parsed.applicable()),
        },
        Source {
            level: Level::Worktree,
            settings: team.worktree.parsed.applicable(),
        },
        Source {
            level: Level::Profile,
            settings: profile,
        },
    ])
}

/// [`policies`] with the profile of the daemon.
pub fn policies_for(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&crate::profile::ProfileDirs>,
) -> Policies {
    let parsed = profile.map(crate::profile::settings::profile_settings);
    policies(
        reader,
        confirmed,
        parsed.as_ref().and_then(|p| p.applicable()),
    )
}

/// What a client that cannot tell the actor evaluates in degraded mode (D11): the floor alone,
/// and only its rules for everyone.
pub fn degraded(reader: &RepoReader) -> Policies {
    let Ok(team) = LOADER.load(reader, None) else {
        return Policies::default();
    };
    combine(&[Source {
        level: Level::Floor,
        settings: team.floor.parsed.applicable(),
    }])
    .everyone_only()
}

/// What the commits of each update bring (D5), aligned with the updates of the operation; only
/// read when a forbidden-path rule governs `actor`. An update that moves no branch to a commit
/// has `None`. Whatever cannot be read or counted within the bounds is `unverifiable`.
pub fn touched(
    reader: &RepoReader,
    operation: &Operation,
    policies: &Policies,
    actor: Option<AgentKind>,
) -> Vec<Option<Touched>> {
    if !policies.needs_paths(actor) {
        return Vec::new();
    }
    let mut left_commits = MAX_COMMITS;
    let mut reads = 0;
    let mut read = |old: Option<&str>, new: &RefValue, updated: &[&str], hide: Hide| {
        let RefValue::Oid(new) = new else {
            return None;
        };
        reads += 1;
        let unverifiable = Touched {
            paths: Vec::new(),
            unverifiable: true,
        };
        if reads > MAX_READS || left_commits == 0 {
            return Some(unverifiable);
        }
        let limits = PathLimits {
            commits: left_commits.min(PathLimits::default().commits),
            ..PathLimits::default()
        };
        match reader.new_commit_paths(old, new, updated, hide, &limits) {
            Ok(found) if !found.unverifiable => {
                left_commits = left_commits.saturating_sub(found.commits);
                Some(Touched {
                    paths: found.paths,
                    unverifiable: false,
                })
            }
            _ => Some(unverifiable),
        }
    };
    match operation {
        Operation::RefTransaction { updates, .. } => {
            let refs: Vec<&str> = updates.iter().map(|u| u.refname.as_str()).collect();
            updates
                .iter()
                .map(|u| {
                    if !u.refname.starts_with("refs/heads/") {
                        return None;
                    }
                    // Zero is also what Git sends when the writer gave no expected old value
                    // (`update-ref <ref> <new>`): the value the ref still has in `prepared`.
                    let old = match &u.old {
                        RefValue::Oid(old) => Some(old.clone()),
                        RefValue::Zero => RefName::new(&u.refname)
                            .ok()
                            .and_then(|name| reader.resolve_ref(&name).ok().flatten()),
                        RefValue::Symbolic(_) => None,
                    };
                    read(old.as_deref(), &u.new, &refs, Hide::OtherBranches)
                })
                .collect()
        }
        Operation::Push { updates, .. } => updates
            .iter()
            .map(|u| read(u.remote.oid(), &u.local, &[], Hide::RemoteTracking))
            .collect(),
        Operation::Rebase { .. } | Operation::Commit { .. } => Vec::new(),
    }
}
