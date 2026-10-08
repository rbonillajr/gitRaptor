//! Evaluation of one governed operation (ADR-GRD-003 § 1 to § 4): the facts read from the repo,
//! the context and the pure function of `crates/policy`, wrapped in the contract's decision.
//! The daemon serves it on the connection thread (never through the loop); the hook client
//! runs the same code in degraded mode.

use std::path::Path;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    CommitStage, ConfigSource, ConfigStatus, Decision, Effect, EvaluateParams, ExceptionState,
    Level, Operation, Reason, Rule,
};
use gitraptor_git::{Ancestry, ReaderOptions, RefName, RepoReader};
use gitraptor_policy::guard::authorship::Effective;
use gitraptor_policy::guard::{self as policy, Context, Evaluation, Facts, FastForward};
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
        push: updates
            .iter()
            .map(|u| {
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
    let ctx = Context {
        bases,
        fold_case: reader.ignores_case() || folds_case(common),
        actor: commit.actor,
        authorship: commit.authorship,
    };
    let mut facts = facts(reader, op);
    facts.authorship = commit.facts;
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
fn valid(params: &EvaluateParams) -> bool {
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

/// [`serve_as`], and the authorship policy applied to a commit (`flexible`, …) when one was:
/// the decision log records an agent's `flexible` commit (US-GRD-005, BR-AUTH-005).
pub fn serve_logged(
    registry: &GuardRegistry,
    params: &EvaluateParams,
    caller: &Caller,
) -> (Decision, Option<&'static str>) {
    if !valid(params) {
        return (system_deny(Rule::InputRejected), None);
    }
    let entry = registry
        .get(&params.repo_id)
        .filter(|e| e.common_dir == params.common_dir);
    let Some(reader) = open(Path::new(&params.common_dir)) else {
        return (system_deny(Rule::InternalError), None);
    };
    let common = Path::new(&params.common_dir);
    let (bases, confirmed) = match entry {
        Some(e) => (e.bases, e.confirmed),
        None => (default_bases(&reader), None),
    };
    let commit = match &params.operation {
        Operation::Commit {
            stage: CommitStage::SecondLine,
        } if caller.second_line_skip => CommitContext::default(),
        Operation::Commit { .. } if caller.authorship => {
            let worktree = caller
                .cwd
                .as_deref()
                .and_then(|cwd| super::authorship::worktree_reader(cwd, common));
            CommitContext {
                actor: caller.actor,
                authorship: super::authorship::policy_for(
                    worktree.as_ref().unwrap_or(&reader),
                    confirmed.as_ref(),
                    registry.profile().as_ref(),
                ),
                facts: params.authorship.clone(),
            }
        }
        _ => CommitContext::default(),
    };
    let applied =
        (commit.actor.is_some() && commit.facts.is_some()).then(|| commit.authorship.policy.mode());
    let mut out = decision(evaluate_commit(
        &reader,
        common,
        &params.operation,
        bases,
        commit,
    ));
    if !caller.authorship {
        out.notices.clear();
    }
    (out, applied)
}
