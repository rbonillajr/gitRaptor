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
