//! US-GRD-016 (part A): the commit plan of the executor (layer `mcp`) applies
//! `policy.commit-format` exactly as the `reference-transaction` hook applies it to the stored
//! commit, and fails closed on `policy.diff-size`: the size of a commit that does not exist yet
//! cannot be counted. The real `GuardrailsEngine` and the real hooks entry point
//! (`serve_audited`) over a temporary repo and profile (NFR-01).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{TempProfile, git};
use gitraptor_api::catalog::{GovernedAs, Layer};
use gitraptor_api::guard::{
    Cause, Decision, Effect, EvaluateParams, Hook, Operation, ParamKind, RefUpdate, RefValue, Rule,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin};
use gitraptor_core::executor::{GateRequest, GuardrailsGate};
use gitraptor_core::guardrails::evaluate::{Caller, serve_audited};
use gitraptor_core::guardrails::log::LogEntry;
use gitraptor_core::guardrails::plan_gate::{GuardrailsEngine, commit_facts, commit_header};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use gitraptor_policy::guard::git_command::GitCommand;
use gitraptor_policy::guard::permissions::Chain;
use gitraptor_policy::guard::plan::PlanShape;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};

const REPO_ID: &str = "0000-ffff";
const FORMAT: &str = r#"{"policies":{"commitFormat":{}}}"#;
const SIZE: &str = r#"{"policies":{"diffSize":{"maxLines":400}}}"#;
/// The trailer an agent's commit carries, so the authorship rule never decides here.
const TRAILER: &str = "\n\nCo-Authored-By: Claude <noreply@anthropic.com>";

fn with_trailer(first: &str) -> String {
    format!("{first}{TRAILER}")
}

struct Rig {
    _profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    registry: Arc<GuardRegistry>,
    engine: GuardrailsEngine,
    /// The tip of `feat-x`.
    tip: String,
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

/// A repo whose `main` carries `team` as the confirmed floor, a branch `feat-x` checked out, an
/// empty profile and the engine attached.
fn rig(team: &str) -> Rig {
    let repo = tempfile::tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(p.join(".gitraptor")).unwrap();
    std::fs::write(p.join(".gitraptor/settings.json"), team).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "chore: settings"]);
    let blob = git(p, &["rev-parse", "main:.gitraptor/settings.json"]);
    git(p, &["switch", "-q", "-c", "feat-x"]);
    let tip = git(p, &["rev-parse", "HEAD"]).trim().to_owned();
    let common = gitraptor_git::paths::canonicalize(&p.join(".git")).unwrap();
    let tp = TempProfile::new();
    std::fs::create_dir_all(tp.dirs().config).unwrap();
    std::fs::write(tp.dirs().config.join("settings.json"), "{}").unwrap();
    let registry = Arc::new(GuardRegistry::for_profile(tp.dirs()));
    registry.set(
        REPO_ID,
        GuardEntry {
            common_dir: common.to_string_lossy().into_owned(),
            bases: vec!["main".into()],
            confirmed: Some(Confirmed {
                base_branch: gitraptor_git::RefName::new("main").unwrap(),
                floor: ConfirmedFloor::Blob(blob.trim().to_owned()),
            }),
        },
    );
    let entries = Arc::new(Mutex::new(Vec::new()));
    let engine = GuardrailsEngine::default();
    let kept = Arc::clone(&entries);
    engine.attach(
        &registry,
        Arc::new(move |entry| {
            kept.lock().unwrap().push(entry);
            true
        }),
    );
    Rig {
        _profile: tp,
        repo,
        common,
        registry,
        engine,
        tip,
        entries,
    }
}

fn agent() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn kind_of(actor: &Actor) -> Option<AgentKind> {
    match actor {
        Actor::Agent { kind, .. } => Some(*kind),
        Actor::Unattributed => None,
    }
}

/// A commit of one small file on `feat-x` with `message`, written as an object only: the branch
/// does not move, so the hook reads it as the new commit of the transaction.
fn stored_commit(r: &Rig, message: &str) -> String {
    let p = r.repo.path();
    std::fs::write(p.join("a.txt"), "one\ntwo\n").unwrap();
    git(p, &["add", "a.txt"]);
    let tree = git(p, &["write-tree"]).trim().to_owned();
    git(p, &["commit-tree", &tree, "-p", &r.tip, "-m", message])
        .trim()
        .to_owned()
}

/// `(rule, level, cause, effect)` of every reason, sorted: what decides, never the text.
fn reasons(d: &Decision) -> Vec<String> {
    let mut out: Vec<String> = d
        .reasons
        .iter()
        .map(|x| format!("{:?}/{:?}/{:?}/{:?}", x.rule, x.level, x.cause, d.effect))
        .collect();
    out.sort();
    out
}

/// What the `reference-transaction` of `git commit` decides for the stored commit `new`.
fn hook(r: &Rig, actor: &Actor, new: &str) -> Decision {
    let caller = Caller {
        actor: kind_of(actor),
        cwd: Some(r.repo.path().to_owned()),
        authorship: true,
        policies: true,
        commit_policies: true,
        permissions: true,
        chain: Some(Chain::new(vec![GitCommand::Certain("commit", None)])),
        ..Caller::default()
    };
    let params = EvaluateParams {
        repo_id: REPO_ID.into(),
        common_dir: r.common.to_string_lossy().into_owned(),
        hook: Hook::ReferenceTransaction,
        operation: Operation::RefTransaction {
            updates: vec![RefUpdate {
                refname: "refs/heads/feat-x".into(),
                old: RefValue::Oid(r.tip.clone()),
                new: RefValue::Oid(new.to_owned()),
            }],
            orphan_head: None,
        },
        authorship: None,
        exception_token: None,
    };
    serve_audited(&r.registry, &params, &caller).decision
}

/// What the engine decides for the commit plan of `message` by `actor`, layer `mcp`.
fn plan(r: &Rig, actor: &Actor, message: &str) -> Decision {
    let req = GateRequest {
        repo_id: REPO_ID.into(),
        operation: GovernedAs::Commit,
        layer: Layer::Mcp,
        actor: actor.clone(),
        fingerprint: "f".repeat(64),
        plan_id: "p-commit".into(),
        common_dir: r.common.clone(),
        worktree: r.repo.path().to_owned(),
        branch: Some("feat-x".into()),
        shape: Some(PlanShape::Commit {
            branch: "refs/heads/feat-x".into(),
            old: r.tip.clone(),
            paths: vec!["a.txt".into()],
            authorship: commit_facts(&r.common, message),
            header: commit_header(&r.common, message),
        }),
    };
    r.engine
        .evaluate_detailed(&req)
        .decision
        .expect("the engine decided the plan")
}

fn commit_policy_reasons(d: &Decision) -> Vec<String> {
    reasons(d)
        .into_iter()
        .filter(|x| x.starts_with("CommitFormat/") || x.starts_with("DiffSize/"))
        .collect()
}

/// A message out of format: the plan gives the hook's `(rule, level, cause, effect)`, its
/// reason names no commit (it does not exist yet), and it leaves one entry, layer `mcp`.
#[test]
fn a_message_out_of_format_gets_the_hooks_reason_without_an_oid() {
    let r = rig(FORMAT);
    let message = &with_trailer("arreglos varios");
    let from_hook = hook(&r, &agent(), &stored_commit(&r, message));
    assert_eq!(
        from_hook.applied_effect,
        Effect::Deny,
        "fixture: {from_hook:?}"
    );
    let expected = commit_policy_reasons(&from_hook);
    assert!(
        expected.iter().any(|x| x.starts_with("CommitFormat/")),
        "fixture: the hook names the format: {from_hook:?}"
    );
    let from_plan = plan(&r, &agent(), message);
    assert_eq!(
        (from_plan.effect, from_plan.applied_effect),
        (from_hook.effect, from_hook.applied_effect)
    );
    assert_eq!(commit_policy_reasons(&from_plan), expected, "{from_plan:?}");
    let format = from_plan
        .reasons
        .iter()
        .find(|x| x.rule == Rule::CommitFormat)
        .expect("the plan names the format");
    assert!(
        format.params.iter().all(|p| p.kind != ParamKind::Oid),
        "a commit that does not exist has no oid: {format:?}"
    );
    r.engine.record_close(
        &GateRequest {
            repo_id: REPO_ID.into(),
            operation: GovernedAs::Commit,
            layer: Layer::Mcp,
            actor: agent(),
            fingerprint: "f".repeat(64),
            plan_id: "p-commit".into(),
            common_dir: r.common.clone(),
            worktree: r.repo.path().to_owned(),
            branch: Some("feat-x".into()),
            shape: None,
        },
        gitraptor_core::executor::PlanClose::Dropped,
    );
    let entries = r.entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].layer, gitraptor_api::guard::LogLayer::Mcp);
    assert!(
        entries[0]
            .reasons
            .iter()
            .any(|x| x.rule == Rule::CommitFormat),
        "{entries:?}"
    );
}

/// A conforming message: both allow.
#[test]
fn a_conforming_message_is_allowed() {
    let r = rig(FORMAT);
    let message = &with_trailer("feat: a change");
    let from_hook = hook(&r, &agent(), &stored_commit(&r, message));
    assert_eq!(
        from_hook.applied_effect,
        Effect::Allow,
        "fixture: {from_hook:?}"
    );
    let from_plan = plan(&r, &agent(), message);
    assert_eq!(from_plan.applied_effect, Effect::Allow, "{from_plan:?}");
    assert!(
        commit_policy_reasons(&from_plan).is_empty(),
        "{from_plan:?}"
    );
}

/// The person is not governed by a rule that applies to agents: both allow.
#[test]
fn the_person_is_not_governed() {
    let r = rig(FORMAT);
    let message = "arreglos varios";
    let from_hook = hook(&r, &Actor::Unattributed, &stored_commit(&r, message));
    assert!(
        commit_policy_reasons(&from_hook).is_empty(),
        "fixture: {from_hook:?}"
    );
    let from_plan = plan(&r, &Actor::Unattributed, message);
    assert!(
        commit_policy_reasons(&from_plan).is_empty(),
        "{from_plan:?}"
    );
    assert_eq!(from_plan.applied_effect, Effect::Allow, "{from_plan:?}");
}

/// A size rule that governs the agent: the hook counts the stored commit and allows it, the plan
/// cannot count a commit that does not exist and denies it as unverifiable (stricter, never
/// laxer).
#[test]
fn a_size_rule_fails_closed_in_the_plan() {
    let r = rig(SIZE);
    let message = &with_trailer("feat: a small change");
    let from_hook = hook(&r, &agent(), &stored_commit(&r, message));
    assert_eq!(
        from_hook.applied_effect,
        Effect::Allow,
        "fixture: {from_hook:?}"
    );
    let from_plan = plan(&r, &agent(), message);
    assert_eq!(from_plan.applied_effect, Effect::Deny, "{from_plan:?}");
    let size = from_plan
        .reasons
        .iter()
        .find(|x| x.rule == Rule::DiffSize)
        .expect("the plan names the size rule");
    assert_eq!(size.cause, Some(Cause::Unverifiable), "{size:?}");
    assert!(
        size.params.iter().all(|p| p.kind != ParamKind::Oid),
        "{size:?}"
    );
}
