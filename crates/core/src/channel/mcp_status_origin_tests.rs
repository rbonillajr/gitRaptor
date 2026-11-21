//! The page of worktrees carries the origin of each linked worktree, as the view has it.

use gitraptor_api::guard::{GuardStatus, Permission, ProtectionState};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, DivergenceView, HeadView, RepoStateView,
    RepoView, WorktreeStatus, WorktreeView,
};
use gitraptor_api::worktree_origin::{
    HandsOffReasonView, OriginEvidenceView, OriginToolView, WorktreeOriginView,
};
use gitraptor_api::{Untrusted, UntrustedName};

use super::*;

fn view(root: &str, main: bool, origin: Option<WorktreeOriginView>) -> WorktreeView {
    WorktreeView {
        path: Untrusted::new(root),
        main,
        admin_name: (!main).then(|| UntrustedName::new("w")),
        status: WorktreeStatus::Ready {
            head: HeadView::Detached,
            counts: ChangeCounts::default(),
            changes: Vec::new(),
            divergence: DivergenceView::NoBase,
        },
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
        origin,
    }
}

fn repo(worktrees: Vec<WorktreeView>) -> RepoView {
    RepoView {
        repo_id: "r1".to_owned(),
        state: RepoStateView::Observed,
        path: Untrusted::new("/t/shop.git"),
        base: BaseBranchView {
            name: Some(UntrustedName::new("main")),
            status: BaseStatusView::Confirmed,
        },
        worktrees,
        fetched_utc_ms: None,
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
        prediction: None,
    }
}

fn context() -> McpContext {
    McpContext {
        detection_available: true,
        sessions: Vec::new(),
        gaps: Vec::new(),
        guard: GuardStatus {
            repo_id: "r1".to_owned(),
            state: ProtectionState::Unprotected,
            permission: Permission::NotAsked,
            offer: false,
            protected_bases: Vec::new(),
            base_confirmed: true,
            not_preventable: Vec::new(),
            last_refusal: Vec::new(),
            misnamed_settings: Vec::new(),
            pending: None,
            hooks: None,
            diagnostics: Vec::new(),
            minimum_set: None,
            confirm: None,
        },
    }
}

#[test]
fn the_page_copies_the_origin_and_the_mark_of_each_worktree() {
    let codex = WorktreeOriginView::new(
        OriginToolView::Codex,
        OriginEvidenceView::PathPattern,
        Some(HandsOffReasonView::Detached),
    )
    .unwrap();
    let repo = repo(vec![
        view("/t/shop", true, None),
        view("/t/a", false, Some(codex.clone())),
        view("/t/b", false, None),
    ]);
    let page = worktrees_page(&repo, 0, &context(), None);
    assert_eq!(page.worktrees.len(), 2);
    assert_eq!(page.worktrees[0].origin, Some(codex));
    assert_eq!(page.worktrees[1].origin, None);
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(
        json["worktrees"][0]["origin"],
        serde_json::json!({"tool": "codex", "evidence": "path-pattern", "hands_off": "detached"})
    );
    assert!(json["worktrees"][1].get("origin").is_none());
}
