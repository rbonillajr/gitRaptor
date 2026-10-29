//! T005 (D2, D3, D8, D12, D16, S-GRD-9, SEC-05): the plan engine behind `GuardrailsGate`.
//! Child module of `guardrails::plan_gate`, attached there with
//! `#[cfg(test)] #[path = "plan_gate_tests.rs"] mod tests;`.
//!
//! No `Command` here (`guard_boundary.rs`): the repo is a minimal Git directory written by hand,
//! unknown to the registry, so the engine protects the union {main, main branch, base} for
//! everyone (D12) and nothing else.

use super::*;
use crate::executor::{GateDecision, GateRequest, GuardrailsGate, PlanClose};
use crate::guardrails::log::LogEntry;
use crate::guardrails::registry::GuardRegistry;
use crate::profile::ProfileDirs;
use gitraptor_api::Actor;
use gitraptor_api::catalog::{GovernedAs, Layer};
use gitraptor_api::guard::{
    AuthorshipFacts, Decision, Effect, ExceptionState, Level, LogKind, LogLayer, Reason, Rule,
};
use gitraptor_policy::guard::plan::PlanShape;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAIN: &str = "refs/heads/main";
const FREE: &str = "refs/heads/free";
const SECRET: &str = "SECRET-COMMIT-MESSAGE-TEXT";

fn oid(c: char) -> String {
    c.to_string().repeat(40)
}

/// A readable repo, written by hand: `main`, an empty object store.
fn repo(parent: &Path) -> PathBuf {
    let git = parent.join("demo/.git");
    for dir in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(git.join(dir)).unwrap();
    }
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        git.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
    )
    .unwrap();
    // The drive form on Windows, as the daemon's repos have it: the reader refuses `\\?\` paths.
    gitraptor_git::paths::canonicalize(&git).unwrap()
}

struct Rig {
    _tmp: tempfile::TempDir,
    common: PathBuf,
    engine: GuardrailsEngine,
    entries: Arc<Mutex<Vec<LogEntry>>>,
}

fn rig(attached: bool) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let common = repo(tmp.path());
    let engine = GuardrailsEngine::default();
    let entries = Arc::new(Mutex::new(Vec::new()));
    if attached {
        let registry = Arc::new(GuardRegistry::for_profile(ProfileDirs::under_root(
            tmp.path().join("profile"),
        )));
        let kept = Arc::clone(&entries);
        engine.attach(
            &registry,
            Arc::new(move |entry| {
                kept.lock().unwrap().push(entry);
                true
            }),
        );
    }
    Rig {
        _tmp: tmp,
        common,
        engine,
        entries,
    }
}

impl Rig {
    fn req(&self, plan_id: &str, operation: GovernedAs, shape: Option<PlanShape>) -> GateRequest {
        GateRequest {
            repo_id: "repo-1".into(),
            operation,
            layer: Layer::Cockpit,
            actor: Actor::Unattributed,
            fingerprint: "f".repeat(64),
            plan_id: plan_id.into(),
            common_dir: self.common.clone(),
            worktree: self.common.parent().unwrap().to_owned(),
            branch: Some("main".into()),
            shape,
        }
    }

    fn merge(&self, plan_id: &str, base: &str) -> GateRequest {
        self.req(
            plan_id,
            GovernedAs::Merge,
            Some(PlanShape::Merge {
                base: base.into(),
                old: oid('a'),
                integrated: oid('b'),
            }),
        )
    }

    fn commit(&self, plan_id: &str, branch: &str, message: &str) -> GateRequest {
        self.req(
            plan_id,
            GovernedAs::Commit,
            Some(PlanShape::Commit {
                branch: branch.into(),
                old: oid('a'),
                paths: vec!["src/a.rs".into()],
                authorship: commit_facts(&self.common, message),
            }),
        )
    }
}

fn decision(effect: Effect) -> Decision {
    Decision {
        decision_id: "d".into(),
        effect,
        applied_effect: effect,
        reasons: vec![Reason {
            rule: Rule::ProtectedBranch,
            level: Level::Minimum,
            cause: None,
            params: vec![],
        }],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    }
}

/// S-GRD-9: a decision of `ask` is applied as `deny` (no confirmation queue yet, US-GRD-015).
#[test]
fn ask_is_applied_as_deny() {
    assert_eq!(view_of(&decision(Effect::Ask)), GateDecision::Deny);
    assert_eq!(view_of(&decision(Effect::Deny)), GateDecision::Deny);
    assert_eq!(view_of(&decision(Effect::Allow)), GateDecision::Allow);
}

/// D16: an engine nobody attached, or a plan without its shape, denies with a reason.
#[test]
fn an_unattached_engine_denies_with_a_reason() {
    let r = rig(false);
    let verdict = r.engine.evaluate_detailed(&r.merge("p1", FREE));
    assert_eq!(verdict.view, GateDecision::Deny);
    assert!(verdict.binding.is_none());
    let d = verdict.decision.expect("a denial carries its decision");
    assert_eq!(d.reasons[0].rule, Rule::InternalError, "{d:?}");

    let r = rig(true);
    let verdict = r
        .engine
        .evaluate_detailed(&r.req("p2", GovernedAs::Merge, None));
    assert_eq!(verdict.view, GateDecision::Deny);
    assert_eq!(
        verdict.decision.unwrap().reasons[0].rule,
        Rule::InternalError
    );
    assert!(
        !GuardrailsEngine::default()
            .evaluate(&r.merge("p3", FREE))
            .eq(&GateDecision::Allow)
    );
    assert!(r.engine.decides());
}

/// D8: with 1 024 plans alive nothing is expelled and the next preview denies with
/// `system.internal-error`; a preview that did deny and is expelled leaves its denial.
#[test]
fn a_running_plan_is_never_evicted() {
    // Every state has a live binding: the 1 025th plan cannot be held.
    let r = rig(true);
    let alive: Vec<_> = (0..1024)
        .map(|i| {
            let req = r.req(
                &format!("run-{i}"),
                GovernedAs::CreateWorktree,
                Some(PlanShape::CreateWorktree {
                    branch: format!("refs/heads/feat-{i}"),
                    at: oid('a'),
                }),
            );
            let v = r.engine.evaluate_detailed(&req);
            assert_eq!(v.view, GateDecision::Allow, "plan {i}: {:?}", v.decision);
            assert!(v.binding.is_some(), "plan {i}");
            v
        })
        .collect();
    let overflow = r.engine.evaluate_detailed(&r.merge("one-too-many", FREE));
    assert_eq!(overflow.view, GateDecision::Deny);
    assert_eq!(
        overflow.decision.unwrap().reasons[0].rule,
        Rule::InternalError
    );
    drop(alive);

    // A preview that denied is expelled by the plans after it, and leaves its `denial`.
    let r = rig(true);
    let first = r.engine.evaluate_detailed(&r.merge("denied-preview", MAIN));
    assert_eq!(first.view, GateDecision::Deny);
    let first_id = first.decision.unwrap().decision_id;
    for i in 0..1024 {
        let v = r
            .engine
            .evaluate_detailed(&r.merge(&format!("later-{i}"), FREE));
        assert_eq!(v.view, GateDecision::Allow, "plan {i}");
    }
    let entries = r.entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].kind, LogKind::Denial);
    assert_eq!(entries[0].decision_id, first_id);
}

/// D8 and the diagram "dónde acaba el dato": the answer and the entry share the `decision_id`,
/// with the layer of the plan, once per plan.
#[test]
fn the_response_and_the_entry_share_the_decision_id() {
    let r = rig(true);
    let req = r.merge("p1", MAIN);
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(verdict.view, GateDecision::Deny);
    let d = verdict.decision.expect("the denial");
    assert_eq!(d.reasons[0].rule, Rule::ProtectedBranch, "{d:?}");
    r.engine.record_close(&req, PlanClose::Denied);
    r.engine.record_close(&req, PlanClose::Denied);
    let entries = r.entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "one entry per plan: {entries:#?}");
    assert_eq!(entries[0].decision_id, d.decision_id);
    assert_eq!(entries[0].kind, LogKind::Denial);
    assert_eq!(entries[0].layer, LogLayer::Cockpit);
}

/// D2: the commit is evaluated on its branch with the sentinel `old -> old`: a protected branch
/// denies, a free one allows and gets its binding.
#[test]
fn a_commit_evaluates_its_branch_with_the_old_to_old_sentinel() {
    let r = rig(true);
    let on_main = r.engine.evaluate_detailed(&r.commit("c1", MAIN, "feat: x"));
    assert_eq!(on_main.view, GateDecision::Deny);
    assert_eq!(
        on_main.decision.unwrap().reasons[0].rule,
        Rule::ProtectedBranch
    );
    let on_free = r.engine.evaluate_detailed(&r.commit("c2", FREE, "feat: x"));
    assert_eq!(on_free.view, GateDecision::Allow, "{:?}", on_free.decision);
    assert!(on_free.binding.is_some());
}

/// SEC-05: the text of the commit message is only ever `AuthorshipFacts`: it is not in the
/// shape (so not in the fingerprint) nor in the entry the plan leaves.
#[test]
fn the_commit_message_never_reaches_the_log_or_the_fingerprint() {
    let r = rig(true);
    let message =
        format!("feat: {SECRET}\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n");
    let facts: AuthorshipFacts = commit_facts(&r.common, &message);
    assert!(!format!("{facts:?}").contains(SECRET));
    assert_eq!(facts.coauthors.len(), 1, "the trailer is read: {facts:?}");

    let req = r.commit("c1", MAIN, &message);
    // What enters the fingerprint is the shape.
    assert!(!format!("{:?}", req.shape).contains(SECRET));
    let verdict = r.engine.evaluate_detailed(&req);
    assert_eq!(verdict.view, GateDecision::Deny);
    assert!(!format!("{:?}", verdict.decision).contains(SECRET));
    r.engine.record_close(&req, PlanClose::Denied);
    let entries = r.entries.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!format!("{entries:?}").contains(SECRET));
}

/// `decide_all` with the plan's known operations (BR-CALC-001): their permission joins the
/// maximum with its reasons, enabling them never turns a denial into `allow`, and a commit is
/// decided by `known_ops` alone, its reason named once.
#[test]
fn decide_all_applies_the_permissions_of_the_known_operations() {
    use gitraptor_policy::guard::plan::normalize;
    use gitraptor_policy::settings::model::Operation as Op;
    let tmp = tempfile::tempdir().unwrap();
    let common = repo(tmp.path());
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    std::fs::create_dir_all(&dirs.config).unwrap();
    std::fs::write(
        dirs.config.join("settings.json"),
        r#"{"permissions":{"deny":["merge","commit"],"ask":["rebase"]}}"#,
    )
    .unwrap();
    let registry = GuardRegistry::for_profile(dirs);
    let worktree = common.parent().unwrap().to_owned();
    let input = DecideInput {
        common_dir: &common,
        repo_id: "repo-1",
        // The person: an agent's movement is denied here anyway, since this hand-written repo
        // has no readable team level (SEC-GRD-17).
        actor: None,
        worktree: Some(&worktree),
        layer: LogLayer::Cockpit,
        rules: RuleSet::all(),
        authorship: None,
        unregistered_bases: Some(&[]),
    };
    let decide = |shape: &PlanShape, known: &[Op]| {
        let n = normalize(shape);
        decide_all(&registry, &input, &n.evaluated, &n.preset, known).decision
    };
    let rules = |d: &Decision| d.reasons.iter().map(|r| r.rule).collect::<Vec<_>>();
    let free = PlanShape::Merge {
        base: FREE.into(),
        old: oid('a'),
        integrated: oid('b'),
    };
    // Without known operations the plan is decided as before: a merge into a free branch goes.
    assert_eq!(decide(&free, &[]).effect, Effect::Allow);
    // The maximum keeps only its reasons: `deny` of merge wins over `ask` of rebase.
    let both = decide(&free, &[Op::Merge, Op::Rebase]);
    assert_eq!(
        (both.effect, both.applied_effect),
        (Effect::Deny, Effect::Deny)
    );
    assert_eq!(rules(&both), [Rule::PermissionMerge], "{both:?}");
    assert_eq!(both.reasons[0].level, Level::Profile);
    // An `ask` alone follows the actor: the person goes ahead with a notice (Q-GRD-46).
    let asked = decide(&free, &[Op::Rebase]);
    assert_eq!(
        (asked.effect, asked.applied_effect),
        (Effect::Ask, Effect::Allow)
    );
    // A permission in `allow` (none set for worktree-add) never relaxes a denial: the merge into
    // the protected `main` stays denied with its own reasons.
    let main = PlanShape::Merge {
        base: MAIN.into(),
        old: oid('a'),
        integrated: oid('b'),
    };
    let before = decide(&main, &[]);
    let after = decide(&main, &[Op::WorktreeAdd]);
    assert_eq!(before.effect, Effect::Deny);
    assert_eq!(
        (after.effect, after.applied_effect, rules(&after)),
        (before.effect, before.applied_effect, rules(&before))
    );
    // A commit applies its permission through `known_ops`, once.
    let commit = PlanShape::Commit {
        branch: FREE.into(),
        old: oid('a'),
        paths: vec!["src/a.rs".into()],
        authorship: commit_facts(&common, "message"),
    };
    let d = decide(&commit, &[Op::Commit]);
    assert_eq!(d.effect, Effect::Deny, "{d:?}");
    assert_eq!(
        rules(&d)
            .iter()
            .filter(|r| **r == Rule::PermissionCommit)
            .count(),
        1,
        "{d:?}"
    );
}
