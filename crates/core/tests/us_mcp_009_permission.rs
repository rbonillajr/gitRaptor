//! US-MCP-009 (DS-009 T008, D10): the decision that counts of a `commit` plan applies the
//! permission of US-GRD-007 like the `pre-commit` hook does, the single log entry keeps the
//! `commit-msg` stage first (guard 8.1) and carries a rule once (guard 8.2), and the hooks of the
//! `git` the executor starts reuse the decision bound to the plan. The real `GuardrailsEngine`
//! over a temporary repo and profile (NFR-01); no process is started but `git` for the fixture.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{TempProfile, git};
use gitraptor_api::catalog::{GovernedAs, Layer, OperationOutcome};
use gitraptor_api::guard::{
    CommitStage, Decision, Effect, EvaluateParams, Hook, LogKind, LoggedOperation, Operation, Rule,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin};
use gitraptor_core::channel::marks::ChildLink;
use gitraptor_core::executor::{GateDecision, GateRequest, GuardrailsGate, PlanClose};
use gitraptor_core::guardrails::link::serve_child;
use gitraptor_core::guardrails::log::LogEntry;
use gitraptor_core::guardrails::plan_gate::{GuardrailsEngine, commit_facts, commit_header};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use gitraptor_policy::guard::plan::PlanShape;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};

const REPO_ID: &str = "0000-ffff";
const NO_POLICY: &str = "{}";
const HUMAN_AUTHOR_DENY: &str = r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#;
const HUMAN_AUTHOR_WARN: &str =
    r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;
const MESSAGE: &str = "feat: a change\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n";

struct Rig {
    _profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    registry: Arc<GuardRegistry>,
    engine: GuardrailsEngine,
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

/// A repo whose `main` carries `team` as the floor, a profile whose settings are `profile`, and
/// the engine attached to both.
fn rig(team: &str, profile: &str) -> Rig {
    let repo = tempfile::tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(p.join(".gitraptor")).unwrap();
    std::fs::write(p.join(".gitraptor/settings.json"), team).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "a"]);
    let blob = git(p, &["rev-parse", "main:.gitraptor/settings.json"]);
    git(p, &["switch", "-q", "-c", "feat-x"]);
    let common = gitraptor_git::paths::canonicalize(&p.join(".git")).unwrap();
    let tp = TempProfile::new();
    std::fs::create_dir_all(tp.dirs().config).unwrap();
    std::fs::write(tp.dirs().config.join("settings.json"), profile).unwrap();
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
        entries,
    }
}

impl Rig {
    fn request(&self, plan_id: &str) -> GateRequest {
        let head = git(self.repo.path(), &["rev-parse", "HEAD"]);
        GateRequest {
            repo_id: REPO_ID.into(),
            operation: GovernedAs::Commit,
            layer: Layer::Mcp,
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            fingerprint: "f".repeat(64),
            plan_id: plan_id.into(),
            common_dir: self.common.clone(),
            worktree: self.repo.path().to_owned(),
            branch: Some("feat-x".into()),
            shape: Some(PlanShape::Commit {
                branch: "feat-x".into(),
                old: head.trim().to_owned(),
                paths: vec!["src/a.rs".into()],
                authorship: commit_facts(&self.common, MESSAGE),
                header: commit_header(&self.common, MESSAGE),
            }),
        }
    }

    fn entries(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().clone()
    }
}

fn rules(decision: &Decision, rule: Rule) -> usize {
    decision.reasons.iter().filter(|r| r.rule == rule).count()
}

#[test]
fn a_commit_plan_with_commit_in_ask_is_viewed_as_ask() {
    let r = rig(NO_POLICY, r#"{"permissions":{"ask":["commit"]}}"#);
    let verdict = r.engine.evaluate_detailed(&r.request("p-ask"));
    assert_eq!(
        verdict.view,
        GateDecision::Ask,
        "a commit plan with `commit` in ask must be viewed as ask (the plan applies the \
         permission, not only the hook): {:?}",
        verdict.decision
    );
    let d = verdict
        .decision
        .expect("an asked plan carries its decision");
    assert_eq!(
        (d.effect, d.applied_effect),
        (Effect::Ask, Effect::Deny),
        "ask is applied as deny while there is no queue: {d:?}"
    );
    assert_eq!(rules(&d, Rule::PermissionCommit), 1, "{d:?}");
    assert!(verdict.binding.is_none(), "an asked plan binds nothing");
}

#[test]
fn a_commit_plan_with_commit_in_deny_is_denied_with_permission_commit() {
    let r = rig(NO_POLICY, r#"{"permissions":{"deny":["commit"]}}"#);
    let req = r.request("p-deny");
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(
        verdict.view,
        GateDecision::Deny,
        "a commit plan with `commit` in deny must be denied: {:?}",
        verdict.decision
    );
    let d = verdict
        .decision
        .expect("a denied plan carries its decision");
    assert_eq!(
        rules(&d, Rule::PermissionCommit),
        1,
        "the denial must name permission.commit once: {d:?}"
    );
    r.engine.record_close(&req, PlanClose::Denied);
    let entries = r.entries();
    assert_eq!(entries.len(), 1, "one entry per plan: {entries:?}");
    assert_eq!(entries[0].kind, LogKind::Denial);
    assert!(
        entries[0]
            .reasons
            .iter()
            .any(|x| x.rule == Rule::PermissionCommit),
        "the entry must carry permission.commit: {:?}",
        entries[0].reasons
    );
}

/// Guard 8.1: whichever stage the plan evaluates last, the entry names `commit-msg`.
#[test]
fn the_commit_plan_logs_the_commit_msg_stage_first() {
    let r = rig(HUMAN_AUTHOR_DENY, NO_POLICY);
    let req = r.request("p-stage");
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(verdict.view, GateDecision::Deny, "{:?}", verdict.decision);
    r.engine.record_close(&req, PlanClose::Denied);
    let entries = r.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].operation,
        LoggedOperation::Commit {
            stage: CommitStage::CommitMsg
        },
        "the entry of a commit plan must keep the commit-msg stage (pre-commit is evaluated last)"
    );
}

/// Guard 8.2: with `human-author` in deny both stages add the same reason; it is carried once.
#[test]
fn a_human_author_denial_carries_its_rule_once() {
    let r = rig(HUMAN_AUTHOR_DENY, NO_POLICY);
    let req = r.request("p-once");
    let verdict = r.engine.evaluate_detailed(&req);
    let d = verdict.decision.expect("a denial carries its decision");
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(
        rules(&d, Rule::AuthorshipHumanAuthor),
        1,
        "decide_all must not repeat a reason already added: {d:?}"
    );
    r.engine.record_close(&req, PlanClose::Denied);
    let entry = &r.entries()[0];
    assert_eq!(
        entry
            .reasons
            .iter()
            .filter(|x| x.rule == Rule::AuthorshipHumanAuthor)
            .count(),
        1,
        "the entry carries the rule once: {:?}",
        entry.reasons
    );
}

/// D10: the hooks of the plan's `git` (pre-commit, commit-msg) get the bound decision, add
/// nothing to the plan and leave no entry; the plan closes with its single entry, with the
/// authorship notice once.
#[test]
fn the_hooks_under_a_safe_commit_reuse_the_bound_decision_and_log_once() {
    let r = rig(HUMAN_AUTHOR_WARN, NO_POLICY);
    let req = r.request("p-hooks");
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(verdict.view, GateDecision::Allow, "{:?}", verdict.decision);
    let binding = verdict.binding.expect("an allowed plan binds its decision");
    let child = ChildLink {
        operation_id: "op-1".into(),
        who: binding.who.clone(),
        binding: Some(Arc::clone(&binding)),
        transitions: Arc::clone(&binding.registered),
    };
    for (hook, stage) in [
        (Hook::PreCommit, CommitStage::PreCommit),
        (Hook::CommitMsg, CommitStage::CommitMsg),
    ] {
        let params = EvaluateParams {
            repo_id: REPO_ID.into(),
            common_dir: r.common.to_string_lossy().into_owned(),
            hook,
            operation: Operation::Commit { stage },
            authorship: Some(commit_facts(&r.common, MESSAGE)),
            exception_token: None,
        };
        let served = serve_child(&r.registry, &params, &child, Some(r.repo.path()));
        assert_eq!(
            served.decision.decision_id, binding.decision.decision_id,
            "the {stage:?} hook must get the decision bound to the plan"
        );
    }
    assert!(
        binding.late.lock().unwrap().is_none(),
        "the hooks of the plan's git must not decide again"
    );
    assert!(
        r.entries().is_empty(),
        "the hooks leave no entry of their own"
    );
    r.engine
        .record_close(&req, PlanClose::Ran(OperationOutcome::Done));
    let entries = r.entries();
    assert_eq!(entries.len(), 1, "one entry per plan: {entries:?}");
    assert_eq!(entries[0].kind, LogKind::Notice, "{entries:?}");
    assert_eq!(
        entries[0]
            .reasons
            .iter()
            .filter(|x| x.rule == Rule::AuthorshipHumanAuthor)
            .count(),
        1,
        "the notice is carried once: {:?}",
        entries[0].reasons
    );
}
