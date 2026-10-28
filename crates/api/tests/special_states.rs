//! ADR-GRP-016: `operation` and `unmerged_paths` of a worktree travel only
//! when they have a value, `without_special_states` removes them, and the
//! shape round-trips under `deny_unknown_fields` (US-GRP-003, T001).

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, DivergenceView, HeadView, OperationKindView,
    RepoStateView, RepoView, WorktreeStateData, WorktreeStatus, WorktreeView,
};
use serde_json::{Value, json};

fn worktree(operation: Option<OperationKindView>, unmerged_paths: u32) -> WorktreeView {
    WorktreeView {
        path: Untrusted::new("/w/feat"),
        main: false,
        admin_name: None,
        status: WorktreeStatus::Ready {
            head: HeadView::Detached,
            counts: ChangeCounts::default(),
            changes: Vec::new(),
            divergence: DivergenceView::NoCommits,
        },
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation,
        unmerged_paths,
    }
}

fn state(w: WorktreeView) -> WorktreeStateData {
    WorktreeStateData {
        repo_id: "r1".into(),
        worktrees: vec![w],
        fetched_utc_ms: None,
    }
}

fn repo(w: WorktreeView) -> RepoView {
    RepoView {
        repo_id: "r1".into(),
        state: RepoStateView::Observed,
        path: Untrusted::new("/r/.git"),
        base: BaseBranchView {
            name: None,
            status: BaseStatusView::Unconfirmed,
        },
        worktrees: vec![w],
        fetched_utc_ms: None,
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
        prediction: None,
    }
}

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn a_worktree_carries_its_operation_and_unmerged_paths_only_when_present() {
    let with = json_of(&worktree(Some(OperationKindView::Rebase), 2));
    assert_eq!(with["operation"], "rebase");
    assert_eq!(with["unmerged_paths"], 2);
    for (kind, text) in [
        (OperationKindView::Merge, "merge"),
        (OperationKindView::CherryPick, "cherry-pick"),
        (OperationKindView::Revert, "revert"),
        (OperationKindView::Am, "am"),
        (OperationKindView::Bisect, "bisect"),
    ] {
        assert_eq!(json_of(&worktree(Some(kind), 0))["operation"], text);
    }

    let without = json_of(&worktree(None, 0));
    let object = without.as_object().unwrap();
    assert!(!object.contains_key("operation"), "{without}");
    assert!(!object.contains_key("unmerged_paths"), "{without}");
    // Unmerged paths without an operation (a conflicted `stash pop`).
    let only_paths = json_of(&worktree(None, 1));
    assert!(only_paths.get("operation").is_none());
    assert_eq!(only_paths["unmerged_paths"], 1);

    for w in [
        worktree(Some(OperationKindView::Rebase), 2),
        worktree(None, 0),
        worktree(None, 1),
    ] {
        let back: WorktreeView = serde_json::from_value(json_of(&w)).unwrap();
        assert_eq!(back, w);
    }
    // A view from before the capability still reads, with the defaults.
    let mut old = json_of(&worktree(None, 0));
    old.as_object_mut().unwrap().retain(|k, _| k != "operation");
    let back: WorktreeView = serde_json::from_value(old).unwrap();
    assert_eq!(back, worktree(None, 0));
    // The strictness stays: an unknown field is still refused.
    let mut unknown = json_of(&worktree(None, 0));
    unknown["conflicts"] = json!(3);
    assert!(serde_json::from_value::<WorktreeView>(unknown).is_err());
}

#[test]
fn without_special_states_drops_both_fields() {
    let mut w = worktree(Some(OperationKindView::Merge), 3);
    w.without_special_states();
    assert_eq!(w, worktree(None, 0));
    assert!(json_of(&w).get("operation").is_none());

    let mut data = state(worktree(Some(OperationKindView::Rebase), 2));
    data.without_special_states();
    assert_eq!(data, state(worktree(None, 0)));
    assert!(
        json_of(&data)["worktrees"][0]
            .get("unmerged_paths")
            .is_none()
    );

    let mut r = repo(worktree(Some(OperationKindView::Bisect), 1));
    r.without_special_states();
    assert_eq!(r, repo(worktree(None, 0)));
}

#[test]
fn removing_the_special_states_leaves_the_activity_fields_alone() {
    let mut w = worktree(Some(OperationKindView::Am), 1);
    w.detached_at = Some("39e852f".into());
    w.last_activity_utc_ms = Some(5);
    w.without_special_states();
    assert_eq!(w.detached_at.as_deref(), Some("39e852f"));
    assert_eq!(w.last_activity_utc_ms, Some(5));
}
