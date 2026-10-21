//! US-GRD-007 on the pure side (DS-US-GRD-007 § 3.1, § 6.1, § 6.3): the classifier table with its
//! precedence and exclusions, the safe reading of a `git` command line, and the permissions in
//! `guard::evaluate`. Public API only; no I/O.

use std::ffi::OsString;

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, CommitStage, Effect, Level, LogLayer, Operation as GuardOp, PushUpdate, RefUpdate,
    RefValue, Rule,
};
use gitraptor_policy::guard::authorship;
use gitraptor_policy::guard::git_command::{GitCommand, parse};
use gitraptor_policy::guard::permissions::{Chain, Classified, Fresh, LineKind, classify};
use gitraptor_policy::guard::policies::Policies;
use gitraptor_policy::guard::{Context, Evaluation, Facts, FastForward, evaluate};
use gitraptor_policy::settings::SourceKind;
use gitraptor_policy::settings::model::{Operation as Op, Settings};
use gitraptor_policy::team::{EffectivePermissions, Permission, combine};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";

fn settings(json: &str) -> Settings {
    serde_json::from_str(json).expect("settings json")
}

/// The team floor's permissions with the minimum as the floor leaves it.
fn perms(json: &str) -> EffectivePermissions {
    combine(Some((SourceKind::Floor, &settings(json))), true, &[])
}

/// Every operation but the ones named is `allow`: only `deny` of `ops` is set.
fn denying(ops: &[&str]) -> EffectivePermissions {
    let list = ops
        .iter()
        .map(|o| format!("\"{o}\""))
        .collect::<Vec<_>>()
        .join(",");
    perms(&format!(r#"{{"permissions":{{"deny":[{list}]}}}}"#))
}

// `ctx()` runs with the minimum off, which D4 derives from a readable floor that disables it.
fn all_allow() -> EffectivePermissions {
    perms(r#"{"permissions":{"disableSafeMinimum":true}}"#)
}

// ---------------------------------------------------------------------------------------------
// git_command
// ---------------------------------------------------------------------------------------------

fn certain(argv: &[&str]) -> Option<(String, Option<String>)> {
    match parse(Some(argv)) {
        GitCommand::Certain(sub, action) => Some((sub.to_string(), action.map(|a| a.to_string()))),
        GitCommand::Unknown => None,
    }
}

#[test]
fn git_command_reads_the_subcommand_and_the_action() {
    assert_eq!(
        certain(&["git", "worktree", "add", "-b", "x", "../p"]),
        Some(("worktree".into(), Some("add".into())))
    );
    assert_eq!(
        certain(&["/usr/bin/git", "worktree", "remove", "../p"]),
        Some(("worktree".into(), Some("remove".into())))
    );
    for sub in [
        "commit", "merge", "pull", "rebase", "fetch", "clone", "branch", "reset",
    ] {
        let found = certain(&["git", sub]);
        assert_eq!(found.map(|f| f.0), Some(sub.to_owned()), "{sub}");
    }
}

#[test]
fn git_command_skips_the_global_options_it_knows() {
    for argv in [
        vec!["git", "-C", "/some/dir", "rebase"],
        vec!["git", "-c", "user.name=x", "--no-pager", "rebase"],
        vec!["git", "--git-dir=/x/.git", "--work-tree=/x", "rebase"],
        vec!["git", "--no-optional-locks", "rebase", "--no-verify"],
    ] {
        assert_eq!(
            certain(&argv).map(|f| f.0),
            Some("rebase".to_owned()),
            "{argv:?}"
        );
    }
}

#[test]
fn git_command_is_unknown_when_it_cannot_be_proved() {
    // An option nobody listed, an alias, a program that is not git, nothing to read.
    assert_eq!(certain(&["git", "--surprising-option", "commit"]), None);
    assert_eq!(certain(&["git", "ci", "-m", "x"]), None);
    assert_eq!(certain(&["/bin/sh", "commit"]), None);
    assert_eq!(certain(&["git"]), None);
    assert_eq!(certain(&[]), None);
    assert!(matches!(parse(None::<&[&str]>), GitCommand::Unknown));
    // `worktree` without an action it knows is not `worktree add`.
    assert_eq!(certain(&["git", "worktree"]), None);
}

#[test]
fn git_command_is_unknown_for_a_non_utf8_argument_before_the_subcommand() {
    use std::os::unix::ffi::OsStringExt;
    let argv: Vec<OsString> = vec![
        "git".into(),
        "-C".into(),
        OsString::from_vec(vec![0xff, 0xfe]),
        "commit".into(),
    ];
    assert!(matches!(parse(Some(&argv[..])), GitCommand::Certain(_, _)));
    let argv: Vec<OsString> = vec![
        "git".into(),
        OsString::from_vec(vec![0xff, 0xfe]),
        "commit".into(),
    ];
    assert!(matches!(parse(Some(&argv[..])), GitCommand::Unknown));
}

// ---------------------------------------------------------------------------------------------
// classify
// ---------------------------------------------------------------------------------------------

fn chain(cmds: &[(&str, Option<&str>)]) -> Chain {
    Chain::new(
        cmds.iter()
            .map(|(sub, action)| match certain_of(sub, *action) {
                Some(c) => c,
                None => GitCommand::Unknown,
            })
            .collect(),
    )
}

/// A `Certain` command built through the public parser, so the test names no variant shape.
fn certain_of(sub: &str, action: Option<&str>) -> Option<GitCommand> {
    let mut argv = vec!["git", sub];
    argv.extend(action);
    match parse(Some(&argv[..])) {
        c @ GitCommand::Certain(_, _) => Some(c),
        GitCommand::Unknown => None,
    }
}

fn unknown_chain() -> Chain {
    Chain::new(vec![GitCommand::Unknown])
}

fn fresh(commits: usize, merges: usize) -> Fresh {
    Fresh {
        commits,
        merges,
        unverifiable: false,
    }
}

fn ops(c: &Classified) -> Vec<Op> {
    let mut v = c.operations.clone();
    v.sort();
    v
}

#[test]
fn classify_a_branch_deletion_is_branch_delete_whatever_the_chain() {
    let needs = all_allow();
    for ch in [
        chain(&[("branch", None)]),
        chain(&[("update-ref", None)]),
        unknown_chain(),
        chain(&[("rebase", None)]),
    ] {
        let c = classify(LineKind::Delete, &ch, None, &needs);
        assert_eq!(ops(&c), [Op::BranchDelete]);
        assert!(!c.unverifiable);
    }
}

#[test]
fn classify_a_creation_under_worktree_add_is_worktree_add() {
    let needs = all_allow();
    let ch = chain(&[("branch", None), ("worktree", Some("add"))]);
    let c = classify(LineKind::Create, &ch, None, &needs);
    assert_eq!(ops(&c), [Op::WorktreeAdd]);
    // The commits it brings count too.
    let c = classify(LineKind::Create, &ch, Some(&fresh(1, 0)), &needs);
    assert_eq!(ops(&c), [Op::Commit, Op::WorktreeAdd]);
}

#[test]
fn classify_a_rebase_owns_its_commits() {
    let needs = all_allow();
    let ch = chain(&[("rebase", None)]);
    for line in [LineKind::Create, LineKind::Update] {
        let c = classify(line, &ch, Some(&fresh(3, 0)), &needs);
        assert_eq!(ops(&c), [Op::Rebase]);
    }
    // `pull --rebase` runs a `git rebase` child under `git pull`.
    let ch = chain(&[("rebase", None), ("pull", None)]);
    let c = classify(LineKind::Update, &ch, Some(&fresh(2, 0)), &needs);
    assert_eq!(ops(&c), [Op::Rebase]);
}

#[test]
fn classify_a_merge_or_a_pull_is_merge_and_commit_only_beyond_the_merge_commit() {
    let needs = all_allow();
    for sub in ["merge", "pull"] {
        let ch = chain(&[(sub, None)]);
        // Only the commit that closes the merge.
        let c = classify(LineKind::Update, &ch, Some(&fresh(1, 1)), &needs);
        assert_eq!(ops(&c), [Op::Merge], "{sub}");
        // A fast-forward that brings a fresh commit: no merge commit, still a merge command.
        let c = classify(LineKind::Update, &ch, Some(&fresh(2, 1)), &needs);
        assert_eq!(ops(&c), [Op::Commit, Op::Merge], "{sub}");
    }
}

#[test]
fn classify_what_fetch_and_clone_bring_is_nothing() {
    let needs = denying(&["commit", "merge"]);
    for sub in ["fetch", "clone"] {
        let ch = chain(&[(sub, None)]);
        let c = classify(LineKind::Update, &ch, Some(&fresh(4, 1)), &needs);
        assert!(ops(&c).is_empty(), "{sub}");
        assert!(!c.unverifiable, "{sub}");
    }
}

#[test]
fn classify_any_other_command_with_fresh_commits_is_a_commit() {
    let needs = all_allow();
    for ch in [
        chain(&[("commit", None)]),
        chain(&[("cherry-pick", None)]),
        chain(&[("revert", None)]),
        chain(&[("am", None)]),
        chain(&[("branch", None)]),
        chain(&[("reset", None)]),
        chain(&[("switch", None)]),
        chain(&[("checkout", None)]),
        chain(&[("update-ref", None)]),
        unknown_chain(),
    ] {
        let c = classify(LineKind::Update, &ch, Some(&fresh(1, 0)), &needs);
        assert_eq!(ops(&c), [Op::Commit]);
        // A merge commit among them is a merge too.
        let c = classify(LineKind::Update, &ch, Some(&fresh(2, 1)), &needs);
        assert_eq!(ops(&c), [Op::Commit, Op::Merge]);
        // A fast-forward to commits that exist brings nothing.
        let c = classify(LineKind::Update, &ch, Some(&fresh(0, 0)), &needs);
        assert!(ops(&c).is_empty());
    }
}

#[test]
fn classify_precedence_is_worktree_add_then_rebase_then_merge_then_fetch() {
    let needs = all_allow();
    let c = classify(
        LineKind::Create,
        &chain(&[("merge", None), ("rebase", None), ("worktree", Some("add"))]),
        None,
        &needs,
    );
    assert_eq!(ops(&c), [Op::WorktreeAdd]);
    let c = classify(
        LineKind::Update,
        &chain(&[("fetch", None), ("merge", None), ("rebase", None)]),
        Some(&fresh(1, 1)),
        &needs,
    );
    assert_eq!(ops(&c), [Op::Rebase]);
    let c = classify(
        LineKind::Update,
        &chain(&[("commit", None), ("fetch", None), ("pull", None)]),
        Some(&fresh(1, 1)),
        &needs,
    );
    assert_eq!(ops(&c), [Op::Merge]);
    let c = classify(
        LineKind::Update,
        &chain(&[("commit", None), ("fetch", None)]),
        Some(&fresh(1, 0)),
        &needs,
    );
    assert!(ops(&c).is_empty());
}

#[test]
fn classify_an_unprovable_creation_is_unverifiable_only_when_worktree_add_matters() {
    let ch = unknown_chain();
    let c = classify(LineKind::Create, &ch, None, &denying(&["worktree-add"]));
    assert!(c.unverifiable);
    let c = classify(LineKind::Create, &ch, None, &all_allow());
    assert!(!c.unverifiable);
    // A creation under a `git` that is certainly not `worktree add` needs no proof.
    let c = classify(
        LineKind::Create,
        &chain(&[("branch", None)]),
        None,
        &denying(&["worktree-add"]),
    );
    assert!(!c.unverifiable);
}

#[test]
fn classify_unknown_with_fresh_commits_is_unverifiable_only_when_rebase_or_merge_matters() {
    for deny in ["rebase", "merge"] {
        let c = classify(
            LineKind::Update,
            &unknown_chain(),
            Some(&fresh(1, 0)),
            &denying(&[deny]),
        );
        assert!(c.unverifiable, "{deny}");
    }
    let c = classify(
        LineKind::Update,
        &unknown_chain(),
        Some(&fresh(1, 0)),
        &denying(&["commit"]),
    );
    assert!(!c.unverifiable);
    assert_eq!(ops(&c), [Op::Commit]);
    // Nothing fresh, nothing to tell apart.
    let c = classify(
        LineKind::Update,
        &unknown_chain(),
        Some(&fresh(0, 0)),
        &denying(&["rebase", "merge"]),
    );
    assert!(!c.unverifiable);
}

#[test]
fn classify_commits_that_cannot_be_counted_are_unverifiable_unless_commit_is_allowed() {
    let broken = Fresh {
        commits: 0,
        merges: 0,
        unverifiable: true,
    };
    let ch = chain(&[("update-ref", None)]);
    let c = classify(LineKind::Update, &ch, Some(&broken), &denying(&["commit"]));
    assert!(c.unverifiable);
    let c = classify(LineKind::Update, &ch, Some(&broken), &all_allow());
    assert!(!c.unverifiable);
}

// ---------------------------------------------------------------------------------------------
// evaluate
// ---------------------------------------------------------------------------------------------

fn ctx(permissions: Option<EffectivePermissions>) -> Context {
    Context {
        bases: vec!["main".into()],
        fold_case: true,
        actor: None,
        authorship: authorship::Effective::default(),
        policies: Policies::default(),
        layer: LogLayer::Hooks,
        // The minimum is off so each permission is seen alone (except where a test says otherwise).
        minimum: false,
        permissions,
    }
}

fn oid(s: &str) -> RefValue {
    RefValue::Oid(s.into())
}

fn push(local: RefValue, remote_ref: &str, remote: RefValue) -> GuardOp {
    GuardOp::Push {
        remote: Untrusted::new("origin"),
        updates: vec![PushUpdate {
            local_ref: (!local.is_zero()).then(|| remote_ref.to_owned()),
            local,
            remote_ref: remote_ref.into(),
            remote,
        }],
    }
}

fn tx(refname: &str, old: RefValue, new: RefValue) -> GuardOp {
    GuardOp::RefTransaction {
        updates: vec![RefUpdate {
            refname: refname.into(),
            old,
            new,
        }],
        orphan_head: None,
    }
}

fn facts_push(ff: FastForward) -> Facts {
    Facts {
        push: vec![Some(ff)],
        ..Facts::default()
    }
}

fn reasons(e: &Evaluation) -> Vec<(Rule, Level)> {
    e.reasons.iter().map(|r| (r.rule, r.level)).collect()
}

fn has(e: &Evaluation, rule: Rule) -> bool {
    e.reasons.iter().any(|r| r.rule == rule)
}

/// The permission `op` is set to `permission` and everything else is `allow`.
fn only(op: &str, permission: &str) -> EffectivePermissions {
    perms(&format!(
        r#"{{"permissions":{{"disableSafeMinimum":true,"{permission}":["{op}"]}}}}"#
    ))
}

#[test]
fn evaluate_push_follows_its_permission() {
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let f = facts_push(FastForward::Yes);
    let e = evaluate(&op, &f, &ctx(Some(only("push", "deny"))));
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(reasons(&e), [(Rule::PermissionPush, Level::Floor)]);
    let e = evaluate(&op, &f, &ctx(Some(only("push", "ask"))));
    assert_eq!(e.effect, Effect::Ask);
    assert_eq!(reasons(&e), [(Rule::PermissionPush, Level::Floor)]);
    let e = evaluate(&op, &f, &ctx(Some(only("push", "allow"))));
    assert_eq!(e.effect, Effect::Allow);
    // Without the capability nothing is evaluated: as before.
    let e = evaluate(&op, &f, &ctx(None));
    assert_eq!(e.effect, Effect::Allow);
}

#[test]
fn evaluate_ask_does_not_look_at_the_actor() {
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let f = facts_push(FastForward::Yes);
    let mut agent = ctx(Some(only("push", "ask")));
    agent.actor = Some(gitraptor_api::AgentKind::ClaudeCode);
    let person = ctx(Some(only("push", "ask")));
    assert_eq!(evaluate(&op, &f, &agent), evaluate(&op, &f, &person));
    assert_eq!(evaluate(&op, &f, &agent).effect, Effect::Ask);
}

#[test]
fn evaluate_a_force_push_needs_push_and_force_push() {
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let f = facts_push(FastForward::NotAncestor);
    let e = evaluate(&op, &f, &ctx(Some(only("force-push", "deny"))));
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(reasons(&e), [(Rule::PermissionForcePush, Level::Floor)]);
    // With the minimum off and force-push allowed by the floor, the push goes.
    let e = evaluate(&op, &f, &ctx(Some(only("force-push", "allow"))));
    assert_eq!(e.effect, Effect::Allow);
    // Both: push and force-push denied name both permissions.
    let both = perms(r#"{"permissions":{"deny":["push","force-push"]}}"#);
    let e = evaluate(&op, &f, &ctx(Some(both)));
    assert!(
        has(&e, Rule::PermissionPush) && has(&e, Rule::PermissionForcePush),
        "{e:?}"
    );
}

#[test]
fn evaluate_the_minimum_and_the_team_name_a_force_push_together() {
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let f = facts_push(FastForward::NotAncestor);
    let mut c = ctx(Some(perms(r#"{"permissions":{"deny":["force-push"]}}"#)));
    c.minimum = true;
    let e = evaluate(&op, &f, &c);
    assert_eq!(e.effect, Effect::Deny);
    assert!(has(&e, Rule::MinimumForcePush), "{e:?}");
    // The minimum is its own reason: no permission reason at the `minimum` level.
    assert!(
        e.reasons
            .iter()
            .filter(|r| r.rule == Rule::PermissionForcePush)
            .all(|r| r.level == Level::Floor),
        "{e:?}"
    );
    assert!(has(&e, Rule::PermissionForcePush), "{e:?}");
}

#[test]
fn evaluate_a_remote_deletion_needs_push_and_branch_delete() {
    let op = push(RefValue::Zero, "refs/heads/feat-x", oid(B));
    let f = Facts {
        push: vec![None],
        ..Facts::default()
    };
    let e = evaluate(&op, &f, &ctx(Some(only("branch-delete", "deny"))));
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(reasons(&e), [(Rule::PermissionBranchDelete, Level::Floor)]);
    let e = evaluate(&op, &f, &ctx(Some(only("push", "deny"))));
    assert_eq!(reasons(&e), [(Rule::PermissionPush, Level::Floor)]);
}

#[test]
fn evaluate_every_cataloged_operation_times_every_permission() {
    let needs_all = all_allow();
    // (operation of the catalog, the guard operation and facts that classify as it, the rule)
    type Case = (&'static str, GuardOp, Facts, Rule);
    let classified = |line: LineKind, ch: Chain, fresh_: Option<Fresh>| Facts {
        classified: vec![classify(line, &ch, fresh_.as_ref(), &needs_all)],
        ..Facts::default()
    };
    let cases: Vec<Case> = vec![
        (
            "commit",
            GuardOp::Commit {
                stage: CommitStage::PreCommit,
            },
            Facts::default(),
            Rule::PermissionCommit,
        ),
        (
            "push",
            push(oid(A), "refs/heads/feat-x", oid(B)),
            facts_push(FastForward::Yes),
            Rule::PermissionPush,
        ),
        (
            "force-push",
            push(oid(A), "refs/heads/feat-x", oid(B)),
            facts_push(FastForward::NotAncestor),
            Rule::PermissionForcePush,
        ),
        (
            "branch-delete",
            tx("refs/heads/feat-x", oid(A), RefValue::Zero),
            classified(LineKind::Delete, chain(&[("branch", None)]), None),
            Rule::PermissionBranchDelete,
        ),
        (
            "rebase",
            GuardOp::Rebase {
                upstream: None,
                branch: None,
            },
            Facts::default(),
            Rule::PermissionRebase,
        ),
        (
            "merge",
            tx("refs/heads/feat-x", oid(A), oid(B)),
            classified(
                LineKind::Update,
                chain(&[("merge", None)]),
                Some(fresh(1, 1)),
            ),
            Rule::PermissionMerge,
        ),
        (
            "worktree-add",
            tx("refs/heads/new", RefValue::Zero, oid(B)),
            classified(LineKind::Create, chain(&[("worktree", Some("add"))]), None),
            Rule::PermissionWorktreeAdd,
        ),
    ];
    for (name, op, facts, rule) in cases {
        for (permission, effect) in [("deny", Effect::Deny), ("ask", Effect::Ask)] {
            let e = evaluate(&op, &facts, &ctx(Some(only(name, permission))));
            assert_eq!(e.effect, effect, "{name} {permission}: {e:?}");
            assert!(has(&e, rule), "{name} {permission}: {e:?}");
            assert!(
                e.reasons.iter().all(|r| r.level == Level::Floor),
                "{name}: {e:?}"
            );
        }
        let e = evaluate(&op, &facts, &ctx(Some(only(name, "allow"))));
        assert_eq!(e.effect, Effect::Allow, "{name} allow: {e:?}");
        let e = evaluate(&op, &facts, &ctx(Some(all_allow())));
        assert_eq!(e.effect, Effect::Allow, "{name} default: {e:?}");
        let e = evaluate(&op, &facts, &ctx(None));
        assert_eq!(e.effect, Effect::Allow, "{name} without permissions: {e:?}");
    }
}

#[test]
fn evaluate_only_the_precommit_stage_evaluates_commit() {
    let denied = Some(only("commit", "deny"));
    for stage in [CommitStage::CommitMsg, CommitStage::SecondLine] {
        let op = GuardOp::Commit { stage };
        let e = evaluate(&op, &Facts::default(), &ctx(denied.clone()));
        assert!(!has(&e, Rule::PermissionCommit), "{stage:?}: {e:?}");
    }
    let op = GuardOp::Commit {
        stage: CommitStage::PreCommit,
    };
    let e = evaluate(&op, &Facts::default(), &ctx(denied));
    assert!(has(&e, Rule::PermissionCommit), "{e:?}");
}

#[test]
fn evaluate_an_unverifiable_classification_is_denied_with_its_cause() {
    let needs = denying(&["worktree-add"]);
    let classified = classify(LineKind::Create, &unknown_chain(), None, &needs);
    assert!(classified.unverifiable);
    let facts = Facts {
        classified: vec![classified],
        ..Facts::default()
    };
    let op = tx("refs/heads/new", RefValue::Zero, oid(B));
    let e = evaluate(&op, &facts, &ctx(Some(needs)));
    assert_eq!(e.effect, Effect::Deny, "{e:?}");
    assert!(
        e.reasons
            .iter()
            .any(|r| r.cause == Some(Cause::Unverifiable)),
        "{e:?}"
    );
}

#[test]
fn evaluate_names_every_level_of_the_maximum() {
    let floor = settings(r#"{"permissions":{"deny":["push"]}}"#);
    let profile = settings(r#"{"permissions":{"deny":["push"]}}"#);
    let local = settings(r#"{"permissions":{"ask":["push"]}}"#);
    let p = combine(
        Some((SourceKind::Floor, &floor)),
        true,
        &[(SourceKind::Profile, &profile), (SourceKind::Local, &local)],
    );
    assert_eq!(p.permission(Op::Push), Permission::Deny);
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let e = evaluate(&op, &facts_push(FastForward::Yes), &ctx(Some(p)));
    // One reason per level that produces the maximum (deny): the floor and the profile; the
    // local `ask` is below the maximum.
    let mut found = reasons(&e);
    found.sort_by_key(|(_, l)| format!("{l:?}"));
    assert_eq!(
        found,
        [
            (Rule::PermissionPush, Level::Floor),
            (Rule::PermissionPush, Level::Profile)
        ]
    );
}

#[test]
fn evaluate_permissions_only_add_to_what_the_rest_decides() {
    // A protected-by-minimum force push stays denied with the team allowing it, while the minimum
    // is active: the permission cannot relax it.
    let op = push(oid(A), "refs/heads/feat-x", oid(B));
    let f = facts_push(FastForward::NotAncestor);
    let mut c = ctx(Some(only("force-push", "allow")));
    c.minimum = true;
    let e = evaluate(&op, &f, &c);
    assert_eq!(e.effect, Effect::Deny);
    assert!(has(&e, Rule::MinimumForcePush));
    assert!(!has(&e, Rule::PermissionForcePush));
}
