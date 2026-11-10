//! TS-CKP-006 T003, review M-1: with `index.skipHash` (as `feature.manyFiles` sets it) the index
//! has no checksum, so `index_diff` cannot tell two reads apart by it alone. The diff must still
//! split staged and unstaged with the right content ids. Temporary repositories only (NFR-01).

mod common;

use std::time::{Duration, Instant};

use common::Fixture;
use gitraptor_git::diff::DiffLimits;
use gitraptor_git::{ReaderOptions, RepoReader};

fn limits() -> DiffLimits {
    DiffLimits {
        max_files: 1_000,
        max_lines_per_file: 2_000,
        max_blob_bytes: 8 * 1024 * 1024,
        max_bytes: 768 * 1024,
        deadline: Instant::now() + Duration::from_secs(60),
    }
}

#[test]
fn ckp006_004_index_diff_holds_with_skip_hash() {
    let f = Fixture::with_commit();
    f.git(&["config", "index.skipHash", "true"]);
    let head_blob = f.git(&["rev-parse", "HEAD:a.txt"]).trim().to_owned();
    f.write("a.txt", "alpha\nstaged\n");
    f.git(&["add", "a.txt"]);
    let index_blob = f.git(&["rev-parse", ":a.txt"]).trim().to_owned();
    f.write("a.txt", "alpha\nstaged\nworktree\n");
    f.write("b.txt", "beta\nunstaged only\n");
    let work_blob = f.git(&["hash-object", "a.txt"]).trim().to_owned();

    // The assumption of M-1: under skipHash the index carries no checksum.
    let index = gix::open(&f.repo).unwrap().index().unwrap();
    assert_eq!(
        index.checksum(),
        None,
        "with index.skipHash the index must have no checksum"
    );

    let d = RepoReader::open(&f.repo, &ReaderOptions::default())
        .unwrap()
        .index_diff(&limits(), &|| false)
        .expect("index_diff must not fail under skipHash");
    let paths = |s: &gitraptor_git::diff::GitIndexSection| -> Vec<String> {
        s.section.files.iter().map(|f| f.path.clone()).collect()
    };
    assert_eq!(paths(&d.staged), vec!["a.txt"]);
    assert_eq!(paths(&d.unstaged), vec!["a.txt", "b.txt"]);
    let staged = d.staged.ids.get("a.txt").expect("staged ids of a.txt");
    assert_eq!(staged.old.as_deref(), Some(head_blob.as_str()));
    assert_eq!(staged.new.as_deref(), Some(index_blob.as_str()));
    let unstaged = d.unstaged.ids.get("a.txt").expect("unstaged ids of a.txt");
    assert_eq!(unstaged.old.as_deref(), Some(index_blob.as_str()));
    assert_eq!(unstaged.new.as_deref(), Some(work_blob.as_str()));
}
