//! #216 M-01 (NFR-02, ADR-MCP-001): the observer never reads a `.git` the observed repo does not
//! own. A worktree root whose `.git` points at another repo, a linked worktree the repo does not
//! register, a `.git` that is a symlink to outside, or a main worktree that `core.worktree`
//! moved onto another repo's tree reads as unavailable and untrusted, and nothing of the other
//! repo is reported. Temporary repos only (NFR-01).

use std::path::{Path, PathBuf};

use gitraptor_api::messages::{UnavailableReason, WorktreeStatus, WorktreeView};
use gitraptor_core::observe::{all_changes, base_branch, locate, read_worktree, reconcile};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

fn canonical(p: &Path) -> PathBuf {
    gitraptor_core::observe::canonical(p)
}

fn untrusted() -> WorktreeStatus {
    WorktreeStatus::Unavailable {
        reason: UnavailableReason::Untrusted,
    }
}

fn is_ready(w: &WorktreeView) -> bool {
    matches!(w.status, WorktreeStatus::Ready { .. })
}

/// The observed repo with a linked worktree `wt-a`, and another repo (the one an attacker
/// points at) with a linked worktree `wt-x` and an untracked file only it has.
struct Pair {
    ours: Fixture,
    theirs: Fixture,
    common: PathBuf,
    a: PathBuf,
    x: PathBuf,
}

fn pair() -> Pair {
    let git = git_from_path();
    let ours = Fixture::with_commit(&git);
    ours.git(&["branch", "a"]);
    let a = canonical(&ours.add_worktree("a", "a"));
    let theirs = Fixture::with_commit(&git);
    theirs.git(&["branch", "x"]);
    let x = canonical(&theirs.add_worktree("x", "x"));
    std::fs::write(theirs.repo.join("secret.txt"), "theirs\n").unwrap();
    std::fs::write(x.join("secret.txt"), "theirs\n").unwrap();
    let common = locate(&ours.repo).unwrap();
    Pair {
        ours,
        theirs,
        common,
        a,
        x,
    }
}

#[test]
fn a_registered_worktree_and_the_main_one_are_still_read() {
    let p = pair();
    std::fs::write(p.a.join("new.txt"), "n\n").unwrap();
    let main = canonical(&p.ours.repo);

    let read = read_worktree(&p.common, &main, true, None);
    assert!(is_ready(&read.view), "{:?}", read.view.status);
    let read = read_worktree(&p.common, &p.a, false, Some("wt-a"));
    assert!(is_ready(&read.view), "{:?}", read.view.status);
    let (counts, _) = all_changes(&p.common, &p.a, false, Some("wt-a")).unwrap();
    assert_eq!(counts.untracked, 1);

    let read = reconcile(&p.common, &base_branch(None)).unwrap();
    assert!(read.views().iter().all(is_ready), "{:?}", read.views());
}

#[test]
fn a_linked_git_file_rewritten_to_point_at_another_repo_is_not_read() {
    let p = pair();
    // The agent rewrites the link of a registered worktree after it was trusted.
    let theirs_admin = locate(&p.theirs.repo)
        .unwrap()
        .join("worktrees")
        .join("wt-x");
    std::fs::write(
        p.a.join(".git"),
        format!("gitdir: {}\n", theirs_admin.display()),
    )
    .unwrap();

    let read = read_worktree(&p.common, &p.a, false, Some("wt-a"));
    assert_eq!(read.view.status, untrusted());
    assert!(read.head_commit.is_none() && read.fingerprint.is_none());
    assert!(all_changes(&p.common, &p.a, false, Some("wt-a")).is_err());

    let read = reconcile(&p.common, &base_branch(None)).unwrap();
    let a = read
        .views()
        .into_iter()
        .find(|w| Path::new(w.path.raw()) == p.a)
        .unwrap();
    assert_eq!(a.status, untrusted());
}

#[test]
fn a_worktree_the_repo_does_not_register_is_not_read() {
    let p = pair();
    // Another repo's worktree, read as if it were one of ours under a name we never had.
    for id in ["wt-x", "ghost", "../worktrees/wt-a", ".."] {
        let read = read_worktree(&p.common, &p.x, false, Some(id));
        assert_eq!(read.view.status, untrusted(), "id {id}");
        assert!(all_changes(&p.common, &p.x, false, Some(id)).is_err());
    }
    // Another repo's main worktree, read as if it were ours.
    let read = read_worktree(&p.common, &canonical(&p.theirs.repo), true, None);
    assert_eq!(read.view.status, untrusted());
    assert!(all_changes(&p.common, &canonical(&p.theirs.repo), true, None).is_err());
    // A linked worktree with no name at all.
    let read = read_worktree(&p.common, &p.a, false, None);
    assert_eq!(read.view.status, untrusted());
}

#[test]
fn a_main_worktree_moved_onto_another_repo_by_core_worktree_is_not_read() {
    let p = pair();
    let theirs_main = canonical(&p.theirs.repo);
    p.ours
        .git(&["config", "core.worktree", theirs_main.to_str().unwrap()]);

    let read = reconcile(&p.common, &base_branch(None)).unwrap();
    let views = read.views();
    let main = views.iter().find(|w| w.main).unwrap();
    assert_eq!(main.status, untrusted(), "{main:?}");
}

#[cfg(unix)]
#[test]
fn a_git_that_is_a_symlink_to_outside_is_not_read() {
    let p = pair();
    let theirs_common = locate(&p.theirs.repo).unwrap();

    // A linked worktree whose `.git` is a symlink to another repo's Git directory.
    std::fs::remove_file(p.a.join(".git")).unwrap();
    std::os::unix::fs::symlink(&theirs_common, p.a.join(".git")).unwrap();
    let read = read_worktree(&p.common, &p.a, false, Some("wt-a"));
    assert_eq!(read.view.status, untrusted());
    assert!(all_changes(&p.common, &p.a, false, Some("wt-a")).is_err());

    // A main worktree (as `core.worktree` names it) whose `.git` is a symlink to another repo.
    let decoy = p.ours.root.join("decoy");
    std::fs::create_dir_all(&decoy).unwrap();
    std::os::unix::fs::symlink(&theirs_common, decoy.join(".git")).unwrap();
    let decoy = canonical(&decoy);
    let read = read_worktree(&p.common, &decoy, true, None);
    assert_eq!(read.view.status, untrusted());
    assert!(all_changes(&p.common, &decoy, true, None).is_err());

    // Even a symlink to the repo's own Git directory: the main `.git` is a real directory.
    std::fs::remove_file(decoy.join(".git")).unwrap();
    std::os::unix::fs::symlink(&p.common, decoy.join(".git")).unwrap();
    let read = read_worktree(&p.common, &decoy, true, None);
    assert_eq!(read.view.status, untrusted());

    // A root that is itself a symlink to a good worktree is not the folder the repo has.
    let link = p.ours.root.join("link-a");
    std::os::unix::fs::symlink(&p.x, &link).unwrap();
    let read = read_worktree(&p.common, &link, false, Some("wt-a"));
    assert_eq!(read.view.status, untrusted());
}

#[test]
fn a_worktree_that_is_gone_is_still_missing_not_untrusted() {
    let p = pair();
    std::fs::remove_dir_all(&p.a).unwrap();
    let read = read_worktree(&p.common, &p.a, false, Some("wt-a"));
    assert_eq!(
        read.view.status,
        WorktreeStatus::Unavailable {
            reason: UnavailableReason::Missing
        }
    );
}
