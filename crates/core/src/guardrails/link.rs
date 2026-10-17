//! Who the `git` of a hook belongs to when the daemon runs it for the executor: the direct child
//! of a governed plan inherits its decision while it makes the registered transitions; anything
//! below it is a grandchild, decided again for the plan's requester (H-01).
//!
//! Inheriting is strict: a transition matches only by equality or by a proof read from the repo
//! (parents, ancestry, paths a subset of the prepared ones); in doubt it is decided again. The
//! child's parent must be the daemon, by pid and start time.

use std::path::Path;

use gitraptor_api::guard::{AuthorshipFacts, Decision, Effect, EvaluateParams, Operation, Rule};
use gitraptor_api::{Actor, AgentKind};
use gitraptor_git::{Ancestry, Count, Hide, PathLimits, RepoReader};
use gitraptor_policy::guard::plan::{Expected, MatchFacts, matches};

use super::evaluate::{self, DecideInput, RuleSet, Served};
use super::registry::GuardRegistry;
use crate::channel::authz::{AcceptedPeer, Checks};
pub use crate::channel::marks::ChildLink;
use crate::channel::marks::ExecutorMarks;
use crate::channel::requester::{self, Who};
pub use crate::executor::PlanBinding;

/// How a hook's caller relates to the executor.
#[derive(Debug, Clone)]
pub enum Link {
    /// Not under any running operation.
    None,
    /// The `git` the executor started, whose parent is the daemon.
    Child(ChildLink),
    /// Under a child of the executor without being it: decided again for `who`.
    Grandchild { who: Who },
}

/// Classifies `peer` against the running operations. The requester is resolved first, so the
/// start barrier (I-02) is respected; then the nearest `git` of the client is looked up among
/// the registered children, and its parent must be the daemon itself.
pub fn classify(peer: AcceptedPeer, checks: &Checks<'_>, marks: &ExecutorMarks) -> Link {
    let Ok(resolution) = requester::resolve(peer, checks, Some(marks)) else {
        return Link::None;
    };
    if resolution.executor_operation.is_none() {
        return Link::None;
    }
    let grandchild = || Link::Grandchild {
        who: resolution.who.clone(),
    };
    let Some((pid, start_us)) = super::second_line::nearest_git(peer, checks) else {
        return grandchild();
    };
    let Some(child) = marks.child_link(pid, start_us) else {
        return grandchild();
    };
    // ⛔ The parent of the child is the daemon, by pid and start time: a reused pid is not.
    let parent_is_daemon = checks.procs.read(pid).ok().is_some_and(|info| {
        info.start_us == start_us
            && checks.daemon.is_some_and(|(daemon_pid, daemon_start)| {
                info.ppid == daemon_pid
                    && checks
                        .procs
                        .read(daemon_pid)
                        .is_ok_and(|d| d.start_us == daemon_start)
            })
    });
    if parent_is_daemon {
        Link::Child(child)
    } else {
        grandchild()
    }
}

/// Adds `decision` to the late one of a plan: the maximum and the reasons of the maximum, the
/// warnings of both. The first decision's id is kept: it is the one the plan's entry and the
/// response carry.
fn accumulate(late: &mut Option<Decision>, decision: &Decision) {
    let Some(kept) = late.as_mut() else {
        *late = Some(decision.clone());
        return;
    };
    if decision.effect > kept.effect {
        kept.effect = decision.effect;
        kept.reasons.clone_from(&decision.reasons);
    } else if decision.effect == kept.effect && decision.effect != Effect::Allow {
        for reason in &decision.reasons {
            if !kept.reasons.contains(reason) {
                kept.reasons.push(reason.clone());
            }
        }
    }
    kept.applied_effect = match kept.effect {
        Effect::Ask => Effect::Deny,
        other => other,
    };
    for notice in &decision.notices {
        if !kept.notices.contains(notice) {
            kept.notices.push(notice.clone());
        }
    }
    if kept.applied_effect != Effect::Allow {
        kept.notices.clear();
    }
}

/// The decision for an operation of a direct child, and whether it was inherited from the plan.
/// A transition that is not registered is decided again with `decide`, and the result is kept
/// in the plan's `late`.
pub fn decide_for_child(
    child: &ChildLink,
    op: &Operation,
    authorship: Option<&AuthorshipFacts>,
    facts: &MatchFacts,
    decide: &mut dyn FnMut(&Operation) -> Decision,
) -> (Decision, bool) {
    let Some(binding) = &child.binding else {
        return (decide(op), false);
    };
    if matches(&child.transitions, op, authorship, facts) {
        return (binding.decision.clone(), true);
    }
    let decision = decide(op);
    let mut late = binding
        .late
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    accumulate(&mut late, &decision);
    (decision, false)
}

fn ancestor(reader: &RepoReader, old: &str, new: &str) -> bool {
    matches!(reader.push_ancestry(old, new), Ok(Ancestry::FastForward))
}

/// What matching `op` against `registered` needs from the repo, read with the bounds of the
/// hooks. Only what a registered transition asks for is read; what cannot be read is left out,
/// so it does not match.
///
/// The parents of a new commit are proved, not listed: `old` is an ancestor of the new value
/// and exactly one commit separates them, so `old` is its parent and anything else it merges
/// is already in `old`. A merge commit of an integrated branch is not proved (the merge arm
/// compares the integrated value); it is decided again.
pub fn match_facts(reader: &RepoReader, op: &Operation, registered: &[Expected]) -> MatchFacts {
    let mut out = MatchFacts::default();
    let Operation::RefTransaction { updates, .. } = op else {
        return out;
    };
    for update in updates {
        let (Some(old), Some(new)) = (update.old.oid(), update.new.oid()) else {
            continue;
        };
        for expected in registered {
            match expected {
                Expected::RebaseOf { refname, onto, .. } if *refname == update.refname => {
                    out.descends_from_onto = ancestor(reader, onto, new);
                }
                Expected::NewCommitOn {
                    refname, old: from, ..
                } if *refname == update.refname && from == old => {
                    let one = matches!(
                        reader.ahead_behind_commits(new, old, 2),
                        Ok((Count::Exact(1), _))
                    );
                    if one && ancestor(reader, old, new) {
                        out.parents = vec![old.to_owned()];
                        out.new_paths = reader
                            .fresh_commit_paths(
                                Some(old),
                                new,
                                &[],
                                Hide::OldOnly,
                                &PathLimits::default(),
                            )
                            .ok()
                            .filter(|found| !found.unverifiable)
                            .map(|found| found.paths);
                    }
                }
                _ => {}
            }
        }
        // One update is enough to read: the facts are per operation.
        if out.descends_from_onto || !out.parents.is_empty() {
            break;
        }
    }
    out
}

fn agent_of(actor: &Actor) -> Option<AgentKind> {
    match actor {
        Actor::Agent { kind, .. } => Some(*kind),
        Actor::Unattributed => None,
    }
}

/// `guard.evaluate` for a direct child of a governed plan: the plan's decision when the
/// transition is one of the child's, or a new one with the plan's layer, its requester and every
/// rule (never the hook connection's capabilities), kept in the plan's `late`. No entry of its
/// own: the plan leaves one.
pub fn serve_child(
    registry: &GuardRegistry,
    params: &EvaluateParams,
    child: &ChildLink,
    cwd: Option<&Path>,
) -> Served {
    if !evaluate::valid(params) {
        return evaluate::plain(evaluate::system_deny(Rule::InputRejected));
    }
    let common = Path::new(&params.common_dir);
    let facts = evaluate::open(common)
        .map(|reader| match_facts(&reader, &params.operation, &child.transitions))
        .unwrap_or_default();
    let layer = child
        .binding
        .as_ref()
        .map_or(gitraptor_api::guard::LogLayer::Hooks, |b| {
            super::plan_gate::log_layer(b.layer)
        });
    let input = DecideInput {
        common_dir: common,
        repo_id: &params.repo_id,
        actor: agent_of(&child.who.actor),
        worktree: cwd,
        layer,
        rules: RuleSet::all(),
        authorship: params.authorship.as_ref(),
        unregistered_bases: Some(&[]),
    };
    let (decision, _) = decide_for_child(
        child,
        &params.operation,
        params.authorship.as_ref(),
        &facts,
        &mut |op| evaluate::decide(registry, &input, op).decision,
    );
    evaluate::plain(decision)
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
