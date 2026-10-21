//! Evaluation of one governed operation (ADR-GRD-003 § 1 to § 4): the facts read from the repo,
//! the context and the pure function of `crates/policy`, wrapped in the contract's decision.
//! The daemon serves it on the connection thread (never through the loop); the hook client
//! runs the same code in degraded mode.

use std::path::Path;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    AuthorshipFacts, CommitStage, ConfigSource, ConfigStatus, Decision, Effect, EvaluateParams,
    ExceptionState, Level, LogLayer, Operation, Reason, Rule,
};
use gitraptor_git::{Ancestry, ReaderOptions, RefName, RepoReader};
use gitraptor_policy::guard::authorship::Effective;
use gitraptor_policy::guard::glob::{Kind, Pattern};
use gitraptor_policy::guard::plan::Preset;
use gitraptor_policy::guard::policies::{Policies, Rules, Scope};
use gitraptor_policy::guard::{self as policy, Context, Evaluation, Facts, FastForward};
use gitraptor_policy::layers::IgnoredRelaxation;
use gitraptor_policy::team::{DEFAULT_BRANCH, resolve_main_branch};

use super::registry::GuardRegistry;

/// Opens the repo for the decision reads: isolated from the environment (no `GIT_*`, no global
/// or system configuration) and without replacement objects (ADR-GRD-003 § 4).
pub fn open(common: &Path) -> Option<RepoReader> {
    RepoReader::open(
        common,
        &ReaderOptions {
            ignore_ambient_config: true,
            ..ReaderOptions::default()
        },
    )
    .ok()
}

/// What the reads prove about each `pre-push` update (H-05).
pub fn facts(reader: &RepoReader, op: &Operation) -> Facts {
    let Operation::Push { updates, .. } = op else {
        return Facts::default();
    };
    Facts {
        authorship: None,
        touched: Vec::new(),
        config_touched: Vec::new(),
        classified: Vec::new(),
        push: updates
            .iter()
            .map(|u| {
                // `push_update` does not look at a ref Guardrails does not govern: no ancestry.
                if !gitraptor_policy::guard::refs::is_governed(&u.remote_ref) {
                    return None;
                }
                let (Some(local), Some(remote)) = (u.local.oid(), u.remote.oid()) else {
                    return None;
                };
                // A read error leaves the fact out: the pure function treats it as forced.
                match reader.push_ancestry(remote, local).ok()? {
                    Ancestry::FastForward => Some(FastForward::Yes),
                    Ancestry::NotAncestor => Some(FastForward::NotAncestor),
                    Ancestry::RemoteMissing => Some(FastForward::RemoteMissing),
                    Ancestry::Shallow => Some(FastForward::Shallow),
                }
            })
            .collect(),
    }
}

/// `main` and the main branch of the repo (ADR-GRD-004 § 3.1–3.2), without duplicates: the
/// union the minimum protects while no base branch is confirmed.
pub fn default_bases(reader: &RepoReader) -> Vec<String> {
    let mut bases = vec![DEFAULT_BRANCH.to_owned()];
    if let Ok(main) = resolve_main_branch(reader)
        && !bases.iter().any(|b| b == main.name.as_str())
    {
        bases.push(main.name.as_str().to_owned());
    }
    bases
}

/// Whether the file system of the common directory does not distinguish case: the probe is
/// the file system itself, not a setting the repo (or an agent) can change.
pub fn folds_case(common: &Path) -> bool {
    common.join("HEAD").is_file() && std::fs::symlink_metadata(common.join("hEAD")).is_ok()
}

/// Who and under which authorship policy a commit is evaluated (US-GRD-018). The default is
/// "unattributed": no authorship rule applies (BR-EDGE-004).
#[derive(Debug, Clone, Default)]
pub struct CommitContext {
    pub actor: Option<AgentKind>,
    pub authorship: Effective,
    pub facts: Option<gitraptor_api::guard::AuthorshipFacts>,
    /// The protected branches and forbidden paths in force (US-GRD-008).
    pub policies: Policies,
}

/// Evaluates with the repo's own facts and case folding: `core.ignoreCase` or a file system
/// that folds case (SEC-GRD-18).
pub fn evaluate(
    reader: &RepoReader,
    common: &Path,
    op: &Operation,
    bases: Vec<String>,
) -> Evaluation {
    evaluate_commit(reader, common, op, bases, CommitContext::default())
}

/// [`evaluate`] with the actor, the authorship policy and the facts of the message.
pub fn evaluate_commit(
    reader: &RepoReader,
    common: &Path,
    op: &Operation,
    bases: Vec<String>,
    commit: CommitContext,
) -> Evaluation {
    evaluate_preset(reader, common, op, bases, commit, None, LogLayer::Hooks)
}

/// [`evaluate_commit`] with the facts a plan already holds (its prepared paths) and the layer
/// that asks. A preset applies to the single update of a transaction.
fn evaluate_preset(
    reader: &RepoReader,
    common: &Path,
    op: &Operation,
    bases: Vec<String>,
    commit: CommitContext,
    preset: Option<&Preset>,
    layer: LogLayer,
) -> Evaluation {
    let ctx = Context {
        bases,
        fold_case: reader.ignores_case() || folds_case(common),
        actor: commit.actor,
        authorship: commit.authorship,
        policies: commit.policies,
        layer,
        minimum: true,
        permissions: None,
    };
    let mut facts = facts(reader, op);
    facts.authorship = commit.facts;
    // The commits of the movement are read for the forbidden paths only when a rule governs the
    // actor; the protected configuration has its own read, for every agent's movement.
    facts.touched = match preset.and_then(|p| p.touched.clone()) {
        Some(touched) => vec![Some(touched)],
        None => super::policies::touched(reader, op, &ctx.policies, ctx.actor),
    };
    facts.config_touched = match preset.and_then(|p| p.config_touched.clone()) {
        // The person is never governed by the configuration rule: nothing to preload.
        Some(touched) if ctx.actor.is_some() => vec![Some(touched)],
        Some(_) => Vec::new(),
        None => super::policies::config_touched(reader, op, ctx.actor),
    };
    policy::evaluate(op, &facts, &ctx)
}

/// A fresh opaque decision id.
pub fn decision_id() -> String {
    let mut buf = [0u8; 16];
    if getrandom::fill(&mut buf).is_err() {
        // Ids only label decisions; uniqueness is not a security property.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        buf = t.to_le_bytes();
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// The contract's decision for an evaluation (ADR-GRD-003 § 3). Without the queue, `ask` is
/// applied as `deny` (S-GRD-9).
pub fn decision(eval: Evaluation) -> Decision {
    let applied_effect = match eval.effect {
        Effect::Ask => Effect::Deny,
        other => other,
    };
    // A warning only travels with a commit that goes ahead (D3).
    let notices = if applied_effect == Effect::Allow {
        eval.notices
    } else {
        Vec::new()
    };
    Decision {
        decision_id: decision_id(),
        effect: eval.effect,
        applied_effect,
        reasons: eval.reasons,
        notices,
        exception: ExceptionState::None,
        // US-GRD-001 reads no configuration: the minimum applies alone.
        config_status: vec![ConfigSource {
            source: Level::Floor,
            status: ConfigStatus::NotRead,
        }],
        config_ref: Vec::new(),
        prior_snapshot: None,
    }
}

/// A `deny` caused by the layer itself (input rejected, internal error, channel, repo).
pub fn system_deny(rule: Rule) -> Decision {
    decision(Evaluation {
        effect: Effect::Deny,
        reasons: vec![Reason {
            rule,
            level: Level::System,
            cause: None,
            params: Vec::new(),
        }],
        notices: Vec::new(),
    })
}

/// Lexical checks of what a hook client sends: refs follow `check-ref-format`, values are
/// object ids or symbolic targets.
pub(crate) fn valid(params: &EvaluateParams) -> bool {
    use gitraptor_api::guard::RefValue;
    let value = |v: &RefValue| match v {
        RefValue::Zero => true,
        RefValue::Oid(o) => {
            matches!(o.len(), 40 | 64) && o.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        }
        RefValue::Symbolic(t) => RefName::new(t).is_ok(),
    };
    let name = |n: &str| RefName::new(n).is_ok();
    let id_ok = !params.repo_id.is_empty()
        && params.repo_id.len() <= 64
        && params
            .repo_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-');
    let ops_ok = match &params.operation {
        Operation::Push { updates, .. } => {
            updates.len() <= 100_000
                && updates
                    .iter()
                    .all(|u| name(&u.remote_ref) && value(&u.local) && value(&u.remote))
        }
        Operation::RefTransaction {
            updates,
            orphan_head,
        } => {
            updates.len() <= 100_000
                && updates
                    .iter()
                    .all(|u| name(&u.refname) && value(&u.old) && value(&u.new))
                && orphan_head
                    .as_ref()
                    .is_none_or(|o| name(&o.branch) && value(&RefValue::Oid(o.oid.clone())))
        }
        Operation::Rebase { .. } | Operation::Commit { .. } => true,
    };
    let facts_ok = params
        .authorship
        .as_ref()
        .is_none_or(|f| f.coauthors.len() <= gitraptor_policy::authorship::MAX_COAUTHORS);
    id_ok && ops_ok && facts_ok && Path::new(&params.common_dir).is_absolute()
}

/// `guard.evaluate` in the daemon. A repo it does not know as protected (an orphan install, a
/// copied dispatcher) is evaluated with the union {`main`, main branch}: never less.
pub fn serve(registry: &GuardRegistry, params: &EvaluateParams) -> Decision {
    serve_as(registry, params, &Caller::default())
}

/// What the daemon knows of the hook client of a `commit` (US-GRD-018).
#[derive(Debug, Clone, Default)]
pub struct Caller {
    /// The agent resolved from the client's ancestry (D5).
    pub actor: Option<AgentKind>,
    /// The client's working directory: the worktree of the commit (Git runs hooks there).
    pub cwd: Option<std::path::PathBuf>,
    /// The connection was granted `guard.authorship`.
    pub authorship: bool,
    /// The connection was granted `guard.policies`: the actor of a `ref-transaction` or a
    /// `push` is resolved and the protected branches and forbidden paths apply (US-GRD-008).
    pub policies: bool,
    /// The `git` process that ran the hook, for an agent's commit (DS-US-GRD-018 D6).
    pub git: Option<super::second_line::GitProcess>,
    /// The second line does not evaluate this commit: the same `git` already had its decision,
    /// or it is certainly a rebase, cherry-pick, revert or am (§ 5.3).
    pub second_line_skip: bool,
}

/// [`serve`] for a caller the channel resolved.
pub fn serve_as(registry: &GuardRegistry, params: &EvaluateParams, caller: &Caller) -> Decision {
    serve_logged(registry, params, caller).0
}

/// What `guard.evaluate` decided, and what only the decision log needs.
#[derive(Debug, Clone)]
pub struct Served {
    pub decision: Decision,
    /// The authorship policy applied to a commit (`flexible`, ...), when one was.
    pub authorship_policy: Option<&'static str>,
    /// Relaxations the levels that only harden declared and that were ignored. Only for an
    /// agent's branch movement or push: never a reason of the decision.
    pub ignored: Vec<IgnoredRelaxation>,
}

/// [`serve_as`], and the authorship policy applied to a commit (`flexible`, ...) when one was:
/// the decision log records an agent's `flexible` commit (US-GRD-005, BR-AUTH-005).
pub fn serve_logged(
    registry: &GuardRegistry,
    params: &EvaluateParams,
    caller: &Caller,
) -> (Decision, Option<&'static str>) {
    let served = serve_audited(registry, params, caller);
    (served.decision, served.authorship_policy)
}

pub(crate) fn plain(decision: Decision) -> Served {
    Served {
        decision,
        authorship_policy: None,
        ignored: Vec::new(),
    }
}

/// Which rules a decision applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSet {
    /// The authorship rules of a commit (`guard.authorship`).
    pub authorship: bool,
    /// The protected branches and forbidden paths of a movement (`guard.policies`).
    pub policies: bool,
    /// The second line does not evaluate this commit (§ 5.3).
    pub second_line_skip: bool,
}

impl RuleSet {
    /// What the hook's connection asked for: a hook sees exactly what it saw before.
    pub fn from_caller(caller: &Caller) -> Self {
        Self {
            authorship: caller.authorship,
            policies: caller.policies,
            second_line_skip: caller.second_line_skip,
        }
    }

    /// Every rule: a plan of the executor, and a transition its child was not registered for.
    pub fn all() -> Self {
        Self {
            authorship: true,
            policies: true,
            second_line_skip: false,
        }
    }
}

/// One decision, as `guard.evaluate` and the executor's plans ask for it.
#[derive(Debug, Clone)]
pub struct DecideInput<'a> {
    /// The repo's common directory, absolute.
    pub common_dir: &'a Path,
    /// The repo id the asker names; only the registry's entry with the same `common_dir` counts.
    pub repo_id: &'a str,
    pub actor: Option<AgentKind>,
    /// The worktree of the operation: its own level of the configuration is read from it.
    pub worktree: Option<&'a Path>,
    /// The layer that asks; it never changes the decision.
    pub layer: LogLayer,
    pub rules: RuleSet,
    /// The facts of a commit message, for a commit.
    pub authorship: Option<&'a AuthorshipFacts>,
    /// For the executor: a repo the registry does not know protects, for everyone, the union of
    /// `main`, its main branch and these bases while nothing is confirmed. `None` for a hook.
    pub unregistered_bases: Option<&'a [String]>,
}

/// [`serve_logged`], and what the levels that only harden tried to relax (for the decision log).
pub fn serve_audited(registry: &GuardRegistry, params: &EvaluateParams, caller: &Caller) -> Served {
    if !valid(params) {
        return plain(system_deny(Rule::InputRejected));
    }
    let input = DecideInput {
        common_dir: Path::new(&params.common_dir),
        repo_id: &params.repo_id,
        actor: caller.actor,
        worktree: caller.cwd.as_deref(),
        layer: LogLayer::Hooks,
        rules: RuleSet::from_caller(caller),
        authorship: params.authorship.as_ref(),
        unregistered_bases: None,
    };
    decide_op(registry, &input, &params.operation, None)
}

/// The decision of `operation` (ADR-GRD-003 § 1 to § 4): the one function the hooks and the
/// executor's plans share.
pub fn decide(registry: &GuardRegistry, input: &DecideInput<'_>, operation: &Operation) -> Served {
    decide_op(registry, input, operation, None)
}

/// The decision of a plan: every operation decided with its preloaded facts, combined as the
/// maximum with the reasons of that maximum (BR-CALC-001). The warnings travel only when the
/// result allows. `preset` is aligned with `ops` (missing entries preload nothing).
pub fn decide_all(
    registry: &GuardRegistry,
    input: &DecideInput<'_>,
    ops: &[Operation],
    preset: &[Preset],
) -> Served {
    let mut total = Evaluation::allow();
    let mut notices = Vec::new();
    let mut authorship_policy = None;
    let mut ignored = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let (evaluation, policy, relaxed) = evaluate_op(registry, input, op, preset.get(i));
        let Some(evaluation) = evaluation else {
            return plain(system_deny(Rule::InternalError));
        };
        notices.extend(evaluation.notices.iter().cloned());
        for reason in evaluation.reasons {
            total.add(evaluation.effect, reason);
        }
        if evaluation.effect != Effect::Allow && total.effect == Effect::Allow {
            total.effect = evaluation.effect;
        }
        authorship_policy = authorship_policy.or(policy);
        ignored.extend(relaxed);
    }
    total.notices = notices;
    let mut out = decision(total);
    if !input.rules.authorship {
        out.notices.clear();
    }
    Served {
        decision: out,
        authorship_policy,
        ignored,
    }
}

fn decide_op(
    registry: &GuardRegistry,
    input: &DecideInput<'_>,
    operation: &Operation,
    preset: Option<&Preset>,
) -> Served {
    let (evaluation, applied, ignored) = evaluate_op(registry, input, operation, preset);
    let Some(evaluation) = evaluation else {
        return plain(system_deny(Rule::InternalError));
    };
    let mut out = decision(evaluation);
    if !input.rules.authorship {
        out.notices.clear();
    }
    Served {
        decision: out,
        authorship_policy: applied,
        ignored,
    }
}

/// The protected branches a repo unknown to the registry has for the executor (D12): the
/// union of bases, for everyone, at the level of the minimum.
fn unregistered_rules(bases: &[String]) -> Option<Rules> {
    let patterns: Vec<Pattern> = bases
        .iter()
        .filter_map(|b| Pattern::new(b, Kind::Branch).ok())
        .collect();
    (!patterns.is_empty()).then_some(Rules {
        level: Level::Minimum,
        scope: Scope::Everyone,
        patterns,
    })
}

/// The evaluation of one operation, or `None` when the repo cannot be read.
fn evaluate_op(
    registry: &GuardRegistry,
    input: &DecideInput<'_>,
    operation: &Operation,
    preset: Option<&Preset>,
) -> (
    Option<Evaluation>,
    Option<&'static str>,
    Vec<IgnoredRelaxation>,
) {
    let common_text = input.common_dir.to_string_lossy();
    let entry = registry
        .get(input.repo_id)
        .filter(|e| e.common_dir == common_text);
    let Some(reader) = open(input.common_dir) else {
        return (None, None, Vec::new());
    };
    let common = input.common_dir;
    // The local level is keyed by the registry's id, found through its `common_dir`: never by
    // the client's text alone.
    let repo_id = entry.is_some().then_some(input.repo_id);
    let registered = entry.is_some();
    let (mut bases, confirmed) = match entry {
        Some(e) => (e.bases, e.confirmed),
        None => (default_bases(&reader), None),
    };
    // D12: the executor protects the union while the repo has no confirmation of its own.
    let extra_rules = match input.unregistered_bases {
        Some(extra) if !registered => {
            for base in extra {
                if !bases.contains(base) {
                    bases.push(base.clone());
                }
            }
            unregistered_rules(&bases)
        }
        _ => None,
    };
    let mut ignored = Vec::new();
    let worktree_reader = || {
        input
            .worktree
            .and_then(|cwd| super::authorship::worktree_reader(cwd, common))
    };
    let commit = match operation {
        Operation::Commit {
            stage: CommitStage::SecondLine,
        } if input.rules.second_line_skip => CommitContext::default(),
        Operation::Commit { .. } if input.rules.authorship => {
            let worktree = worktree_reader();
            CommitContext {
                actor: input.actor,
                authorship: super::authorship::policy_for(
                    worktree.as_ref().unwrap_or(&reader),
                    confirmed.as_ref(),
                    registry.profile().as_ref(),
                    repo_id,
                ),
                facts: input.authorship.cloned(),
                policies: Policies::default(),
            }
        }
        Operation::RefTransaction { .. } | Operation::Push { .. } if input.rules.policies => {
            let worktree = worktree_reader();
            // One read of every level: the rules and the ignored relaxations come from it.
            let mut policies = match super::layers::load(
                worktree.as_ref().unwrap_or(&reader),
                confirmed.as_ref(),
                registry.profile().as_ref(),
                repo_id,
            ) {
                Ok(layers) => {
                    if input.actor.is_some() {
                        ignored = layers.ignored();
                    }
                    layers.policies()
                }
                // Never "no rules": an agent's movement cannot be judged (SEC-GRD-17).
                Err(_) => Policies::unreadable(),
            };
            if let Some(rules) = extra_rules {
                policies.branches.push(rules);
            }
            CommitContext {
                actor: input.actor,
                policies,
                ..CommitContext::default()
            }
        }
        _ => CommitContext::default(),
    };
    let applied =
        (commit.actor.is_some() && commit.facts.is_some()).then(|| commit.authorship.policy.mode());
    let evaluation = evaluate_preset(
        &reader,
        common,
        operation,
        bases,
        commit,
        preset,
        input.layer,
    );
    (Some(evaluation), applied, ignored)
}
