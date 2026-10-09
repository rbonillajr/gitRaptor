use std::sync::mpsc::channel;

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::RequesterOrigin;
use crate::timemachine::store::snapshot_refs;

const T0: i64 = 1_728_000_000_000;
const WAIT: Duration = Duration::from_secs(20);
const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

fn write_sparse(path: &Path, len: u64) {
    let file = std::fs::File::create(path).unwrap();
    file.set_len(len).unwrap();
}

fn deadline() -> Instant {
    Instant::now() + WAIT
}

#[test]
fn a_small_worktree_reserves_the_minimum() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    assert_eq!(reserve_for(tmp.path(), deadline(), None), MIN_RESERVE_BYTES);
}

#[test]
fn the_git_folder_is_not_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let git = tmp.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    write_sparse(&git.join("pack"), 3 * MIN_RESERVE_BYTES);
    assert_eq!(reserve_for(tmp.path(), deadline(), None), MIN_RESERVE_BYTES);
}

#[test]
fn a_large_worktree_reserves_its_size() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    let each = MIN_RESERVE_BYTES / 2 + 1;
    write_sparse(&tmp.path().join("one.bin"), each);
    write_sparse(&tmp.path().join("sub").join("two.bin"), each);
    assert_eq!(reserve_for(tmp.path(), deadline(), None), 2 * each);
}

#[test]
fn a_walk_cut_by_its_deadline_keeps_the_last_estimate() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..300 {
        std::fs::write(tmp.path().join(format!("f{i}")), b"x").unwrap();
    }
    let past = Instant::now();
    let last = 5 * MIN_RESERVE_BYTES;
    assert_eq!(reserve_for(tmp.path(), past, Some(last)), last);
    assert_eq!(reserve_for(tmp.path(), past, None), MIN_RESERVE_BYTES);
}

#[test]
fn the_reserve_of_a_worktree_is_measured_once_per_ttl() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (store, _) = SnapshotStore::open_or_create(&dirs, REPO).unwrap();
    let tree = tmp.path().join("tree");
    std::fs::create_dir(&tree).unwrap();

    let first = cached_reserve(&store, "k", &tree, deadline());
    // Grows after the measurement: the cached answer does not see it.
    write_sparse(&tree.join("big.bin"), 4 * MIN_RESERVE_BYTES);
    let second = cached_reserve(&store, "k", &tree, deadline());
    assert_eq!(first, MIN_RESERVE_BYTES);
    assert_eq!(second, first, "served from the cache, not walked again");

    let other = cached_reserve(&store, "another", &tree, deadline());
    assert_eq!(other, 4 * MIN_RESERVE_BYTES, "another worktree walks");
}

/// A capture that cannot get the recording lock in time leaves no row and no ref.
#[test]
fn a_time_limit_leaves_no_row_nor_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (store, _) = SnapshotStore::open_or_create(&dirs, REPO).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let oplog = Mutex::new(oplog);
    let tree = tmp.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    let requester = Requester::Agent {
        name: "claude".into(),
        origin: RequesterOrigin::Detected,
        session_id: "s1".into(),
    };
    let ask = ManualAsk {
        repo_id: REPO.into(),
        worktree: tree.clone(),
        common_dir: crate::observe::locate(&tree).unwrap(),
        label: "before".into(),
        requester,
        channel: Channel::Mcp,
    };
    let refs_before = snapshot_refs(&store).unwrap();

    let (held_tx, held_rx) = channel();
    let (release_tx, release_rx) = channel::<()>();
    let store_ref = &store;
    std::thread::scope(|s| {
        s.spawn(move || {
            let _recording = store_ref
                .manual()
                .lock_recording(deadline())
                .expect("the lock is free");
            held_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(WAIT);
        });
        held_rx.recv_timeout(WAIT).expect("the lock is held");

        let result = capture_in_store(&store, &oplog, &ask, None, false, None, T0, Instant::now());
        assert!(matches!(result, Err(ManualError::TimeLimit)), "{result:?}");
        release_tx.send(()).unwrap();
    });

    let (_, key) = resolve_worktree(&tree).unwrap();
    let input = oplog
        .lock()
        .unwrap()
        .manual_quota_input("s1", &key, T0)
        .unwrap();
    assert!(input.requester_ms.is_empty(), "no manual row");
    assert_eq!(snapshot_refs(&store).unwrap(), refs_before, "no ref");
}
