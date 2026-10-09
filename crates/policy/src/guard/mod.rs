//! Guardrails decision function (ADR-GRD-003 § 1 and § 2): pure and
//! deterministic, no I/O, no clock, no randomness. The daemon hosts it for the
//! hook layer and the hook client runs the same function in degraded mode, so
//! both layers decide alike (BR-CONS-002).
//!
//! The safe minimum (BR-EDGE-001) is evaluated from the context alone and nothing can turn it
//! off; the protected branches, the forbidden paths and the authorship policy come from the
//! configuration the caller resolved, and the protection of the Guardrails configuration
//! (BR-AUTH-004) from no configuration at all.

pub mod authorship;
pub mod config;
pub mod fastpath;
pub mod glob;
pub mod input;
pub mod policies;
pub mod refs;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    AuthorshipFacts, Cause, Effect, Level, NotPreventable, Operation, Param, ParamKind, PushUpdate,
    Reason, RefBackend, RefUpdate, Rule,
};

/// What the client proved about a `pre-push` update whose local and remote
/// sides both exist (ADR-GRD-002 § 1, H-05). Read without replacement
/// objects, grafts or the commit-graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastForward {
    Yes,
    NotAncestor,
    RemoteMissing,
    Shallow,
}

/// Facts the I/O side computed for an operation, aligned with its updates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// One entry per `pre-push` update; `None` where no fact applies (a
    /// creation or a deletion). A missing fact where one is needed is treated
    /// as a forced update (fail-closed).
    pub push: Vec<Option<FastForward>>,
    /// What the hook client derived from the commit message (US-GRD-018, D7).
    pub authorship: Option<AuthorshipFacts>,
    /// What the commits of each update bring (US-GRD-008, D5), aligned with the updates of a
    /// `pre-push` or a `reference-transaction`; `None` where nothing was read (no forbidden-path
    /// rule governs the actor, or the update brings no commit).
    pub touched: Vec<Option<policies::Touched>>,
}

/// The context of an evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// Base branches the minimum protects, short names (the union while the
    /// base is unconfirmed or in degraded mode; ADR-GRD-004 § 3).
    pub bases: Vec<String>,
    /// The repo's file system does not distinguish case (`core.ignoreCase`).
    pub fold_case: bool,
    /// The agent the daemon resolved for the operation (D5); `None` when unattributed.
    pub actor: Option<AgentKind>,
    /// `policies.commitAuthorship` in force (D2).
    pub authorship: authorship::Effective,
    /// The protected branches and forbidden paths in force (US-GRD-008).
    pub policies: policies::Policies,
}

/// The effect and every rule that produces it (BR-CALC-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub effect: Effect,
    pub reasons: Vec<Reason>,
    /// Rules that warn without changing the effect (US-GRD-018, D3).
    pub notices: Vec<Reason>,
}

impl Evaluation {
    pub fn allow() -> Self {
        Self {
            effect: Effect::Allow,
            reasons: Vec::new(),
            notices: Vec::new(),
        }
    }

    /// Adds a warning: it never raises the effect (D3).
    pub fn notice(&mut self, reason: Reason) {
        if !self.notices.contains(&reason) {
            self.notices.push(reason);
        }
    }

    /// Adds a reason and keeps the most restrictive effect; only the reasons
    /// that produce the final effect are kept.
    pub fn add(&mut self, effect: Effect, reason: Reason) {
        if effect > self.effect {
            self.effect = effect;
            self.reasons.clear();
        }
        if effect == self.effect && effect != Effect::Allow && !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

/// Evaluates an operation against the safe minimum.
pub fn evaluate(operation: &Operation, facts: &Facts, ctx: &Context) -> Evaluation {
    let mut out = Evaluation::allow();
    // One budget for everything matched in this evaluation (US-GRD-008, D5).
    let mut budget = glob::Budget::default();
    match operation {
        Operation::Push { remote, updates } => {
            for (i, update) in updates.iter().enumerate() {
                let fact = facts.push.get(i).copied().flatten();
                push_update(&mut out, remote.raw(), update, fact, ctx);
            }
            // The policies are their own pass: the minimum returns early on creations,
            // deletions and fast-forwards (Architect, D4).
            for (i, update) in updates.iter().enumerate() {
                policies_of(
                    &mut out,
                    &update.remote_ref,
                    facts.touched.get(i).and_then(Option::as_ref),
                    ctx,
                    &mut budget,
                );
            }
        }
        Operation::RefTransaction {
            updates,
            orphan_head,
        } => {
            for update in updates {
                ref_update(&mut out, update, orphan_head.as_ref(), ctx);
            }
            for (i, update) in updates.iter().enumerate() {
                policies_of(
                    &mut out,
                    &update.refname,
                    facts.touched.get(i).and_then(Option::as_ref),
                    ctx,
                    &mut budget,
                );
            }
        }
        // Nothing of the minimum governs a rebase (US-GRD-007 adds rules).
        Operation::Rebase { .. } => {}
        Operation::Commit { stage } => authorship::evaluate(
            &mut out,
            *stage,
            ctx.actor,
            facts.authorship.as_ref(),
            ctx.authorship,
        ),
    }
    out
}

/// The protected branches and forbidden paths of one moved ref.
fn policies_of(
    out: &mut Evaluation,
    refname: &str,
    touched: Option<&policies::Touched>,
    ctx: &Context,
    budget: &mut glob::Budget,
) {
    if !refs::is_governed(refname) {
        return;
    }
    // A product rule: it holds whatever the configuration says, even with no policies at all.
    if let Some(touched) = touched {
        config::protect_config(out, touched, ctx.actor, budget);
    }
    if ctx.policies.is_empty() {
        return;
    }
    policies::protected_branch(out, refname, ctx.actor, &ctx.policies, budget);
    if let Some(touched) = touched {
        policies::forbidden_paths(out, touched, ctx.actor, &ctx.policies, budget);
    }
}

fn base_delete(cause: Cause, params: Vec<Param>) -> Reason {
    Reason {
        rule: Rule::MinimumBaseBranchDelete,
        level: Level::Minimum,
        cause: Some(cause),
        params,
    }
}

/// Denies any governed name that is an alias of a base branch (SEC-GRD-18).
/// Applies to every line, not only deletions (E-02-10).
fn alias(out: &mut Evaluation, refname: &str, ctx: &Context) -> bool {
    for base in &ctx.bases {
        if refs::matches_branch(refname, base, ctx.fold_case) == refs::Match::Alias {
            out.add(
                Effect::Deny,
                base_delete(
                    Cause::Alias,
                    vec![
                        Param::new(ParamKind::Ref, refname),
                        Param::new(ParamKind::Base, base.as_str()),
                    ],
                ),
            );
            return true;
        }
    }
    false
}

fn is_base<'a>(refname: &str, ctx: &'a Context) -> Option<&'a str> {
    ctx.bases
        .iter()
        .find(|b| refs::matches_branch(refname, b, ctx.fold_case) == refs::Match::Exact)
        .map(String::as_str)
}

fn push_update(
    out: &mut Evaluation,
    remote: &str,
    u: &PushUpdate,
    fact: Option<FastForward>,
    ctx: &Context,
) {
    if !refs::is_governed(&u.remote_ref) || alias(out, &u.remote_ref, ctx) {
        return;
    }
    if u.local.is_zero() {
        if let Some(base) = is_base(&u.remote_ref, ctx) {
            out.add(
                Effect::Deny,
                base_delete(
                    Cause::Delete,
                    vec![
                        Param::new(ParamKind::Branch, base),
                        Param::new(ParamKind::Remote, remote),
                    ],
                ),
            );
        }
        return;
    }
    if u.remote.is_zero() {
        return; // creation
    }
    let cause = match fact {
        Some(FastForward::Yes) => return,
        Some(FastForward::NotAncestor) => Cause::NotFastForward,
        Some(FastForward::Shallow) => Cause::ShallowHistory,
        Some(FastForward::RemoteMissing) | None => Cause::RemoteObjectMissing,
    };
    out.add(
        Effect::Deny,
        Reason {
            rule: Rule::MinimumForcePush,
            level: Level::Minimum,
            cause: Some(cause),
            params: vec![
                Param::new(ParamKind::Branch, refs::short(&u.remote_ref)),
                Param::new(ParamKind::Remote, remote),
            ],
        },
    );
}

fn ref_update(
    out: &mut Evaluation,
    u: &RefUpdate,
    orphan: Option<&gitraptor_api::guard::OrphanHead>,
    ctx: &Context,
) {
    if !refs::is_governed(&u.refname) || alias(out, &u.refname, ctx) {
        return;
    }
    if !u.new.is_zero() {
        return;
    }
    let Some(base) = is_base(&u.refname, ctx) else {
        return;
    };
    let reason = match orphan {
        Some(o) => base_delete(
            Cause::RenameOntoBase,
            vec![
                Param::new(ParamKind::Base, base),
                Param::new(ParamKind::Branch, o.branch.as_str()),
                Param::new(ParamKind::Oid, o.oid.as_str()),
            ],
        ),
        None => base_delete(Cause::Delete, vec![Param::new(ParamKind::Branch, base)]),
    };
    out.add(Effect::Deny, reason);
}

/// What the hook layer cannot prevent in a repo with this refs backend
/// (ADR-GRD-002 § 3, verified in macOS by SPIKE-GRD-001; Linux and Windows
/// pending). Versioned with the binary.
pub fn not_preventable(backend: RefBackend) -> Vec<NotPreventable> {
    let mut list = vec![
        NotPreventable::ResetHard,
        NotPreventable::Merge,
        NotPreventable::RemoveWorktree,
        NotPreventable::CreateWorktreeUnrecognized,
        NotPreventable::VoluntarySkips,
        NotPreventable::PolicyActor,
        NotPreventable::PolicyReach,
        NotPreventable::PolicyFloor,
    ];
    if backend == RefBackend::Reftable {
        list.push(NotPreventable::RenameBaseReftable);
    }
    list
}

#[cfg(test)]
mod tests;
