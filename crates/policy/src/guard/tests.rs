//! ADR-GRD-003 Validación 1, 2 y 4 on the pure function.

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, Effect, Operation, OrphanHead, PushUpdate, RefBackend, RefUpdate, RefValue, Rule,
};

use super::*;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";

fn ctx() -> Context {
    Context {
        bases: vec!["main".into()],
        fold_case: true,
        actor: None,
        authorship: authorship::Effective::default(),
        policies: policies::Policies::default(),
    }
}

fn oid(s: &str) -> RefValue {
    RefValue::Oid(s.into())
}

fn push(updates: Vec<PushUpdate>) -> Operation {
    Operation::Push {
        remote: Untrusted::new("origin"),
        updates,
    }
}

fn pu(remote_ref: &str, local: RefValue, remote: RefValue) -> PushUpdate {
    PushUpdate {
        local_ref: (!local.is_zero()).then(|| remote_ref.to_owned()),
        local,
        remote_ref: remote_ref.into(),
        remote,
    }
}

fn tx(updates: Vec<(&str, RefValue, RefValue)>) -> Operation {
    Operation::RefTransaction {
        updates: updates
            .into_iter()
            .map(|(r, old, new)| RefUpdate {
                refname: r.into(),
                old,
                new,
            })
            .collect(),
        orphan_head: None,
    }
}

fn facts(push: Vec<Option<FastForward>>) -> Facts {
    Facts {
        push,
        ..Facts::default()
    }
}

fn rules(e: &Evaluation) -> Vec<(Rule, Option<Cause>)> {
    e.reasons.iter().map(|r| (r.rule, r.cause)).collect()
}

#[test]
fn force_push_of_any_branch_is_denied_by_the_minimum() {
    for (fact, cause) in [
        (Some(FastForward::NotAncestor), Cause::NotFastForward),
        (Some(FastForward::RemoteMissing), Cause::RemoteObjectMissing),
        (Some(FastForward::Shallow), Cause::ShallowHistory),
        (None, Cause::RemoteObjectMissing),
    ] {
        let op = push(vec![pu("refs/heads/feat-x", oid(A), oid(B))]);
        let e = evaluate(&op, &facts(vec![fact]), &ctx());
        assert_eq!(e.effect, Effect::Deny);
        assert_eq!(rules(&e), vec![(Rule::MinimumForcePush, Some(cause))]);
        assert_eq!(e.reasons[0].level, Level::Minimum);
    }
}

#[test]
fn fast_forward_creation_and_tags_are_allowed() {
    let ff = push(vec![pu("refs/heads/main", oid(A), oid(B))]);
    assert_eq!(
        evaluate(&ff, &facts(vec![Some(FastForward::Yes)]), &ctx()).effect,
        Effect::Allow
    );
    let create = push(vec![pu("refs/heads/new", oid(A), RefValue::Zero)]);
    assert_eq!(
        evaluate(&create, &facts(vec![None]), &ctx()).effect,
        Effect::Allow
    );
    let tag = push(vec![pu("refs/tags/v1", oid(A), oid(B))]);
    assert_eq!(
        evaluate(&tag, &facts(vec![Some(FastForward::NotAncestor)]), &ctx()).effect,
        Effect::Allow
    );
    let delete_feature = push(vec![pu("refs/heads/feat", RefValue::Zero, oid(B))]);
    assert_eq!(
        evaluate(&delete_feature, &facts(vec![None]), &ctx()).effect,
        Effect::Allow
    );
}

#[test]
fn deleting_the_base_branch_is_denied_locally_and_remotely() {
    let remote = push(vec![pu("refs/heads/main", RefValue::Zero, oid(A))]);
    let e = evaluate(&remote, &facts(vec![None]), &ctx());
    assert_eq!(
        rules(&e),
        vec![(Rule::MinimumBaseBranchDelete, Some(Cause::Delete))]
    );

    // Explicit deletion: the packed-refs transaction comes first with old zero (D06).
    for old in [oid(A), RefValue::Zero] {
        let local = tx(vec![("refs/heads/main", old, RefValue::Zero)]);
        let e = evaluate(&local, &Facts::default(), &ctx());
        assert_eq!(
            rules(&e),
            vec![(Rule::MinimumBaseBranchDelete, Some(Cause::Delete))]
        );
    }
}

#[test]
fn a_commit_or_a_feature_deletion_passes() {
    let commit = tx(vec![
        ("refs/heads/feat-x", oid(A), oid(B)),
        ("HEAD", oid(A), oid(B)),
        ("AUTO_MERGE", RefValue::Zero, oid(B)),
    ]);
    assert_eq!(
        evaluate(&commit, &Facts::default(), &ctx()),
        Evaluation::allow()
    );
    let delete = tx(vec![("refs/heads/feat-x", oid(A), RefValue::Zero)]);
    assert_eq!(
        evaluate(&delete, &Facts::default(), &ctx()).effect,
        Effect::Allow
    );
    let rebase = Operation::Rebase {
        upstream: None,
        branch: None,
    };
    assert_eq!(
        evaluate(&rebase, &Facts::default(), &ctx()).effect,
        Effect::Allow
    );
}

#[test]
fn aliases_are_denied_on_every_line() {
    // branch -f Main x rewrites main on a case-insensitive file system (D21b).
    let rewrite = tx(vec![("refs/heads/Main", oid(A), oid(B))]);
    let e = evaluate(&rewrite, &Facts::default(), &ctx());
    assert_eq!(
        rules(&e),
        vec![(Rule::MinimumBaseBranchDelete, Some(Cause::Alias))]
    );
    // A push to refs/heads/Main arrives as a creation (F11).
    let create = push(vec![pu("refs/heads/Main", oid(A), RefValue::Zero)]);
    assert_eq!(
        evaluate(&create, &facts(vec![None]), &ctx()).effect,
        Effect::Deny
    );
    // A case-sensitive file system does not fold, but NFC still applies.
    let sensitive = Context {
        fold_case: false,
        ..ctx()
    };
    assert_eq!(
        evaluate(&rewrite, &Facts::default(), &sensitive).effect,
        Effect::Allow
    );
    let nfc = Context {
        bases: vec!["caf\u{e9}".into()],
        fold_case: false,
        ..ctx()
    };
    let nfd = tx(vec![("refs/heads/cafe\u{301}", oid(A), RefValue::Zero)]);
    assert_eq!(evaluate(&nfd, &Facts::default(), &nfc).effect, Effect::Deny);
}

#[test]
fn every_denying_rule_is_named() {
    // BR-CALC-001: a push that deletes the base and forces a feature names both.
    let op = push(vec![
        pu("refs/heads/main", RefValue::Zero, oid(A)),
        pu("refs/heads/feat", oid(A), oid(B)),
    ]);
    let e = evaluate(
        &op,
        &facts(vec![None, Some(FastForward::NotAncestor)]),
        &ctx(),
    );
    assert_eq!(
        rules(&e),
        vec![
            (Rule::MinimumBaseBranchDelete, Some(Cause::Delete)),
            (Rule::MinimumForcePush, Some(Cause::NotFastForward)),
        ]
    );
    // The same reason twice is named once.
    let twice = tx(vec![
        ("refs/heads/main", RefValue::Zero, RefValue::Zero),
        ("refs/heads/main", oid(A), RefValue::Zero),
    ]);
    assert_eq!(evaluate(&twice, &Facts::default(), &ctx()).reasons.len(), 1);
}

#[test]
fn every_base_of_the_union_is_protected() {
    let union = Context {
        bases: vec!["main".into(), "trunk".into()],
        fold_case: false,
        ..ctx()
    };
    for base in ["refs/heads/main", "refs/heads/trunk"] {
        let op = tx(vec![(base, oid(A), RefValue::Zero)]);
        assert_eq!(
            evaluate(&op, &Facts::default(), &union).effect,
            Effect::Deny,
            "{base}"
        );
    }
}

#[test]
fn rename_onto_base_carries_the_oid_to_recover() {
    let op = Operation::RefTransaction {
        updates: vec![RefUpdate {
            refname: "refs/heads/main".into(),
            old: RefValue::Zero,
            new: RefValue::Zero,
        }],
        orphan_head: Some(OrphanHead {
            branch: "feat".into(),
            oid: A.into(),
        }),
    };
    let e = evaluate(&op, &Facts::default(), &ctx());
    assert_eq!(
        rules(&e),
        vec![(Rule::MinimumBaseBranchDelete, Some(Cause::RenameOntoBase))]
    );
    let values: Vec<_> = e.reasons[0].params.iter().map(|p| p.value.raw()).collect();
    assert_eq!(values, vec!["main", "feat", A]);
}

#[test]
fn the_function_is_deterministic() {
    // Validación 1: the same input always gives the same decision.
    let ops = [
        push(vec![
            pu("refs/heads/main", RefValue::Zero, oid(A)),
            pu("refs/heads/f", oid(A), oid(B)),
        ]),
        tx(vec![("refs/heads/Main", oid(A), RefValue::Zero)]),
    ];
    let f = facts(vec![None, Some(FastForward::Shallow)]);
    for op in &ops {
        let first = evaluate(op, &f, &ctx());
        for _ in 0..50 {
            assert_eq!(evaluate(op, &f, &ctx()), first);
        }
    }
}

#[test]
fn reftable_adds_the_rename_of_the_base() {
    assert!(!not_preventable(RefBackend::Files).contains(&NotPreventable::RenameBaseReftable));
    assert!(not_preventable(RefBackend::Reftable).contains(&NotPreventable::RenameBaseReftable));
}

mod policies_in_the_function {
    use super::*;
    use crate::guard::glob::{Kind, Pattern};
    use crate::guard::policies::{Policies, Rules, Scope, Touched};
    use gitraptor_api::AgentKind;
    use gitraptor_api::guard::Level;

    fn rules(scope: Scope, kind: Kind, patterns: &[&str]) -> Vec<Rules> {
        vec![Rules {
            level: Level::Floor,
            scope,
            patterns: patterns
                .iter()
                .map(|p| Pattern::new(p, kind).unwrap())
                .collect(),
        }]
    }

    fn with_policies(actor: Option<AgentKind>, scope: Scope) -> Context {
        Context {
            actor,
            policies: Policies {
                branches: rules(scope, Kind::Branch, &["main", "release/*"]),
                paths: rules(scope, Kind::Path, &["secrets/"]),
                unreadable: false,
            },
            ..ctx()
        }
    }

    const AGENT: Option<AgentKind> = Some(AgentKind::ClaudeCode);

    fn touched(paths: &[&str]) -> Facts {
        let what = Touched {
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            unverifiable: false,
        };
        Facts {
            touched: vec![Some(what.clone())],
            config_touched: vec![Some(what)],
            ..Facts::default()
        }
    }

    fn rules_of(e: &Evaluation) -> Vec<Rule> {
        e.reasons.iter().map(|r| r.rule).collect()
    }

    #[test]
    fn an_agent_moving_a_protected_branch_is_denied_in_every_way_it_can_move_it() {
        let ctx = with_policies(AGENT, Scope::Agents);
        // Commit (update), creation, deletion.
        for (old, new, branch) in [
            (oid(A), oid(B), "refs/heads/main"),
            (RefValue::Zero, oid(B), "refs/heads/release/1.0"),
            (oid(A), RefValue::Zero, "refs/heads/main"),
        ] {
            let e = evaluate(&tx(vec![(branch, old, new)]), &Facts::default(), &ctx);
            assert_eq!(e.effect, Effect::Deny, "{branch}");
            assert!(rules_of(&e).contains(&Rule::ProtectedBranch), "{branch}");
        }
        // Push: update, creation and deletion of the remote ref.
        for (local, remote) in [
            (oid(B), oid(A)),
            (oid(A), RefValue::Zero),
            (RefValue::Zero, oid(A)),
        ] {
            let e = evaluate(
                &push(vec![pu("refs/heads/main", local, remote)]),
                &Facts {
                    push: vec![Some(FastForward::Yes)],
                    ..Facts::default()
                },
                &ctx,
            );
            assert_eq!(e.effect, Effect::Deny);
            assert!(rules_of(&e).contains(&Rule::ProtectedBranch));
        }
        // Another branch and a tag are free.
        for refname in ["refs/heads/feat-x", "refs/tags/main"] {
            let e = evaluate(
                &tx(vec![(refname, oid(A), oid(B))]),
                &Facts::default(),
                &ctx,
            );
            assert_eq!(e.effect, Effect::Allow, "{refname}");
        }
    }

    #[test]
    fn the_person_passes_unless_the_rule_is_for_everyone() {
        let update = tx(vec![("refs/heads/main", oid(A), oid(B))]);
        let person = with_policies(None, Scope::Agents);
        assert_eq!(
            evaluate(&update, &touched(&["secrets/a"]), &person).effect,
            Effect::Allow
        );
        let everyone = with_policies(None, Scope::Everyone);
        let e = evaluate(&update, &touched(&["secrets/a"]), &everyone);
        assert_eq!(e.effect, Effect::Deny);
        assert_eq!(rules_of(&e), [Rule::ProtectedBranch, Rule::ForbiddenPath]);
    }

    #[test]
    fn the_configuration_directory_is_protected_with_no_policies_at_all() {
        let update = tx(vec![("refs/heads/feat-x", oid(A), oid(B))]);
        let agent = Context {
            actor: AGENT,
            ..ctx()
        };
        for path in [".gitraptor/settings.json", ".gitraptor"] {
            let e = evaluate(&update, &touched(&[path]), &agent);
            assert_eq!(e.effect, Effect::Deny, "{path}");
            assert_eq!(rules_of(&e), [Rule::ConfigProtected]);
            assert_eq!(e.reasons[0].level, Level::Minimum);
        }
        // Outside the directory, on another ref kind, or by the person: free.
        assert_eq!(
            evaluate(&update, &touched(&["src/a"]), &agent).effect,
            Effect::Allow
        );
        let tag = tx(vec![("refs/tags/v1", oid(A), oid(B))]);
        assert_eq!(
            evaluate(&tag, &touched(&[".gitraptor/settings.json"]), &agent).effect,
            Effect::Allow
        );
        let person = Context {
            actor: None,
            ..ctx()
        };
        assert_eq!(
            evaluate(&update, &touched(&[".gitraptor/settings.json"]), &person).effect,
            Effect::Allow
        );
        // An agent's movement whose commits cannot be read is denied with no rules at all.
        let unreadable = Facts {
            config_touched: vec![Some(Touched {
                paths: Vec::new(),
                unverifiable: true,
            })],
            ..Facts::default()
        };
        let e = evaluate(&update, &unreadable, &agent);
        assert_eq!(e.effect, Effect::Deny);
        assert_eq!(e.reasons[0].cause, Some(Cause::Unverifiable));
        // It names itself next to the other rules that stop the movement.
        let both = Context {
            actor: AGENT,
            ..with_policies(AGENT, Scope::Agents)
        };
        let e = evaluate(
            &tx(vec![("refs/heads/main", oid(A), oid(B))]),
            &touched(&[".gitraptor/x", "secrets/k"]),
            &both,
        );
        assert_eq!(
            rules_of(&e),
            [
                Rule::ConfigProtected,
                Rule::ProtectedBranch,
                Rule::ForbiddenPath
            ]
        );
    }

    #[test]
    fn two_broken_rules_are_named_together_with_the_minimum() {
        let ctx = with_policies(AGENT, Scope::Agents);
        // Deleting the base branch that is also protected: the minimum and the policy.
        let e = evaluate(
            &tx(vec![("refs/heads/main", oid(A), RefValue::Zero)]),
            &Facts::default(),
            &ctx,
        );
        assert_eq!(
            rules_of(&e),
            [Rule::MinimumBaseBranchDelete, Rule::ProtectedBranch]
        );
        // A commit on a protected branch touching a forbidden path.
        let e = evaluate(
            &tx(vec![("refs/heads/main", oid(A), oid(B))]),
            &touched(&["src/a", "secrets/api.txt"]),
            &ctx,
        );
        assert_eq!(rules_of(&e), [Rule::ProtectedBranch, Rule::ForbiddenPath]);
    }

    #[test]
    fn a_forbidden_path_denies_a_push_of_any_branch() {
        let ctx = with_policies(AGENT, Scope::Agents);
        let facts = Facts {
            push: vec![Some(FastForward::Yes)],
            ..touched(&["secrets/api.txt"])
        };
        let e = evaluate(
            &push(vec![pu("refs/heads/feat-x", oid(B), oid(A))]),
            &facts,
            &ctx,
        );
        assert_eq!(rules_of(&e), [Rule::ForbiddenPath]);
    }

    #[test]
    fn only_the_forbidden_paths_reach_a_ref_that_is_not_governed() {
        let ctx = with_policies(AGENT, Scope::Agents);
        let facts = Facts {
            push: vec![None],
            ..touched(&["secrets/api.txt"])
        };
        // A tag named like a protected branch is not that branch: only the path rule applies.
        for refname in ["refs/tags/main", "refs/notes/main", "refs/tags/v1"] {
            let e = evaluate(
                &push(vec![pu(refname, oid(B), RefValue::Zero)]),
                &facts,
                &ctx,
            );
            assert_eq!(rules_of(&e), [Rule::ForbiddenPath], "{refname}");
        }
        let clean = Facts {
            push: vec![None],
            ..touched(&["src/a.rs"])
        };
        let e = evaluate(
            &push(vec![pu("refs/tags/main", oid(B), RefValue::Zero)]),
            &clean,
            &ctx,
        );
        assert_eq!(e.effect, Effect::Allow);
        // The person is not governed by a rule for agents.
        let person = with_policies(None, Scope::Agents);
        let e = evaluate(
            &push(vec![pu("refs/tags/v1", oid(B), RefValue::Zero)]),
            &facts,
            &person,
        );
        assert_eq!(e.effect, Effect::Allow);
    }

    #[test]
    fn every_rule_governs_whoever_moves_the_ref() {
        let agents = with_policies(None, Scope::Agents).policies;
        let widened = Context {
            policies: agents.every_rule(),
            ..with_policies(None, Scope::Agents)
        };
        let facts = Facts {
            push: vec![None],
            ..touched(&["secrets/api.txt"])
        };
        let update = push(vec![pu("refs/tags/v1", oid(B), RefValue::Zero)]);
        assert_eq!(evaluate(&update, &facts, &widened).effect, Effect::Deny);
    }

    #[test]
    fn a_configuration_that_cannot_be_read_denies_the_agents_branch_moves_only() {
        let ctx = |actor| Context {
            actor,
            policies: Policies::unreadable(),
            ..ctx()
        };
        let update = tx(vec![("refs/heads/feat-x", oid(A), oid(B))]);
        let e = evaluate(&update, &Facts::default(), &ctx(AGENT));
        assert_eq!(e.effect, Effect::Deny);
        assert_eq!(e.reasons[0].cause, Some(Cause::Unverifiable));
        assert_eq!(
            evaluate(&update, &Facts::default(), &ctx(None)).effect,
            Effect::Allow
        );
    }

    #[test]
    fn no_policies_change_nothing() {
        let e = evaluate(
            &tx(vec![("refs/heads/main", oid(A), oid(B))]),
            &touched(&["secrets/a"]),
            &Context {
                actor: AGENT,
                ..ctx()
            },
        );
        assert_eq!(e.effect, Effect::Allow);
    }
}

mod fast_path {
    use super::super::fastpath::*;
    use std::path::Path;

    const O: &str = "1111111111111111111111111111111111111111";
    const Z: &str = "0000000000000000000000000000000000000000";

    fn input(lines: &[String]) -> Vec<u8> {
        lines
            .iter()
            .flat_map(|l| format!("{l}\n").into_bytes())
            .collect()
    }

    #[test]
    fn only_clearly_ungoverned_lines_are_skipped() {
        let none = Path::new("/nonexistent-common");
        for ok in [
            vec![
                format!("{Z} {O} refs/remotes/origin/main"),
                format!("{Z} {O} refs/tags/v1"),
            ],
            vec![format!("{Z} ref:refs/heads/feat HEAD")],
            vec![
                format!("{O} {O} ORIG_HEAD"),
                format!("{Z} {O} worktrees/wt/REBASE_HEAD"),
            ],
        ] {
            assert!(skippable_ref_transaction(&input(&ok), none, 8192), "{ok:?}");
        }
        for not in [
            vec![format!("{O} {Z} refs/heads/main")],
            vec![format!("{O} {O} HEAD")],
            vec![format!("{O} {Z} HEAD")],
            vec![format!("{Z} {O} refs/new/x")],
            vec![format!("{Z} {O} refs/tags/a b")],
            vec![format!("{Z} {O}")],
            vec![format!("{Z} {O} refs/tags/../heads/main")],
            vec![format!("{} {O} refs/tags/v1", "X".repeat(40))],
        ] {
            assert!(
                !skippable_ref_transaction(&input(&not), none, 8192),
                "{not:?}"
            );
        }
        assert!(!skippable_ref_transaction(b"\xff\n", none, 8192));
        let long = input(&[format!("{Z} {O} refs/tags/{}", "x".repeat(9000))]);
        assert!(!skippable_ref_transaction(&long, none, 8192));
    }

    #[test]
    fn the_prune_of_pack_refs_is_recognized_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let common = dir.path();
        std::fs::create_dir_all(common.join("refs/heads")).unwrap();
        std::fs::write(common.join("refs/heads/main"), format!("{O}\n")).unwrap();
        std::fs::write(
            common.join("packed-refs"),
            format!("# pack-refs with: peeled fully-peeled sorted\n{O} refs/heads/main\n"),
        )
        .unwrap();
        let prune = input(&[format!("{O} {Z} refs/heads/main")]);
        assert!(skippable_ref_transaction(&prune, common, 8192));
        assert!(!deletes_a_branch(&prune, common, 8192));
        // An explicit deletion: the packed-refs transaction has old zero.
        let explicit = input(&[format!("{Z} {Z} refs/heads/main")]);
        assert!(!skippable_ref_transaction(&explicit, common, 8192));
        assert!(deletes_a_branch(&explicit, common, 8192));
        // Loose and packed values that differ: not a prune.
        std::fs::write(
            common.join("refs/heads/main"),
            format!("{}\n", "2".repeat(40)),
        )
        .unwrap();
        assert!(!skippable_ref_transaction(&prune, common, 8192));
        assert!(deletes_a_branch(&prune, common, 8192));
    }

    #[test]
    fn pre_push_skips_only_ungoverned_remote_refs() {
        let tags = input(&[format!("refs/tags/v1 {O} refs/tags/v1 {Z}")]);
        assert!(skippable_push(&tags, 8192));
        let branch = input(&[format!("refs/heads/f {O} refs/heads/f {Z}")]);
        assert!(!skippable_push(&branch, 8192));
        let delete = input(&[format!("(delete) {Z} refs/heads/main {O}")]);
        assert!(!skippable_push(&delete, 8192));
    }

    #[test]
    fn without_raptor_deletions_of_branches_and_head_exit_1() {
        let none = Path::new("/nonexistent-common");
        assert!(deletes_a_branch(
            &input(&[format!("{O} {Z} refs/heads/feat")]),
            none,
            8192
        ));
        assert!(deletes_a_branch(
            &input(&[format!("{O} {Z} HEAD")]),
            none,
            8192
        ));
        assert!(deletes_a_branch(b"garbage\n", none, 8192));
        // A symbolic branch deleted (`symref-delete`) is still a branch deletion; only a
        // symbolic `HEAD` itself is not.
        assert!(deletes_a_branch(
            &input(&[format!("ref:refs/heads/x {Z} refs/heads/main")]),
            none,
            8192
        ));
        assert!(!deletes_a_branch(
            &input(&[format!("ref:refs/heads/x {Z} HEAD")]),
            none,
            8192
        ));
        let spaced = input(&[format!("main@{{1 day ago}} {O} refs/tags/t {Z}")]);
        assert!(skippable_push(&spaced, 8192));
        assert!(!deletes_a_branch(
            &input(&[format!("{O} {O} refs/heads/feat")]),
            none,
            8192
        ));
        assert!(!deletes_a_branch(
            &input(&[format!("{O} {Z} refs/tags/v1")]),
            none,
            8192
        ));
    }
}
