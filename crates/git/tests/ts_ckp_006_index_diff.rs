//! TS-CKP-006 T003 (CKP006-004): the diff of the index, split in staged and unstaged, with the
//! content ids of each side and the conflicts. Temporary repositories only (NFR-01).

mod common;

use std::time::{Duration, Instant};

use common::Fixture;
use gitraptor_git::diff::{DiffLimits, GitIndexDiff, GitIndexSection};
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

fn index_diff(f: &Fixture) -> GitIndexDiff {
    RepoReader::open(&f.repo, &ReaderOptions::default())
        .unwrap()
        .index_diff(&limits(), &|| false)
        .expect("index_diff must not fail")
}

fn paths(s: &GitIndexSection) -> Vec<String> {
    s.section.files.iter().map(|f| f.path.clone()).collect()
}

fn id(f: &Fixture, spec: &str) -> String {
    f.git(&["rev-parse", spec]).trim().to_owned()
}

#[test]
fn ckp006_004_staged_half_staged_and_untracked_are_separated() {
    let f = Fixture::with_commit();
    f.write("a.txt", "alpha\nstaged\n");
    f.git(&["add", "a.txt"]);
    f.write("a.txt", "alpha\nstaged\nand more\n"); // half staged
    f.write("b.txt", "beta\nunstaged only\n");
    f.write("c.txt", "untracked\n");
    let d = index_diff(&f);
    assert_eq!(
        paths(&d.staged),
        vec!["a.txt"],
        "staged is HEAD against the index"
    );
    assert_eq!(
        paths(&d.unstaged),
        vec!["a.txt", "b.txt", "c.txt"],
        "unstaged is the index against the working tree, untracked included"
    );
    assert!(d.conflicts.is_empty() && d.conflicts_total == 0);
}

#[test]
fn ckp006_004_a_conflict_is_in_conflicts_and_out_of_both_sections() {
    let f = Fixture::with_commit();
    f.git(&["checkout", "-q", "-b", "feat"]);
    f.write("a.txt", "feature side\n");
    f.git(&["commit", "-q", "-am", "feature"]);
    f.git(&["checkout", "-q", "main"]);
    f.write("a.txt", "main side\n");
    f.git(&["commit", "-q", "-am", "main"]);
    let merge = f.git_command(&f.repo, &["merge", "feat"]).output().unwrap();
    assert!(!merge.status.success(), "the merge must conflict");
    f.write("b.txt", "beta\nunrelated change\n");
    let d = index_diff(&f);
    assert_eq!(
        d.conflicts,
        vec!["a.txt"],
        "a path with stages 1 to 3 is a conflict"
    );
    assert_eq!(d.conflicts_total, 1);
    for (name, s) in [("staged", &d.staged), ("unstaged", &d.unstaged)] {
        assert!(
            !paths(s).contains(&"a.txt".to_owned()),
            "the conflicted path must not be in {name}: {:?}",
            paths(s)
        );
    }
    assert!(paths(&d.unstaged).contains(&"b.txt".to_owned()));
}

#[test]
fn ckp006_004_content_ids_come_from_the_index_head_and_worktree() {
    let f = Fixture::with_commit();
    let head_blob = id(&f, "HEAD:a.txt");
    f.write("a.txt", "alpha\nstaged\n");
    f.git(&["add", "a.txt"]);
    let index_blob = id(&f, ":a.txt");
    f.write("a.txt", "alpha\nstaged\nworktree\n");
    f.write("c.txt", "untracked\n");
    let work_blob = f.git(&["hash-object", "a.txt"]).trim().to_owned();
    let untracked_blob = f.git(&["hash-object", "c.txt"]).trim().to_owned();
    let d = index_diff(&f);
    let staged = d.staged.ids.get("a.txt").expect("staged ids of a.txt");
    assert_eq!(
        staged.old.as_deref(),
        Some(head_blob.as_str()),
        "staged old is HEAD's blob"
    );
    assert_eq!(
        staged.new.as_deref(),
        Some(index_blob.as_str()),
        "staged new is the index blob"
    );
    let unstaged = d.unstaged.ids.get("a.txt").expect("unstaged ids of a.txt");
    assert_eq!(
        unstaged.old.as_deref(),
        Some(index_blob.as_str()),
        "unstaged old is the index blob"
    );
    assert_eq!(
        unstaged.new.as_deref(),
        Some(work_blob.as_str()),
        "unstaged new is the worktree hash"
    );
    let untracked = d.unstaged.ids.get("c.txt").expect("unstaged ids of c.txt");
    assert_eq!(untracked.old, None, "an untracked file has no old side");
    assert_eq!(untracked.new.as_deref(), Some(untracked_blob.as_str()));
}

#[test]
fn ckp006_004_worktree_id_is_the_raw_blob_hash() {
    let f = Fixture::with_commit();
    let marker = f.root().join("clean-ran");
    f.write(".gitattributes", "*.txt filter=up\n");
    f.git(&["add", ".gitattributes"]);
    f.git(&["commit", "-q", "-m", "attributes"]);
    f.git(&[
        "config",
        "filter.up.clean",
        &format!("touch '{}'; tr a-z A-Z", marker.display()),
    ]);
    f.write("c.txt", "lower case\n");
    let raw = f
        .git(&["hash-object", "--no-filters", "c.txt"])
        .trim()
        .to_owned();
    let d = index_diff(&f);
    let c = d.unstaged.ids.get("c.txt").expect("unstaged ids of c.txt");
    assert_eq!(
        c.new.as_deref(),
        Some(raw.as_str()),
        "the worktree id is the hash of the raw bytes, with no clean filter"
    );
    assert!(!marker.exists(), "the clean filter ran");
}
