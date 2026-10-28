//! The diff size and commit format rules in force and the facts they read (US-GRD-009,
//! DS-US-GRD-009 D5, D6, D12 and D15): the floor alone for the client, and, in the daemon, the
//! new commits of each update of a `reference-transaction` in two passes. The message never
//! leaves this module: the first line is classified here and its bytes are dropped.

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Effect, GuardLogResult, NotPreventable, Operation, RefValue, Rule};
use gitraptor_git::{Hide, LineLimits, PathLimits, RefName, RepoReader};
use gitraptor_policy::guard::Evaluation;
use gitraptor_policy::guard::commit_format::parse_with;
use gitraptor_policy::guard::commit_policies::{
    CommitFacts, CommitPolicies, FileLines, NewCommits, apply, combine,
};
use gitraptor_policy::guard::glob::Budget;
use gitraptor_policy::guard::permissions::Chain;
use gitraptor_policy::guard::policies::Source;

use super::layers;

/// New commits one evaluation may read in all, across the updates of a transaction.
const MAX_COMMITS: usize = 4_096;
/// Updates one evaluation may read the commits of.
const MAX_READS: usize = 256;

/// The floor alone, every scope: what the client reads without a daemon. Nothing readable gives
/// no rules.
pub fn floor(reader: &RepoReader) -> CommitPolicies {
    let Ok(team) = layers::LOADER.load(reader, None) else {
        return CommitPolicies::default();
    };
    combine(&[Source {
        level: gitraptor_api::guard::Level::Floor,
        settings: team.floor.parsed.applicable(),
    }])
}

/// The rules of the floor that do not need an actor: what a client applies on its own (D14).
pub fn floor_everyone(reader: &RepoReader) -> CommitPolicies {
    floor(reader).everyone_only()
}

/// What the new commits of each update bring (D6), aligned with the updates of `operation`;
/// only read when a rule of the two keys governs `actor`, and the trees only when a size rule
/// does. An update that moves no branch to a commit, or that a `clone` certainly
/// makes, has `None`. Whatever cannot be read or counted within the bounds is `unverifiable`.
pub fn changes(
    reader: &RepoReader,
    operation: &Operation,
    rules: &CommitPolicies,
    actor: Option<AgentKind>,
    chain: &Chain,
) -> Vec<Option<NewCommits>> {
    let Operation::RefTransaction { updates, .. } = operation else {
        return Vec::new();
    };
    if !rules.needs_commit_facts(actor) {
        return Vec::new();
    }
    let lines = rules.needs_lines(actor).then(LineLimits::default);
    let refs: Vec<&str> = updates.iter().map(|u| u.refname.as_str()).collect();
    let takes_remote = chain.certainly_takes_remote_commits();
    let mut left_commits = MAX_COMMITS;
    let mut reads = 0;
    updates
        .iter()
        .map(|u| {
            // A branch, or a remote-tracking ref written by anything but a certain fetch, pull or
            // merge: an agent parks a commit under `refs/remotes/*` with `update-ref` and lands
            // it later, so parking is judged like a branch line.
            let remote_write = u.refname.starts_with("refs/remotes/") && !takes_remote;
            if !(u.refname.starts_with("refs/heads/") || remote_write) || chain.certainly_clones() {
                return None;
            }
            let unverifiable = NewCommits {
                commits: Vec::new(),
                unverifiable: true,
            };
            let new = match &u.new {
                RefValue::Oid(new) => new,
                // A symbolic ref follows another ref: what it will hold cannot be read here.
                RefValue::Symbolic(_) => return Some(unverifiable),
                // A deletion evaluates nothing.
                RefValue::Zero => return None,
            };
            reads += 1;
            if reads > MAX_READS || left_commits == 0 {
                return Some(unverifiable);
            }
            // Zero is also what Git sends when the writer gave no expected old value.
            let old = match &u.old {
                RefValue::Oid(old) => Some(old.clone()),
                RefValue::Zero => RefName::new(&u.refname)
                    .ok()
                    .and_then(|name| reader.resolve_ref(&name).ok().flatten()),
                RefValue::Symbolic(_) => None,
            };
            let limits = PathLimits {
                commits: left_commits.min(PathLimits::default().commits),
                ..PathLimits::default()
            };
            let read = |hide| {
                reader
                    .fresh_commit_changes(old.as_deref(), new, &refs, hide, &limits, lines.as_ref())
                    .map(into_facts)
            };
            // First a cheap look that hides only the old value: it brings a superset of the new
            // commits, so when none breaks a rule the answer is final. Only a hit (or a look
            // that does not fit the bounds) pays for hiding what the other branches hold: the
            // local ones always, the remote-tracking ones only when the processes above the hook
            // are certainly a fetch, pull or merge. An agent writes `refs/remotes/*` with
            // `update-ref` without any evaluation, so a commit parked there is still new.
            let confirm = if takes_remote && !remote_write {
                Hide::OtherBranches
            } else {
                Hide::LocalBranches
            };
            let found = match read(Hide::OldOnly) {
                Ok(found) if !found.unverifiable && !denies(rules, actor, &found) => Ok(found),
                _ => read(confirm),
            };
            match found {
                Ok(found) => {
                    left_commits = left_commits.saturating_sub(found.commits.len());
                    Some(found)
                }
                Err(_) => Some(unverifiable),
            }
        })
        .collect()
}

/// Whether the rules deny or ask for these commits.
fn denies(rules: &CommitPolicies, actor: Option<AgentKind>, found: &NewCommits) -> bool {
    let mut scratch = Evaluation::allow();
    apply(&mut scratch, found, actor, rules, &mut Budget::default());
    scratch.effect != Effect::Allow
}

/// The facts the rules read. The first line is classified here and its bytes are dropped.
fn into_facts(found: gitraptor_git::NewCommitChanges) -> NewCommits {
    NewCommits {
        unverifiable: found.unverifiable,
        commits: found
            .commits
            .into_iter()
            .map(|c| CommitFacts {
                header: parse_with(&c.head, c.body_separated, c.parents, c.reverts),
                oid: c.oid,
                parents: c.parents,
                partial: c.partial,
                files: c.files.map(|files| {
                    files
                        .into_iter()
                        .map(|(path, lines)| FileLines { path, lines })
                        .collect()
                }),
            })
            .collect(),
    }
}

/// A transaction that writes a remote-tracking ref: what a `fetch` does, and what an agent can
/// do with `update-ref` to park a commit, so the chain above the hook tells them apart.
pub fn writes_remote_tracking(op: &Operation) -> bool {
    matches!(op, Operation::RefTransaction { updates, .. }
        if updates.iter().any(|u| u.refname.starts_with("refs/remotes/") && !u.new.is_zero()))
}

/// Whether `rule` is one of the two commit policies.
pub fn is_commit_policy(rule: Rule) -> bool {
    matches!(rule, Rule::DiffSize | Rule::CommitFormat)
}

/// A `guard.log` page for a connection without `guard.commit-policies` (D10): the reasons of the
/// two policies are left out, and an entry whose reasons were all of them is not listed. The
/// summary still counts them.
pub fn legacy_log(log: &mut GuardLogResult) {
    log.entries.retain_mut(|entry| {
        let had_reasons = !entry.reasons.is_empty();
        entry.reasons.retain(|r| !is_commit_policy(r.rule));
        !had_reasons || !entry.reasons.is_empty()
    });
}

/// The limits a connection without `guard.commit-policies` can read (D10): the reach of the two
/// policies is left out.
pub fn legacy_not_preventable(limits: &mut Vec<NotPreventable>) {
    limits.retain(|n| *n != NotPreventable::CommitPolicyReach);
}
