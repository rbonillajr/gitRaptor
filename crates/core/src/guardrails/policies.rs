//! The protected branches and forbidden paths in force for one operation (US-GRD-008,
//! DS-US-GRD-008 D2): the floor (and the confirmed floor, so a removal waits for its
//! confirmation like any relaxation), the worktree of the operation and the profile's
//! `settings.json`, from committed objects (TS-GRD-001). They only harden: the effective rules
//! are the union. What cannot be read adds nothing, and never removes anything.
//!
//! The local level (`settings.local.json`) joins them through `layers`.

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Level, Operation, RefValue};
use gitraptor_git::{Hide, PathLimits, RefName, RepoReader};
use gitraptor_policy::guard::config::protect_config;
use gitraptor_policy::guard::glob::Budget;
use gitraptor_policy::guard::policies::{Policies, Source, Touched, combine, forbidden_paths};
use gitraptor_policy::guard::{Evaluation, refs};

use super::layers;

/// New commits one evaluation may read in all, across the updates of a transaction or a push.
const MAX_COMMITS: usize = 4_096;
/// Updates one evaluation may read the commits of.
const MAX_READS: usize = 256;

/// The floor alone, every scope: what the client reads without a daemon. Nothing readable gives
/// no rules.
pub fn floor(reader: &RepoReader) -> Policies {
    let Ok(team) = layers::LOADER.load(reader, None) else {
        return Policies::default();
    };
    combine(&[Source {
        level: Level::Floor,
        settings: team.floor.parsed.applicable(),
    }])
}

/// What a client that cannot tell the actor evaluates in degraded mode (D11): the floor alone,
/// and only its rules for everyone.
pub fn degraded(reader: &RepoReader) -> Policies {
    floor(reader).everyone_only()
}

/// What each update of an agent's movement does to the protected configuration (BR-AUTH-004),
/// aligned with the updates of the operation. Read once per governed update, whatever the rules
/// say; empty for the person.
///
/// The commits the repo does not already hold are read, hiding only local branches: a commit
/// parked under `refs/remotes/*` is still new. When the root entries of the configuration (name,
/// mode, id) of the new and the old value are equal, nothing changed whatever those commits did
/// in between, and none of their paths counts. Whatever cannot be read or counted within the
/// bounds is `unverifiable`.
pub fn config_touched(
    reader: &RepoReader,
    operation: &Operation,
    actor: Option<AgentKind>,
) -> Vec<Option<Touched>> {
    let Some(actor) = actor else {
        return Vec::new();
    };
    let mut check = ConfigCheck {
        reader,
        actor,
        left_commits: MAX_COMMITS,
        reads: 0,
        budget: Budget::default(),
    };
    match operation {
        Operation::RefTransaction { updates, .. } => {
            let names: Vec<&str> = updates.iter().map(|u| u.refname.as_str()).collect();
            updates
                .iter()
                .map(|u| {
                    if !refs::is_governed(&u.refname) {
                        return None;
                    }
                    // Zero is also what Git sends when the writer gave no expected old value.
                    let old = match &u.old {
                        RefValue::Oid(old) => Some(old.clone()),
                        RefValue::Zero => RefName::new(&u.refname)
                            .ok()
                            .and_then(|name| reader.resolve_ref(&name).ok().flatten()),
                        RefValue::Symbolic(_) => None,
                    };
                    check.run(old.as_deref(), &u.new, &names)
                })
                .collect()
        }
        Operation::Push { updates, .. } => updates
            .iter()
            .map(|u| {
                // A ref that is not governed (a tag) is never evaluated: nothing to read.
                if !refs::is_governed(&u.remote_ref) {
                    return None;
                }
                check.run(u.remote.oid(), &u.local, &[])
            })
            .collect(),
        Operation::Rebase { .. } | Operation::Commit { .. } => Vec::new(),
    }
}

struct ConfigCheck<'a> {
    reader: &'a RepoReader,
    actor: AgentKind,
    left_commits: usize,
    reads: usize,
    budget: Budget,
}

impl ConfigCheck<'_> {
    /// Whether any of `paths` is the configuration. Running out of work counts as a hit, so it
    /// is looked at again with the exact set.
    fn hits(&mut self, paths: &[String]) -> bool {
        let mut scratch = Evaluation::allow();
        let touched = Touched {
            paths: paths.to_vec(),
            unverifiable: false,
        };
        protect_config(&mut scratch, &touched, Some(self.actor), &mut self.budget);
        scratch.effect != gitraptor_api::guard::Effect::Allow
    }

    fn run(&mut self, old: Option<&str>, new: &RefValue, updated: &[&str]) -> Option<Touched> {
        let unverifiable = Touched {
            paths: Vec::new(),
            unverifiable: true,
        };
        let new = match new {
            RefValue::Oid(new) => new,
            // A symbolic ref follows another ref: what it will hold cannot be read here.
            RefValue::Symbolic(_) => return Some(unverifiable),
            RefValue::Zero => return None,
        };
        self.reads += 1;
        if self.reads > MAX_READS || self.left_commits == 0 {
            return Some(unverifiable);
        }
        let limits = PathLimits {
            commits: self.left_commits.min(PathLimits::default().commits),
            ..PathLimits::default()
        };
        // First a cheap look that hides only the old value: it brings a superset of the new
        // commits, so when nothing in it touches the configuration the answer is final. Only a hit (or a look that does not fit the bounds)
        // pays for hiding what the local branches hold.
        let first = self
            .reader
            .fresh_commit_paths(old, new, updated, Hide::OldOnly, &limits);
        let found = match first {
            Ok(found) if !found.unverifiable && !self.hits(&found.paths) => Ok(found),
            _ => self
                .reader
                .fresh_commit_paths(old, new, updated, Hide::LocalBranches, &limits),
        };
        match found {
            Ok(found) if !found.unverifiable => {
                self.left_commits = self.left_commits.saturating_sub(found.commits);
                Some(Touched {
                    paths: found.paths,
                    unverifiable: false,
                })
            }
            _ => Some(unverifiable),
        }
    }
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
    let mut budget = Budget::default();
    let mut read = |old: Option<&str>, new: &RefValue, updated: &[&str], hide: Hide| {
        let unverifiable = Touched {
            paths: Vec::new(),
            unverifiable: true,
        };
        let new = match new {
            RefValue::Oid(new) => new,
            // A symbolic ref follows another ref: what it will hold cannot be read here.
            RefValue::Symbolic(_) => return Some(unverifiable),
            RefValue::Zero => return None,
        };
        reads += 1;
        if reads > MAX_READS || left_commits == 0 {
            return Some(unverifiable);
        }
        let limits = PathLimits {
            commits: left_commits.min(PathLimits::default().commits),
            ..PathLimits::default()
        };
        // First a cheap look that hides only the old value: it brings a superset of the new
        // commits, so when nothing in it is forbidden the answer is final. Only a hit (or a
        // look that does not fit the bounds) pays for hiding what the other branches hold.
        let first = reader.fresh_commit_paths(old, new, updated, Hide::OldOnly, &limits);
        let found = match first {
            Ok(found)
                if !found.unverifiable && !hits(policies, actor, &found.paths, &mut budget) =>
            {
                Ok(found)
            }
            _ => reader.fresh_commit_paths(old, new, updated, hide, &limits),
        };
        match found {
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
            // Every pushed ref: a tag or a note uploads commits too, and the forbidden paths reach
            // them. A deletion (`Zero`) reads nothing.
            .map(|u| read(u.remote.oid(), &u.local, &[], Hide::RemoteTracking))
            .collect(),
        Operation::Rebase { .. } | Operation::Commit { .. } => Vec::new(),
    }
}

/// Whether any of `paths` is forbidden for `actor`. Running out of work counts as a hit, so it
/// is looked at again with the exact set.
fn hits(
    policies: &Policies,
    actor: Option<AgentKind>,
    paths: &[String],
    budget: &mut Budget,
) -> bool {
    let mut scratch = Evaluation::allow();
    let touched = Touched {
        paths: paths.to_vec(),
        unverifiable: false,
    };
    forbidden_paths(&mut scratch, &touched, actor, policies, budget);
    scratch.effect != gitraptor_api::guard::Effect::Allow
}
