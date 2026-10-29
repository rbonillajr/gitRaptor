//! US-GRD-016 (DS T001): the plan of the executor (layer `mcp`) gets the decision the hooks give
//! to the operations they would evaluate, with the same effect, applied effect and list of
//! `(rule, level, cause)`; `decision_id` is never compared. The real `GuardrailsEngine` and the
//! real hooks entry point (`serve_audited`) over a temporary repo and profile (NFR-01).
//!
//! US-GRD-009: policy.diff-size y policy.commit-format, cuando existan
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{TempProfile, git};
use gitraptor_api::catalog::{GovernedAs, Layer};
use gitraptor_api::guard::{
    CommitStage, Decision, Effect, EvaluateParams, Hook, Level, LogLayer, Operation, PushUpdate,
    RefUpdate, RefValue, Rule,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted};
use gitraptor_core::executor::{GateDecision, GateRequest, GuardrailsGate};
use gitraptor_core::guardrails::evaluate::{Caller, DecideInput, RuleSet, decide, serve_audited};
use gitraptor_core::guardrails::log::LogEntry;
use gitraptor_core::guardrails::plan_gate::{GuardrailsEngine, commit_facts, commit_header};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use gitraptor_policy::guard::git_command::GitCommand;
use gitraptor_policy::guard::permissions::Chain;
use gitraptor_policy::guard::plan::{PlanShape, normalize};
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};

const REPO_ID: &str = "0000-ffff";
const MESSAGE: &str = "feat: a change\n";

struct Rig {
    _profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    registry: Arc<GuardRegistry>,
    engine: GuardrailsEngine,
    /// `main`.
    base: String,
    /// The tip of `feat-x`, one commit ahead of `main`.
    tip: String,
    /// A commit no branch holds: the fresh commit a `git merge` integrates.
    fresh: String,
    _entries: Arc<Mutex<Vec<LogEntry>>>,
}

/// A repo whose `main` carries `team` as the floor, an empty profile and the engine attached.
fn rig(team: &str) -> Rig {
    let repo = tempfile::tempdir().unwrap();
    let p = repo.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(p.join(".gitraptor")).unwrap();
    std::fs::write(p.join(".gitraptor/settings.json"), team).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "a"]);
    let blob = git(p, &["rev-parse", "main:.gitraptor/settings.json"]);
    let base = git(p, &["rev-parse", "HEAD"]).trim().to_owned();
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]).trim().to_owned();
    let fresh = git(p, &["commit-tree", &tree, "-p", &base, "-m", "fresh"])
        .trim()
        .to_owned();
    git(p, &["switch", "-q", "-c", "feat-x"]);
    std::fs::write(p.join("f.txt"), "x").unwrap();
    git(p, &["add", "f.txt"]);
    git(p, &["commit", "-q", "-m", "b"]);
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
        base,
        tip,
        fresh,
        _entries: entries,
    }
}

fn deny(op: &str) -> String {
    format!(r#"{{"permissions":{{"deny":["{op}"]}}}}"#)
}

fn agent() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn request(r: &Rig, plan_id: &str, operation: GovernedAs, shape: PlanShape) -> GateRequest {
    GateRequest {
        repo_id: REPO_ID.into(),
        operation,
        layer: Layer::Mcp,
        actor: agent(),
        fingerprint: "f".repeat(64),
        plan_id: plan_id.into(),
        common_dir: r.common.clone(),
        worktree: r.repo.path().to_owned(),
        branch: Some("feat-x".into()),
        shape: Some(shape),
    }
}

fn full(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

fn merge_shape(r: &Rig) -> PlanShape {
    PlanShape::Merge {
        base: full("main"),
        old: r.base.clone(),
        integrated: r.fresh.clone(),
    }
}

fn rebase_shape(r: &Rig) -> PlanShape {
    PlanShape::Rebase {
        branch: full("feat-x"),
        upstream: full("main"),
        old_tip: r.tip.clone(),
        onto: r.base.clone(),
    }
}

fn create_shape(r: &Rig) -> PlanShape {
    PlanShape::CreateWorktree {
        branch: full("new-b"),
        at: r.base.clone(),
    }
}

fn delete_shape(r: &Rig) -> PlanShape {
    PlanShape::DeleteWorktree {
        branch: full("feat-x"),
        tip: r.tip.clone(),
    }
}

fn commit_shape(r: &Rig) -> PlanShape {
    PlanShape::Commit {
        branch: full("feat-x"),
        old: r.tip.clone(),
        paths: vec!["src/a.rs".into()],
        authorship: commit_facts(&r.common, MESSAGE),
        header: commit_header(&r.common, MESSAGE),
    }
}

/// `(rule, level, cause)` of every reason, sorted: the reasons that decide, not their text.
fn reasons(d: &Decision) -> Vec<String> {
    let mut out: Vec<String> = d
        .reasons
        .iter()
        .map(|x| format!("{:?}/{:?}/{:?}", x.rule, x.level, x.cause))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn shape_of(d: &Decision) -> (Effect, Effect, Vec<String>) {
    (d.effect, d.applied_effect, reasons(d))
}

fn rank(e: Effect) -> u8 {
    match e {
        Effect::Allow => 0,
        Effect::Ask => 1,
        Effect::Deny => 2,
    }
}

fn hook_of(op: &Operation) -> Hook {
    match op {
        Operation::Push { .. } => Hook::PrePush,
        Operation::RefTransaction { .. } => Hook::ReferenceTransaction,
        Operation::Rebase { .. } => Hook::PreRebase,
        Operation::Commit {
            stage: CommitStage::PreCommit,
        } => Hook::PreCommit,
        Operation::Commit { .. } => Hook::CommitMsg,
    }
}

/// What the hooks of the `git` command `chain` decide for `ops`, combined as the maximum with
/// the reasons of every operation (the entry point of `guard.evaluate`, `LogLayer::Hooks`).
fn hooks(r: &Rig, ops: &[Operation], chain: Vec<GitCommand>) -> (Effect, Effect, Vec<String>) {
    let caller = Caller {
        actor: Some(AgentKind::ClaudeCode),
        cwd: Some(r.repo.path().to_owned()),
        authorship: true,
        policies: true,
        permissions: true,
        chain: Some(Chain::new(chain)),
        ..Caller::default()
    };
    let mut effect = Effect::Allow;
    let mut applied = Effect::Allow;
    let mut all = Vec::new();
    for op in ops {
        let params = EvaluateParams {
            repo_id: REPO_ID.into(),
            common_dir: r.common.to_string_lossy().into_owned(),
            hook: hook_of(op),
            operation: op.clone(),
            authorship: matches!(op, Operation::Commit { .. })
                .then(|| commit_facts(&r.common, MESSAGE)),
            exception_token: None,
        };
        let d = serve_audited(&r.registry, &params, &caller).decision;
        if rank(d.effect) > rank(effect) {
            effect = d.effect;
        }
        if rank(d.applied_effect) > rank(applied) {
            applied = d.applied_effect;
        }
        all.extend(reasons(&d));
    }
    all.sort();
    all.dedup();
    (effect, applied, all)
}

fn mcp_plan(r: &Rig, id: &str, operation: GovernedAs, shape: PlanShape) -> Decision {
    let verdict = r
        .engine
        .evaluate_detailed(&request(r, id, operation, shape));
    verdict.decision.expect("the engine decided the plan")
}

struct Case {
    key: &'static str,
    rule: Rule,
    operation: GovernedAs,
    shape: fn(&Rig) -> PlanShape,
    chain: fn() -> Vec<GitCommand>,
}

const CASES: &[Case] = &[
    Case {
        key: "merge",
        rule: Rule::PermissionMerge,
        operation: GovernedAs::Merge,
        shape: merge_shape,
        chain: || vec![GitCommand::Certain("merge", None)],
    },
    Case {
        key: "rebase",
        rule: Rule::PermissionRebase,
        operation: GovernedAs::Rebase,
        shape: rebase_shape,
        chain: || vec![GitCommand::Certain("rebase", None)],
    },
    Case {
        key: "worktree-add",
        rule: Rule::PermissionWorktreeAdd,
        operation: GovernedAs::CreateWorktree,
        shape: create_shape,
        chain: || vec![GitCommand::Certain("worktree", Some("add"))],
    },
    Case {
        key: "commit",
        rule: Rule::PermissionCommit,
        operation: GovernedAs::Commit,
        shape: commit_shape,
        chain: || vec![GitCommand::Certain("commit", None)],
    },
];

/// GRD016-PARITY-PLAN (scenario 2, D6): the plan of each shape with its operation in `deny` gets
/// the decision of the hooks of the `git` command that would run it.
#[test]
fn every_plan_operation_gets_the_hooks_decision() {
    for case in CASES {
        let r = rig(&deny(case.key));
        let shape = (case.shape)(&r);
        let ops = normalize(&shape).evaluated;
        let from_hooks = hooks(&r, &ops, (case.chain)());
        assert_eq!(
            from_hooks.0,
            Effect::Deny,
            "fixture: the hooks must deny `{}` ({:?})",
            case.key,
            from_hooks
        );
        assert!(
            from_hooks
                .2
                .iter()
                .any(|x| x.starts_with(&format!("{:?}/", case.rule))),
            "fixture: the hooks must name {:?} for `{}`: {:?}",
            case.rule,
            case.key,
            from_hooks
        );
        let plan = mcp_plan(&r, &format!("p-{}", case.key), case.operation, shape);
        assert_eq!(
            shape_of(&plan),
            from_hooks,
            "the {} plan with `{}` in deny must get the decision of the hooks \
             (the plan must apply the permission of the operations it knows it runs)",
            case.key,
            case.key
        );
    }
}

/// GRD016-DELWT-BRANCH (guard 1.1): the plan that deletes a worktree deletes its branch too.
#[test]
fn deleting_a_worktree_applies_the_branch_delete_permission() {
    // (a) `branch-delete` in deny: denied with permission.branch-delete at the floor, with the
    // reasons the hooks give to the deletion of that branch.
    let r = rig(&deny("branch-delete"));
    let shape = delete_shape(&r);
    let ops = normalize(&shape).evaluated;
    let from_hooks = hooks(
        &r,
        &ops,
        vec![GitCommand::Certain("worktree", Some("remove"))],
    );
    let plan = mcp_plan(&r, "p-del-branch", GovernedAs::DeleteWorktree, shape);
    assert_eq!(
        plan.applied_effect,
        Effect::Deny,
        "deleting a worktree deletes its branch: with branch-delete in deny the plan must be \
         denied: {plan:?}"
    );
    assert!(
        plan.reasons
            .iter()
            .any(|x| x.rule == Rule::PermissionBranchDelete && x.level == Level::Floor),
        "the denial must name permission.branch-delete at the floor: {:?}",
        plan.reasons
    );
    assert_eq!(
        shape_of(&plan),
        from_hooks,
        "same reasons as the hooks give to the deletion of that branch"
    );
    // (b) `worktree-remove` in deny: the hooks never see it, the MCP layer is stricter here.
    let r = rig(&deny("worktree-remove"));
    let plan = mcp_plan(&r, "p-del-wt", GovernedAs::DeleteWorktree, delete_shape(&r));
    assert_eq!(
        plan.applied_effect,
        Effect::Deny,
        "with worktree-remove in deny the plan must be denied: {plan:?}"
    );
    assert!(
        plan.reasons
            .iter()
            .any(|x| x.rule == Rule::PermissionWorktreeRemove && x.level == Level::Floor),
        "the denial must name permission.worktree-remove at the floor: {:?}",
        plan.reasons
    );
}

/// GRD016-PLAN-ASK: `ask` reaches an agent's plan as `Ask`, and a denied commit names the
/// permission once per level.
#[test]
fn an_agent_plan_in_ask_is_asked() {
    let r = rig(r#"{"permissions":{"ask":["rebase"]}}"#);
    let req = request(&r, "p-ask", GovernedAs::Rebase, rebase_shape(&r));
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(
        verdict.view,
        GateDecision::Ask,
        "a rebase plan with `rebase` in ask must be asked, not allowed: {:?}",
        verdict.decision
    );
    let d = verdict
        .decision
        .expect("an asked plan carries its decision");
    assert_eq!(
        (d.effect, d.applied_effect),
        (Effect::Ask, Effect::Deny),
        "{d:?}"
    );
    assert!(
        d.reasons.iter().any(|x| x.rule == Rule::PermissionRebase),
        "the ask must name permission.rebase: {:?}",
        d.reasons
    );
    let r = rig(&deny("commit"));
    let verdict =
        r.engine
            .evaluate_detailed(&request(&r, "p-c", GovernedAs::Commit, commit_shape(&r)));
    assert_eq!(verdict.view, GateDecision::Deny, "{:?}", verdict.decision);
    let d = verdict.decision.unwrap();
    assert_eq!(
        d.reasons
            .iter()
            .filter(|x| x.rule == Rule::PermissionCommit)
            .count(),
        1,
        "permission.commit once per level: {:?}",
        d.reasons
    );
}

// ---- Regression guards (S4): green today and after ----

fn layer_input<'a>(r: &'a Rig, layer: LogLayer) -> DecideInput<'a> {
    DecideInput {
        common_dir: &r.common,
        repo_id: REPO_ID,
        actor: Some(AgentKind::ClaudeCode),
        worktree: Some(r.repo.path()),
        layer,
        rules: RuleSet {
            authorship: true,
            policies: true,
            commit_policies: false,
            second_line_skip: false,
            permissions: true,
        },
        authorship: None,
        unregistered_bases: None,
    }
}

fn same_by_both_layers(r: &Rig, op: &Operation) -> Decision {
    let mcp = decide(&r.registry, &layer_input(r, LogLayer::Mcp), op).decision;
    let hooks = decide(&r.registry, &layer_input(r, LogLayer::Hooks), op).decision;
    assert_eq!(shape_of(&mcp), shape_of(&hooks), "{op:?}");
    mcp
}

fn push(local: &str, remote: &str) -> Operation {
    Operation::Push {
        remote: Untrusted::new("origin"),
        updates: vec![PushUpdate {
            local_ref: Some(full("feat-x")),
            local: RefValue::Oid(local.to_owned()),
            remote_ref: full("feat-x"),
            remote: RefValue::Oid(remote.to_owned()),
        }],
    }
}

fn tx(branch: &str, old: RefValue, new: RefValue) -> Operation {
    Operation::RefTransaction {
        updates: vec![RefUpdate {
            refname: full(branch),
            old,
            new,
        }],
        orphan_head: None,
    }
}

#[test]
fn direct_operations_get_the_same_decision_by_both_layers() {
    let r = rig(&deny("push"));
    same_by_both_layers(&r, &push(&r.tip, &r.base));
    let r = rig(&deny("force-push"));
    same_by_both_layers(&r, &push(&r.base, &r.tip));
    let r = rig(&deny("reset-hard"));
    same_by_both_layers(
        &r,
        &tx(
            "feat-x",
            RefValue::Oid(r.tip.clone()),
            RefValue::Oid(r.base.clone()),
        ),
    );
    let r = rig(&deny("branch-delete"));
    let d = same_by_both_layers(
        &r,
        &tx("feat-x", RefValue::Oid(r.tip.clone()), RefValue::Zero),
    );
    assert_eq!(
        d.applied_effect,
        Effect::Deny,
        "branch-delete in deny: {d:?}"
    );
}

#[test]
fn the_safe_minimum_without_configuration_holds_by_both_layers() {
    let r = rig("{}");
    let forced = same_by_both_layers(&r, &push(&r.base, &r.tip));
    assert!(
        forced
            .reasons
            .iter()
            .any(|x| x.rule == Rule::MinimumForcePush),
        "force-push is denied by the minimum: {:?}",
        forced.reasons
    );
    let delete_base = Operation::Push {
        remote: Untrusted::new("origin"),
        updates: vec![PushUpdate {
            local_ref: None,
            local: RefValue::Zero,
            remote_ref: full("main"),
            remote: RefValue::Oid(r.base.clone()),
        }],
    };
    let deleted = same_by_both_layers(&r, &delete_base);
    assert!(
        deleted
            .reasons
            .iter()
            .any(|x| x.rule == Rule::MinimumBaseBranchDelete),
        "deleting the base is denied by the minimum: {:?}",
        deleted.reasons
    );
}

/// Guard 3.1: `merge` allowed with `commit` in deny; the integrated commits are not the agent's.
#[test]
fn a_merge_plan_is_allowed_with_merge_allow_and_commit_deny() {
    let r = rig(r#"{"permissions":{"allow":["merge"],"deny":["commit"]}}"#);
    let d = mcp_plan(&r, "p-m", GovernedAs::Merge, merge_shape(&r));
    assert_eq!(
        (d.effect, d.applied_effect),
        (Effect::Allow, Effect::Allow),
        "{d:?}"
    );
}

/// Guard 4.1: `commit` in deny at the floor and in the worktree: one reason per level.
#[test]
fn commit_deny_at_floor_and_worktree_gives_one_reason_per_level() {
    let r = rig(&deny("commit"));
    std::fs::create_dir_all(r.repo.path().join(".gitraptor")).unwrap();
    std::fs::write(
        r.repo.path().join(".gitraptor/settings.json"),
        deny("commit"),
    )
    .unwrap();
    let d = mcp_plan(&r, "p-c2", GovernedAs::Commit, commit_shape(&r));
    let levels: Vec<Level> = d
        .reasons
        .iter()
        .filter(|x| x.rule == Rule::PermissionCommit)
        .map(|x| x.level)
        .collect();
    assert!(!levels.is_empty(), "{d:?}");
    let mut unique = levels.clone();
    unique.sort_by_key(|l| format!("{l:?}"));
    unique.dedup();
    assert_eq!(
        levels.len(),
        unique.len(),
        "one reason per level: {levels:?}"
    );
    assert!(levels.contains(&Level::Floor), "{levels:?}");
}
