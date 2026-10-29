//! T002 (DS-TS-CKP-003 § 3, D1, D2, D4, D18): `normalize` follows the table of D2 and never leaves
//! `registered` empty; `matches` is strict equality (in doubt, it does not match); the layer
//! never changes the decision. Child module of `guard::plan`, attached there with
//! `#[cfg(test)] #[path = "plan_tests.rs"] mod tests;`.

use super::*;
use crate::guard::{Context, Facts, authorship, evaluate, policies};
use gitraptor_api::guard::{
    AuthorshipFacts, CommitStage, LogLayer, Operation, PushUpdate, RefUpdate, RefValue,
};
use gitraptor_api::{AgentKind, Untrusted};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const D: &str = "4444444444444444444444444444444444444444";
const MAIN: &str = "refs/heads/main";
const FEAT: &str = "refs/heads/feat/x";

fn oid(s: &str) -> RefValue {
    RefValue::Oid(s.into())
}

fn facts_of(trailer: bool) -> AuthorshipFacts {
    AuthorshipFacts {
        coauthors: if trailer {
            vec![Some(AgentKind::ClaudeCode)]
        } else {
            Vec::new()
        },
        trailer_table: 1,
        unreadable: false,
    }
}

/// A `reference-transaction` of one update.
fn tx(refname: &str, old: RefValue, new: RefValue) -> Operation {
    txs(vec![(refname, old, new)])
}

fn txs(updates: Vec<(&str, RefValue, RefValue)>) -> Operation {
    Operation::RefTransaction {
        updates: updates
            .into_iter()
            .map(|(refname, old, new)| RefUpdate {
                refname: refname.into(),
                old,
                new,
            })
            .collect(),
        orphan_head: None,
    }
}

fn none() -> MatchFacts {
    MatchFacts::default()
}

fn shapes() -> Vec<PlanShape> {
    vec![
        PlanShape::Merge {
            base: MAIN.into(),
            old: A.into(),
            integrated: B.into(),
        },
        PlanShape::Rebase {
            branch: FEAT.into(),
            upstream: MAIN.into(),
            old_tip: B.into(),
            onto: A.into(),
        },
        PlanShape::CreateWorktree {
            branch: FEAT.into(),
            at: A.into(),
        },
        PlanShape::DeleteWorktree {
            branch: FEAT.into(),
            tip: B.into(),
        },
        PlanShape::Commit {
            branch: FEAT.into(),
            old: A.into(),
            paths: vec!["src/a.rs".into()],
            authorship: facts_of(true),
            header: crate::guard::commit_format::parse(b"feat: x", true, 1),
        },
        PlanShape::Commit {
            branch: FEAT.into(),
            old: A.into(),
            paths: Vec::new(),
            authorship: facts_of(false),
            header: crate::guard::commit_format::parse(b"feat: x", true, 1),
        },
    ]
}

// ----- normalize (D2) ------------------------------------------------------------------------

#[test]
fn a_merge_is_a_transaction_on_the_base_from_old_to_integrated() {
    let n = normalize(&PlanShape::Merge {
        base: MAIN.into(),
        old: A.into(),
        integrated: B.into(),
    });
    assert_eq!(n.evaluated, [tx(MAIN, oid(A), oid(B))]);
    assert_eq!(n.preset.len(), n.evaluated.len());
    assert_eq!(
        n.registered,
        [Expected::MergeInto {
            refname: MAIN.into(),
            old: A.into(),
            integrated: B.into()
        }]
    );
}

#[test]
fn a_rebase_is_the_rebase_plus_the_move_of_the_branch_from_onto_to_its_tip() {
    let n = normalize(&PlanShape::Rebase {
        branch: FEAT.into(),
        upstream: MAIN.into(),
        old_tip: B.into(),
        onto: A.into(),
    });
    assert_eq!(n.evaluated.len(), 2, "{:?}", n.evaluated);
    match &n.evaluated[0] {
        Operation::Rebase { upstream, branch } => {
            assert_eq!(upstream.as_ref().map(Untrusted::raw), Some(MAIN));
            assert_eq!(branch.as_ref().map(Untrusted::raw), Some(FEAT));
        }
        other => panic!("expected the rebase first, got {other:?}"),
    }
    assert_eq!(n.evaluated[1], tx(FEAT, oid(A), oid(B)));
    assert_eq!(n.preset.len(), n.evaluated.len());
    assert_eq!(
        n.registered,
        [
            Expected::RebaseStart {
                upstream: MAIN.into(),
                branch: FEAT.into()
            },
            Expected::RebaseOf {
                refname: FEAT.into(),
                old: B.into(),
                onto: A.into()
            },
        ]
    );
}

#[test]
fn creating_a_worktree_is_the_creation_of_its_new_branch() {
    let n = normalize(&PlanShape::CreateWorktree {
        branch: FEAT.into(),
        at: A.into(),
    });
    assert_eq!(n.evaluated, [tx(FEAT, RefValue::Zero, oid(A))]);
    assert_eq!(
        n.registered,
        [Expected::Exact {
            refname: FEAT.into(),
            old: RefValue::Zero,
            new: oid(A)
        }]
    );
}

#[test]
fn deleting_a_worktree_is_the_deletion_of_its_branch() {
    let n = normalize(&PlanShape::DeleteWorktree {
        branch: FEAT.into(),
        tip: B.into(),
    });
    assert_eq!(n.evaluated, [tx(FEAT, oid(B), RefValue::Zero)]);
    assert_eq!(
        n.registered,
        [Expected::Exact {
            refname: FEAT.into(),
            old: oid(B),
            new: RefValue::Zero
        }]
    );
}

/// Deleting a worktree deletes its branch too, so the plan carries both permissions; every
/// other shape carries exactly its own operation of the catalog.
#[test]
fn delete_worktree_knows_it_deletes_the_branch() {
    use crate::settings::model::Operation as Op;
    let delete = PlanShape::DeleteWorktree {
        branch: FEAT.into(),
        tip: B.into(),
    };
    assert_eq!(
        delete.catalog_operations(),
        [Op::WorktreeRemove, Op::BranchDelete]
    );
    for shape in shapes() {
        let expected: &[Op] = match &shape {
            PlanShape::Merge { .. } => &[Op::Merge],
            PlanShape::Rebase { .. } => &[Op::Rebase],
            PlanShape::CreateWorktree { .. } => &[Op::WorktreeAdd],
            PlanShape::DeleteWorktree { .. } => &[Op::WorktreeRemove, Op::BranchDelete],
            PlanShape::Commit { .. } => &[Op::Commit],
        };
        assert_eq!(shape.catalog_operations(), expected, "{shape:?}");
    }
}

/// D2: the commit is evaluated as a commit plus the move of its branch with the sentinel
/// `old -> old` (the new oid does not exist yet), with the prepared paths preloaded and the
/// configuration ones the prepared paths under `.gitraptor/`.
#[test]
fn a_commit_is_the_commit_plus_the_old_to_old_sentinel_with_the_prepared_paths() {
    let facts = facts_of(true);
    let n = normalize(&PlanShape::Commit {
        branch: FEAT.into(),
        old: A.into(),
        paths: vec!["src/a.rs".into(), ".gitraptor/settings.json".into()],
        authorship: facts.clone(),
        header: crate::guard::commit_format::parse(b"feat: x", true, 1),
    });
    assert_eq!(
        n.evaluated,
        [
            Operation::Commit {
                stage: CommitStage::CommitMsg
            },
            tx(FEAT, oid(A), oid(A)),
            Operation::Commit {
                stage: CommitStage::PreCommit
            },
        ]
    );
    assert_eq!(n.preset.len(), 3);
    assert_eq!(
        n.preset[1].touched,
        Some(policies::Touched {
            paths: vec!["src/a.rs".into(), ".gitraptor/settings.json".into()],
            unverifiable: false
        })
    );
    assert_eq!(
        n.preset[1].config_touched,
        Some(policies::Touched {
            paths: vec![".gitraptor/settings.json".into()],
            unverifiable: false
        })
    );
    assert_eq!(
        n.registered,
        [
            Expected::CommitStages { authorship: facts },
            Expected::NewCommitOn {
                refname: FEAT.into(),
                old: A.into(),
                paths: vec!["src/a.rs".into(), ".gitraptor/settings.json".into()]
            },
        ]
    );
}

#[test]
fn more_than_100_000_prepared_paths_are_unverifiable() {
    let paths: Vec<String> = (0..100_001).map(|i| format!("f/{i}")).collect();
    let n = normalize(&PlanShape::Commit {
        branch: FEAT.into(),
        old: A.into(),
        paths,
        authorship: facts_of(true),
        header: crate::guard::commit_format::parse(b"feat: x", true, 1),
    });
    let touched = n.preset[1].touched.as_ref().expect("preloaded paths");
    assert!(touched.unverifiable, "{} paths kept", touched.paths.len());
}

#[test]
fn registered_is_never_empty() {
    for shape in shapes() {
        let n = normalize(&shape);
        assert!(!n.registered.is_empty(), "{shape:?}");
        assert!(!n.evaluated.is_empty(), "{shape:?}");
        assert_eq!(n.preset.len(), n.evaluated.len(), "aligned: {shape:?}");
    }
}

// ----- matches: strict equality, in doubt it does not match ------------------------------------

#[test]
fn an_exact_transition_matches_only_itself() {
    let reg = [Expected::Exact {
        refname: FEAT.into(),
        old: RefValue::Zero,
        new: oid(A),
    }];
    assert!(matches(
        &reg,
        &tx(FEAT, RefValue::Zero, oid(A)),
        None,
        &none()
    ));
    assert!(!matches(
        &reg,
        &tx(FEAT, RefValue::Zero, oid(B)),
        None,
        &none()
    ));
    assert!(!matches(
        &reg,
        &tx(MAIN, RefValue::Zero, oid(A)),
        None,
        &none()
    ));
    assert!(!matches(&reg, &tx(FEAT, oid(B), oid(A)), None, &none()));
    // One update that matches and one that does not: the transaction does not match.
    let two = txs(vec![(FEAT, RefValue::Zero, oid(A)), (MAIN, oid(A), oid(B))]);
    assert!(!matches(&reg, &two, None, &none()));
    assert!(!matches(
        &[],
        &tx(FEAT, RefValue::Zero, oid(A)),
        None,
        &none()
    ));
}

#[test]
fn a_zero_old_never_matches_an_expected_old() {
    let reg = [
        Expected::MergeInto {
            refname: MAIN.into(),
            old: A.into(),
            integrated: B.into(),
        },
        Expected::Exact {
            refname: FEAT.into(),
            old: oid(A),
            new: RefValue::Zero,
        },
    ];
    assert!(!matches(
        &reg,
        &tx(MAIN, RefValue::Zero, oid(B)),
        None,
        &none()
    ));
    assert!(!matches(
        &reg,
        &tx(FEAT, RefValue::Zero, RefValue::Zero),
        None,
        &none()
    ));
}

#[test]
fn a_merge_matches_the_integrated_oid_or_a_merge_of_old_and_integrated() {
    let reg = [Expected::MergeInto {
        refname: MAIN.into(),
        old: A.into(),
        integrated: B.into(),
    }];
    // A fast-forward: the new value is the integrated one.
    assert!(matches(&reg, &tx(MAIN, oid(A), oid(B)), None, &none()));
    // A merge commit: its parents are [old, integrated], read by the I/O side.
    let merge = MatchFacts {
        parents: vec![A.into(), B.into()],
        ..MatchFacts::default()
    };
    assert!(matches(&reg, &tx(MAIN, oid(A), oid(C)), None, &merge));
    for parents in [
        vec![A.into(), D.into()],
        vec![A.into()],
        vec![],
        vec![B.into(), A.into()],
    ] {
        let facts = MatchFacts {
            parents,
            ..MatchFacts::default()
        };
        assert!(
            !matches(&reg, &tx(MAIN, oid(A), oid(C)), None, &facts),
            "{facts:?}"
        );
    }
    assert!(!matches(&reg, &tx(MAIN, oid(D), oid(B)), None, &none()));
    assert!(!matches(&reg, &tx(FEAT, oid(A), oid(B)), None, &none()));
}

#[test]
fn a_rebase_start_matches_its_upstream_and_branch() {
    let reg = [Expected::RebaseStart {
        upstream: MAIN.into(),
        branch: FEAT.into(),
    }];
    let rebase = |upstream: Option<&str>, branch: Option<&str>| Operation::Rebase {
        upstream: upstream.map(Untrusted::new),
        branch: branch.map(Untrusted::new),
    };
    assert!(matches(
        &reg,
        &rebase(Some(MAIN), Some(FEAT)),
        None,
        &none()
    ));
    assert!(!matches(
        &reg,
        &rebase(Some(MAIN), Some("refs/heads/other")),
        None,
        &none()
    ));
    assert!(!matches(
        &reg,
        &rebase(Some("refs/heads/other"), Some(FEAT)),
        None,
        &none()
    ));
    // A rebase that names neither is a doubt.
    assert!(!matches(&reg, &rebase(None, None), None, &none()));
    assert!(!matches(&reg, &rebase(Some(MAIN), None), None, &none()));
}

#[test]
fn a_rebase_move_needs_its_old_and_a_new_value_descending_from_onto() {
    let reg = [Expected::RebaseOf {
        refname: FEAT.into(),
        old: B.into(),
        onto: A.into(),
    }];
    let descends = MatchFacts {
        descends_from_onto: true,
        ..MatchFacts::default()
    };
    assert!(matches(&reg, &tx(FEAT, oid(B), oid(C)), None, &descends));
    assert!(!matches(&reg, &tx(FEAT, oid(B), oid(C)), None, &none()));
    assert!(!matches(&reg, &tx(FEAT, oid(D), oid(C)), None, &descends));
    assert!(!matches(
        &reg,
        &tx(FEAT, RefValue::Zero, oid(C)),
        None,
        &descends
    ));
    assert!(!matches(&reg, &tx(MAIN, oid(B), oid(C)), None, &descends));
    assert!(!matches(
        &reg,
        &tx(FEAT, oid(B), RefValue::Zero),
        None,
        &descends
    ));
}

/// `pre-commit` carries no facts; `commit-msg` and the second line carry the ones derived
/// from the message, which must be the plan's.
#[test]
fn a_commit_stage_matches_the_same_authorship_facts() {
    let plan = facts_of(true);
    let reg = [Expected::CommitStages {
        authorship: plan.clone(),
    }];
    let stage = |stage| Operation::Commit { stage };
    assert!(matches(&reg, &stage(CommitStage::PreCommit), None, &none()));
    for s in [CommitStage::CommitMsg, CommitStage::SecondLine] {
        assert!(matches(&reg, &stage(s), Some(&plan), &none()), "{s:?}");
        assert!(
            !matches(&reg, &stage(s), Some(&facts_of(false)), &none()),
            "{s:?}"
        );
        assert!(!matches(&reg, &stage(s), None, &none()), "{s:?}");
    }
}

#[test]
fn a_new_commit_matches_its_parent_and_a_subset_of_the_prepared_paths() {
    let reg = [Expected::NewCommitOn {
        refname: FEAT.into(),
        old: A.into(),
        paths: vec!["src/a.rs".into(), "src/b.rs".into()],
    }];
    let with = |parents: Vec<String>, new_paths: Option<Vec<&str>>| MatchFacts {
        parents,
        descends_from_onto: false,
        new_paths: new_paths.map(|p| p.into_iter().map(str::to_owned).collect()),
    };
    let new = tx(FEAT, oid(A), oid(C));
    assert!(matches(
        &reg,
        &new,
        None,
        &with(vec![A.into()], Some(vec!["src/a.rs"]))
    ));
    assert!(matches(
        &reg,
        &new,
        None,
        &with(vec![A.into()], Some(vec!["src/b.rs", "src/a.rs"]))
    ));
    // A path the plan did not prepare, paths nobody read, a merge commit, another parent.
    assert!(!matches(
        &reg,
        &new,
        None,
        &with(vec![A.into()], Some(vec!["src/a.rs", "x"]))
    ));
    assert!(!matches(&reg, &new, None, &with(vec![A.into()], None)));
    assert!(!matches(
        &reg,
        &new,
        None,
        &with(vec![A.into(), B.into()], Some(vec!["src/a.rs"]))
    ));
    assert!(!matches(
        &reg,
        &new,
        None,
        &with(vec![B.into()], Some(vec!["src/a.rs"]))
    ));
    assert!(!matches(
        &reg,
        &new,
        None,
        &with(vec![], Some(vec!["src/a.rs"]))
    ));
    // Another ref, another old value, a deletion.
    let ok = with(vec![A.into()], Some(vec!["src/a.rs"]));
    assert!(!matches(&reg, &tx(MAIN, oid(A), oid(C)), None, &ok));
    assert!(!matches(&reg, &tx(FEAT, oid(B), oid(C)), None, &ok));
    assert!(!matches(&reg, &tx(FEAT, RefValue::Zero, oid(C)), None, &ok));
    assert!(!matches(&reg, &tx(FEAT, oid(A), RefValue::Zero), None, &ok));
}

#[test]
fn the_abort_move_back_matches_its_restore_value_from_any_old() {
    let reg = [Expected::RestoreTo {
        refname: FEAT.into(),
        value: B.into(),
    }];
    assert!(matches(&reg, &tx(FEAT, oid(C), oid(B)), None, &none()));
    assert!(matches(&reg, &tx(FEAT, oid(D), oid(B)), None, &none()));
    assert!(!matches(&reg, &tx(FEAT, oid(C), oid(D)), None, &none()));
    assert!(!matches(
        &reg,
        &tx(FEAT, oid(C), RefValue::Zero),
        None,
        &none()
    ));
    assert!(!matches(&reg, &tx(MAIN, oid(C), oid(B)), None, &none()));
}

#[test]
fn an_operation_of_another_kind_never_matches() {
    let reg: Vec<Expected> = shapes()
        .iter()
        .flat_map(|s| normalize(s).registered)
        .collect();
    let push = Operation::Push {
        remote: Untrusted::new("origin"),
        updates: vec![PushUpdate {
            local_ref: Some(FEAT.into()),
            local: oid(A),
            remote_ref: FEAT.into(),
            remote: oid(B),
        }],
    };
    assert!(!matches(&reg, &push, None, &none()));
}

// ----- D4: the layer never changes the decision ---------------------------------------------

/// Property (enumerated: the repo has no property-testing dependency): the same input with the
/// three layers gives the same `Evaluation`.
#[test]
fn the_layer_never_changes_the_decision() {
    let operations = [
        tx(MAIN, oid(A), RefValue::Zero),
        tx(FEAT, oid(A), oid(B)),
        tx(FEAT, RefValue::Zero, oid(A)),
        Operation::Commit {
            stage: CommitStage::CommitMsg,
        },
        Operation::Rebase {
            upstream: Some(Untrusted::new(MAIN)),
            branch: Some(Untrusted::new(FEAT)),
        },
        Operation::Push {
            remote: Untrusted::new("origin"),
            updates: vec![PushUpdate {
                local_ref: Some(MAIN.into()),
                local: oid(A),
                remote_ref: MAIN.into(),
                remote: oid(B),
            }],
        },
    ];
    let mut denied = 0;
    for operation in &operations {
        for actor in [None, Some(AgentKind::ClaudeCode)] {
            for authorship in [None, Some(facts_of(true)), Some(facts_of(false))] {
                let facts = Facts {
                    authorship: authorship.clone(),
                    ..Facts::default()
                };
                let decide = |layer| {
                    let ctx = Context {
                        bases: vec!["main".into()],
                        fold_case: true,
                        actor,
                        authorship: authorship::Effective::default(),
                        policies: policies::Policies::default(),
                        commit_policies: Default::default(),
                        layer,
                        minimum: true,
                        permissions: None,
                    };
                    evaluate(operation, &facts, &ctx)
                };
                let hooks = decide(LogLayer::Hooks);
                denied += usize::from(hooks.effect != gitraptor_api::guard::Effect::Allow);
                assert_eq!(hooks, decide(LogLayer::Mcp), "{operation:?} {actor:?}");
                assert_eq!(hooks, decide(LogLayer::Cockpit), "{operation:?} {actor:?}");
            }
        }
    }
    assert!(denied > 0, "the property must cover denials too");
}
