//! The operation's own part of `snapshot`: a manual recovery point of the worktree. It writes
//! nothing in the repo, so the common preconditions let an operation in progress through; this
//! part refuses it.

use gitraptor_api::catalog::RejectReason;

use crate::executor::{Affected, OpPlan, PlanError, RepoFacts};

/// A detached HEAD is allowed; a Git operation in progress is not.
pub(super) fn plan_op(facts: &RepoFacts) -> Result<OpPlan, PlanError> {
    if facts.in_progress.is_some() {
        return Err(PlanError::Rejected(RejectReason::OperationInProgress));
    }
    Ok(OpPlan {
        expected: serde_json::json!({}),
        warnings: Vec::new(),
        affected: Affected::Nobody,
        other_session: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(in_progress: Option<&str>, detached: bool) -> RepoFacts {
        RepoFacts {
            root: "/w/a".into(),
            git_dir: "/w/a/.git".into(),
            common_dir: "/w/a/.git".into(),
            linked: false,
            root_id: Some((1, 2)),
            dot_git_id: Some((1, 3)),
            gitdir_linked_back: true,
            head_branch: None,
            head_commit: None,
            detached,
            in_progress: in_progress.map(str::to_owned),
            git_locks: Vec::new(),
            locked: false,
            grafts: false,
            branches_elsewhere: Vec::new(),
        }
    }

    #[test]
    fn snapshot_plan_refuses_an_operation_in_progress() {
        assert_eq!(
            plan_op(&facts(Some("Rebase"), false)),
            Err(PlanError::Rejected(RejectReason::OperationInProgress))
        );
        assert!(
            plan_op(&facts(None, true)).is_ok(),
            "detached HEAD is allowed"
        );
    }
}
