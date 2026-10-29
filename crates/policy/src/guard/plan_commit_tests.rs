//! T008 (DS-US-MCP-009): `normalize` of a commit plan evaluates `commit-msg`, the branch
//! transition and `pre-commit`, in that order. Child module of `guard::plan`, attached there with
//! `#[cfg(test)] #[path = "plan_commit_tests.rs"] mod commit_tests;`.

use super::*;
use gitraptor_api::AgentKind;

const OLD: &str = "1111111111111111111111111111111111111111";
const FEAT: &str = "refs/heads/feat/x";

#[test]
fn a_commit_plan_evaluates_commit_msg_branch_and_pre_commit_in_order() {
    let facts = AuthorshipFacts {
        coauthors: vec![Some(AgentKind::ClaudeCode)],
        trailer_table: 1,
        unreadable: false,
    };
    let paths = vec!["src/a.rs".to_string()];
    let n = normalize(&PlanShape::Commit {
        branch: FEAT.into(),
        old: OLD.into(),
        paths: paths.clone(),
        authorship: facts.clone(),
        header: crate::guard::commit_format::parse(b"feat: x", true, 1),
    });
    assert_eq!(
        n.evaluated,
        [
            Operation::Commit {
                stage: CommitStage::CommitMsg
            },
            tx(FEAT, oid(OLD), oid(OLD)),
            Operation::Commit {
                stage: CommitStage::PreCommit
            },
        ],
        "a commit plan must evaluate commit-msg, the branch old->old and pre-commit last (T008 1.1)"
    );
    assert_eq!(
        n.preset,
        [
            Preset::default(),
            Preset {
                commit: Some(PlannedCommit {
                    header: crate::guard::commit_format::parse(b"feat: x", true, 1),
                    parents: 1,
                    paths: paths.clone(),
                }),
                ..prepared(&paths)
            },
            Preset::default()
        ],
        "the presets must be aligned by index with the evaluated operations"
    );
    assert_eq!(
        n.registered,
        [
            Expected::CommitStages {
                authorship: facts.clone()
            },
            Expected::NewCommitOn {
                refname: FEAT.into(),
                old: OLD.into(),
                paths,
                header: crate::guard::commit_format::parse(b"feat: x", true, 1),
            },
        ],
        "registered must not change"
    );
}
