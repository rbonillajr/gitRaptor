//! Confirmed base branch and floor in the per-repo store (TS-GRD-001; ADR-GRD-004 § 3–4).

mod common;

use common::*;
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};

#[test]
fn nothing_is_confirmed_until_written_and_it_survives_reopening() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    // Adding the repo confirms nothing (D9).
    assert_eq!(store.confirmed_team_baseline().unwrap(), None);

    let install = Confirmed {
        base_branch: RefName::new("main").unwrap(),
        floor: ConfirmedFloor::Absent,
    };
    store.set_confirmed_team_baseline(&install).unwrap();
    assert_eq!(store.confirmed_team_baseline().unwrap(), Some(install));

    let explicit = Confirmed {
        base_branch: RefName::new("develop").unwrap(),
        floor: ConfirmedFloor::Blob("ab".repeat(20)),
    };
    store.set_confirmed_team_baseline(&explicit).unwrap();
    drop(store);
    drop(profile);

    let profile = tp.open();
    let (store, _) = profile.open_store(&entry.repo_id).unwrap();
    assert_eq!(store.confirmed_team_baseline().unwrap(), Some(explicit));
}

#[test]
fn an_invalid_floor_id_is_rejected() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let bad = Confirmed {
        base_branch: RefName::new("main").unwrap(),
        floor: ConfirmedFloor::Blob("not-an-id".into()),
    };
    assert!(store.set_confirmed_team_baseline(&bad).is_err());
    assert_eq!(store.confirmed_team_baseline().unwrap(), None);
}
