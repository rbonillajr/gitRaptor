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
    // The engine's own canonical form (the drive form on Windows).
    gitraptor_core::observe::canonical(p)
        .to_str()
        .unwrap()
        .to_owned()
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
    // US-CKP-001: the commit it is at, so a worktree without a branch can be told apart.
    assert_eq!(
        read.views()[0].detached_at.as_deref(),
        Some(head_commit.trim())
    );

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

/// SEC-11 (US-GRP-002, D13): a linked worktree whose `.git` does not point
/// back to its `gitdir` is neither read nor watched: it is reported
/// untrusted. A root at `/` or at an ancestor of the repo never is.
#[test]
fn a_linked_worktree_without_its_back_link_is_untrusted() {
    let f = Fixture::with_commit(&git_from_path());
    f.git(&["branch", "a"]);
    f.git(&["branch", "b"]);
    let a = f.add_worktree("a", "a");
    let b = f.add_worktree("b", "b");
    // "b" now claims to be "a"'s worktree.
    let common = locate(&f.repo).unwrap();
    std::fs::write(
        b.join(".git"),
        format!("gitdir: {}\n", common.join("worktrees/wt-a").display()),
    )
    .unwrap();

    let read = reconcile(&common, &base_branch(None)).unwrap();
    let wts = read.views();
    let by_path = |p: &Path| {
        wts.iter()
            .find(|w| w.path.raw() == canonical(p))
            .unwrap()
            .status
            .clone()
    };
    assert!(matches!(by_path(&a), WorktreeStatus::Ready { .. }));
    assert_eq!(
        by_path(&b),
        WorktreeStatus::Unavailable {
            reason: UnavailableReason::Untrusted
        }
    );
    use gitraptor_core::observe::linked_is_trusted;
    assert!(linked_is_trusted(
        &common,
        "wt-a",
        Path::new(&canonical(&a))
    ));
    assert!(!linked_is_trusted(
        &common,
        "wt-b",
        Path::new(&canonical(&b))
    ));
    assert!(!linked_is_trusted(&common, "wt-a", Path::new("/")));
    assert!(!linked_is_trusted(&common, "wt-a", &f.root));
}

mod activity {
    //! DEP-CKP-4 (ADR-GRP-013, amendment 2026-10-04): what counts as activity of a worktree,
    //! and the age of the local copy of the remote.

    use gitraptor_api::messages::{
        ChangeCounts, CommitCountView, DivergenceView, HeadView, WorktreeStatus, WorktreeView,
    };
    use gitraptor_api::{Untrusted, UntrustedName};
    use gitraptor_core::observe::{HeadRef, fetched_utc_ms, stamp_activity};

    const NOW: i64 = 1_000_000;

    fn view(path: &str, unstaged: u32, behind: u64, last: Option<i64>) -> WorktreeView {
        let count = |count| CommitCountView { count, exact: true };
        WorktreeView {
            path: Untrusted::new(path),
            main: false,
            admin_name: None,
            status: WorktreeStatus::Ready {
                head: HeadView::Branch {
                    name: UntrustedName::new("feat"),
                },
                counts: ChangeCounts {
                    staged: 0,
                    unstaged,
                    untracked: 0,
                },
                changes: Vec::new(),
                divergence: DivergenceView::Counted {
                    ahead: count(0),
                    behind: count(behind),
                },
            },
            last_activity_utc_ms: last,
            detached_at: None,
        }
    }

    fn at(commit: &str) -> HeadRef {
        HeadRef::Branch {
            name: "feat".into(),
            commit: commit.into(),
        }
    }

    fn stamped(
        old: &[WorktreeView],
        old_heads: &[HeadRef],
        new: WorktreeView,
        head: HeadRef,
    ) -> Option<i64> {
        let mut new = vec![new];
        stamp_activity(old, old_heads, &mut new, &[head], NOW);
        new[0].last_activity_utc_ms
    }

    #[test]
    fn a_change_in_the_worktree_is_activity() {
        let old = [view("/w/a", 0, 0, Some(5))];
        assert_eq!(
            stamped(&old, &[at("c1")], view("/w/a", 1, 0, None), at("c1")),
            Some(NOW)
        );
    }

    /// A commit leaves the counts as they were (clean before and after) but moves the head.
    #[test]
    fn a_commit_is_activity() {
        let old = [view("/w/a", 0, 0, Some(5))];
        assert_eq!(
            stamped(&old, &[at("c1")], view("/w/a", 0, 0, None), at("c2")),
            Some(NOW)
        );
    }

    /// A fetch moves "behind" with nobody touching the worktree: the old activity stays.
    #[test]
    fn a_new_ahead_behind_alone_is_not_activity() {
        let old = [view("/w/a", 0, 0, Some(5))];
        assert_eq!(
            stamped(&old, &[at("c1")], view("/w/a", 0, 3, None), at("c1")),
            Some(5)
        );
    }

    #[test]
    fn the_first_read_knows_no_activity_and_a_new_worktree_is_activity() {
        assert_eq!(stamped(&[], &[], view("/w/a", 0, 0, None), at("c1")), None);
        let old = [view("/w/a", 0, 0, Some(5))];
        assert_eq!(
            stamped(&old, &[at("c1")], view("/w/b", 0, 0, None), at("c1")),
            Some(NOW)
        );
    }

    #[test]
    fn the_last_fetch_is_the_time_of_fetch_head_never_in_the_future() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(fetched_utc_ms(tmp.path(), NOW), None);
        std::fs::write(tmp.path().join("FETCH_HEAD"), "").unwrap();
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let fetched = fetched_utc_ms(tmp.path(), i64::MAX).unwrap();
        assert!((fetched - now).abs() < 60_000, "{fetched} vs {now}");
        // A clock moved back: the fetch is not shown in the future.
        assert_eq!(fetched_utc_ms(tmp.path(), NOW), Some(NOW));
    }
}
