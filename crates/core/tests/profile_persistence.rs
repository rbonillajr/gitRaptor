//! US-GRP-004: after the writer is killed (SIGKILL), every committed batch
//! is still there. The test re-runs this test binary as a child writer.

mod common;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use common::*;
use gitraptor_core::profile::{Profile, ProfileDirs, StoreOpen};

const CHILD_ENV: &str = "GITRAPTOR_TEST_WRITER_ROOT";
const REPO_ENV: &str = "GITRAPTOR_TEST_WRITER_REPO";

/// Child mode: writes batches forever, printing each committed batch.
/// Does nothing when run as a normal test.
#[test]
fn writer_child() {
    let (Some(root), Some(repo)) = (std::env::var_os(CHILD_ENV), std::env::var_os(REPO_ENV)) else {
        return;
    };
    let repo = PathBuf::from(repo);
    let (mut profile, _) = Profile::open(ProfileDirs::under_root(root)).unwrap();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    store.write_batch(&sample_batch(&repo, "s1", 0)).unwrap();
    // Runs until the parent kills it.
    loop {
        let seqs = store
            .write_batch(&[event(&repo, Some("s1"), "{}")])
            .unwrap()
            .seqs;
        println!("committed {}", seqs[0]);
    }
}

#[test]
fn committed_batches_survive_sigkill() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let root = tp.root.path().join("profile");

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["writer_child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, &root)
        .env(REPO_ENV, &repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut last_committed = 0;
    while last_committed < 200 {
        let line = lines.next().expect("child exited early").unwrap();
        if let Some((_, seq)) = line.split_once("committed ") {
            last_committed = seq.parse().unwrap();
        }
    }
    child.kill().unwrap(); // SIGKILL on Unix, TerminateProcess on Windows
    child.wait().unwrap();

    let profile = Profile::open(ProfileDirs::under_root(&root)).unwrap().0;
    let entry = profile
        .repo_by_common_dir(&common_dir(&repo))
        .unwrap()
        .unwrap();
    let (store, status) = profile.open_store(&entry.repo_id).unwrap();
    assert_eq!(
        status,
        StoreOpen::Existing,
        "a killed writer does not corrupt the store"
    );
    let events = store.events_for_session("s1").unwrap();
    assert!(
        events.len() as i64 >= last_committed,
        "{} events, but {last_committed} were reported committed",
        events.len()
    );
    let seqs: Vec<i64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(
        seqs,
        (1..=seqs.len() as i64).collect::<Vec<_>>(),
        "no holes"
    );
}
