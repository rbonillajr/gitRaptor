//! C7 of `docs/dev-briefs/tm-prior-point-binding.md` (D3, D4): a complete capture records
//! whether an operation was in progress in its worktrees. Temporary repos only (NFR-01).

mod tm_common;

use gitraptor_core::timemachine::oplog::{CompleteInfo, NewSnapshot, SnapshotLevel};
use gitraptor_testkit::Fixture;
use tm_common::{Env, git};

/// The `mid_operation` of the completed row of `id`.
fn recorded(env: &Env, id: &str) -> Option<bool> {
    env.oplog
        .lock()
        .unwrap()
        .snapshot(id)
        .unwrap()
        .expect("snapshot row")
        .complete
        .expect("complete row")
        .mid_operation
}

/// `main` and `feature` both change `a.txt`; `feature` is checked out.
fn diverged() -> Fixture {
    let f = Fixture::with_commit(&git());
    f.git(&["branch", "feature"]);
    f.write("a.txt", "alpha on main\n");
    f.git(&["commit", "-q", "-am", "main side"]);
    f.git(&["checkout", "-q", "feature"]);
    f.write("a.txt", "alpha on feature\n");
    f.git(&["commit", "-q", "-am", "feature side"]);
    f
}

#[test]
fn an_observation_taken_mid_rebase_records_mid_operation() {
    let f = diverged();
    let out = f
        .git_command(&f.repo, &["rebase", "main"])
        .output()
        .expect("run git rebase");
    assert!(
        !out.status.success(),
        "the rebase must stop on the conflict"
    );
    assert!(
        f.repo.join(".git/rebase-merge").is_dir(),
        "a rebase is in progress"
    );
    let env = Env::new(f);
    let snap = env.capture(SnapshotLevel::Observation, None);
    assert_eq!(recorded(&env, &snap.snapshot_id), Some(true));
}

#[test]
fn an_observation_of_a_clean_worktree_records_clean() {
    let env = Env::new(diverged());
    let snap = env.capture(SnapshotLevel::Observation, None);
    assert_eq!(recorded(&env, &snap.snapshot_id), Some(false));
}

#[test]
fn a_complete_row_without_the_field_reads_as_unrecorded() {
    let env = Env::new(diverged());
    // A row completed the way rows were before the field existed: no `mid_operation`.
    let old = {
        let mut log = env.oplog.lock().unwrap();
        let id = log
            .begin_snapshot(
                &NewSnapshot {
                    level: SnapshotLevel::Observation,
                    worktrees: vec!["main".into()],
                    engine_mark: Some(1),
                    cause_operation: None,
                    cause_event_seq: None,
                },
                1_000,
            )
            .unwrap();
        log.complete_snapshot(&id, &CompleteInfo::default(), 1_000)
            .unwrap();
        id
    };
    assert_eq!(recorded(&env, &old), None);
    // A capture after it does record the field: `None` means "not recorded", not "clean".
    let new = env.capture(SnapshotLevel::Observation, None);
    assert_eq!(recorded(&env, &new.snapshot_id), Some(false));
    assert_eq!(recorded(&env, &old), None);
}
