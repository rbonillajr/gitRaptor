//! US-GRP-001: the full reconciliation of an observed repo's worktrees, on
//! temporary repos of the "intact repo" harness (NFR-01). Each read leaves
//! the repo and its worktrees as they were (BR-CONS-001).

use std::path::Path;

use gitraptor_api::messages::{
    ChangeCounts, HeadView, RepoRejection, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_core::observe::{base_branch, locate, reconcile};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

fn head(w: &WorktreeView) -> &HeadView {
    match &w.status {
        WorktreeStatus::Ready { head, .. } => head,
        other => panic!("not ready: {other:?}"),
    }
}

fn counts(w: &WorktreeView) -> ChangeCounts {
    match &w.status {
        WorktreeStatus::Ready { counts, .. } => *counts,
        other => panic!("not ready: {other:?}"),
    }
}

fn canonical(p: &Path) -> String {
    p.canonicalize().unwrap().to_str().unwrap().to_owned()
}

#[test]
fn repo_intact_every_worktree_is_read_and_one_that_is_gone_is_unavailable() {
    let f = Fixture::with_commit(&git_from_path());
    f.git(&["branch", "a"]);
    f.git(&["branch", "b"]);
    let a = f.add_worktree("a", "a");
    let b = f.add_worktree("b", "b");
    std::fs::write(a.join("new.txt"), "n\n").unwrap();
    std::fs::remove_dir_all(&b).unwrap();

    let report = check("reconcile", &f, &Exceptions::none(), || {
        // Any worktree locates the same common directory.
        let common = locate(&a).unwrap();
        assert_eq!(locate(&f.repo).unwrap(), common);
        let read = reconcile(&common, &base_branch(None)).unwrap();
        let wts = read.views();
        assert_eq!(wts.len(), 3);
        assert!(wts[0].main);
        assert_eq!(wts[0].path.raw(), canonical(&f.repo));
        assert!(counts(&wts[0]).is_clean());
        assert_eq!(wts[1].path.raw(), canonical(&a));
        assert_eq!(wts[1].admin_name.as_ref().unwrap().raw(), "wt-a");
        assert_eq!(counts(&wts[1]).untracked, 1);
        // The folder of "b" is gone: only that one is unavailable.
        assert_eq!(
            wts[2].status,
            WorktreeStatus::Unavailable {
                reason: UnavailableReason::Missing
            }
        );
        assert!(read.worktrees[0].fingerprint.is_some());
        assert_ne!(read.worktrees[0].fingerprint, read.worktrees[1].fingerprint);
    });
    report.assert_intact();
}

#[test]
fn detached_and_unborn_heads_are_reported_as_such() {
    let f = Fixture::with_commit(&git_from_path());
    let head_commit = f.git(&["rev-parse", "HEAD"]);
    f.git(&["checkout", "-q", "--detach", head_commit.trim()]);
    let read = reconcile(&locate(&f.repo).unwrap(), &base_branch(None)).unwrap();
    assert_eq!(head(&read.views()[0]), &HeadView::Detached);

    let empty = Fixture::new(&git_from_path());
    let read = reconcile(&locate(&empty.repo).unwrap(), &base_branch(None)).unwrap();
    match head(&read.views()[0]) {
        HeadView::Unborn { name } => assert_eq!(name.raw(), "main"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bare_repo_has_no_main_worktree_but_its_linked_ones() {
    let f = Fixture::with_commit(&git_from_path());
    let bare = f.root.join("bare.git");
    f.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    let wt = f.root.join("wt-bare");
    f.git_in(
        &bare,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "main"],
    );
    let read = reconcile(&locate(&bare).unwrap(), &base_branch(None)).unwrap();
    let wts = read.views();
    assert_eq!(wts.len(), 1);
    assert!(!wts[0].main);
    assert_eq!(wts[0].path.raw(), canonical(&wt));
}

#[test]
fn a_folder_that_is_not_a_repo_is_rejected_as_such() {
    let f = Fixture::new(&git_from_path());
    let notas = f.root.join("notas");
    std::fs::create_dir(&notas).unwrap();
    assert_eq!(locate(&notas), Err(RepoRejection::NotARepo));
    assert_eq!(
        locate(&f.root.join("missing")),
        Err(RepoRejection::NotARepo)
    );
}
