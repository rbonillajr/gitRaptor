//! Repo key (ADR-GRP-006 § 3) and retire/re-add (Q25).

mod common;

use common::*;
use gitraptor_core::profile::{AddOutcome, RepoState};

#[test]
fn two_worktrees_of_one_repo_share_the_key() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let main = init_repo(repos.path(), "repo", true);
    let linked = repos.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            linked.to_str().unwrap(),
        ],
    );

    let mut profile = tp.open();
    let (a, outcome_a) = profile.add_repo(&common_dir(&main), None, 1).unwrap();
    let (b, outcome_b) = profile.add_repo(&common_dir(&linked), None, 2).unwrap();
    assert_eq!(outcome_a, AddOutcome::New);
    assert_eq!(outcome_b, AddOutcome::AlreadyObserved);
    assert_eq!(a.repo_id, b.repo_id);
    assert_eq!(profile.repos().unwrap().len(), 1);
}

#[test]
fn two_clones_of_one_project_get_different_keys() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let origin = init_repo(repos.path(), "origin", true);
    let clone = repos.path().join("clone");
    git(
        repos.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let root = git(&origin, &["rev-list", "--max-parents=0", "HEAD"]);

    let mut profile = tp.open();
    let (a, _) = profile
        .add_repo(&common_dir(&origin), Some(&root), 1)
        .unwrap();
    let (b, outcome) = profile
        .add_repo(&common_dir(&clone), Some(&root), 2)
        .unwrap();
    assert_eq!(outcome, AddOutcome::New);
    assert_ne!(
        a.repo_id, b.repo_id,
        "same root commit must not merge clones"
    );
    assert_eq!(a.root_commit_hint.as_deref(), Some(root.as_str()));
}

#[test]
fn repo_without_commits_can_be_added() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let empty = init_repo(repos.path(), "empty", false);

    let mut profile = tp.open();
    let (entry, outcome) = profile.add_repo(&common_dir(&empty), None, 1).unwrap();
    assert_eq!(outcome, AddOutcome::New);
    assert_eq!(entry.root_commit_hint, None);
    let (_store, _) = profile.open_store(&entry.repo_id).unwrap();
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn other_case_spelling_gives_the_same_key() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "CaseRepo", true);
    let common = common_dir(&repo);
    let respelled =
        std::path::PathBuf::from(common.to_str().unwrap().replace("CaseRepo", "caserepo"));
    assert!(respelled.exists(), "case-insensitive file system expected");

    let mut profile = tp.open();
    let (a, _) = profile.add_repo(&common, None, 1).unwrap();
    let (b, outcome) = profile.add_repo(&respelled, None, 2).unwrap();
    assert_eq!(outcome, AddOutcome::AlreadyObserved);
    assert_eq!(a.repo_id, b.repo_id);
}

#[test]
fn retire_and_readd_recovers_key_and_data() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let common = common_dir(&repo);

    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common, None, 10).unwrap();
    {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store.write_batch(&sample_batch(&repo, "s1", 2)).unwrap();
    }
    profile.retire_repo(&entry.repo_id, 20).unwrap();
    let retired = profile.repo(&entry.repo_id).unwrap().unwrap();
    assert_eq!(retired.state, RepoState::Retired);
    assert_eq!(retired.retired_ms, Some(20));
    drop(profile);

    // Survives a restart of the engine too.
    let mut profile = tp.open();
    let (again, outcome) = profile.add_repo(&common, None, 30).unwrap();
    assert_eq!(outcome, AddOutcome::Reactivated { retired_ms: 20 });
    assert_eq!(again.repo_id, entry.repo_id);
    assert_eq!(again.state, RepoState::Observed);
    let (store, _) = profile.open_store(&again.repo_id).unwrap();
    assert_eq!(store.events_for_session("s1").unwrap().len(), 2);
}
