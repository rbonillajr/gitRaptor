//! TMC011-TREE-DIFF (T004 of DS-US-TMC-011): `RepoReader::commit_tree_diff` returns the entries
//! (path, before, after) of the tree difference between two commits, uncut. Child module of
//! `changed_paths` (`use super::*`), mounted there as `mod tree_diff_tests`.

use std::time::Duration;

use gitraptor_testkit::Fixture;

use super::*;
use crate::tm_write::files::Kind;
use crate::{Oid, ReaderOptions};

fn fixture() -> Fixture {
    Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path())
}

fn head(f: &Fixture) -> String {
    f.git(&["rev-parse", "HEAD"]).trim().to_owned()
}

fn commit(f: &Fixture, message: &str) -> String {
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", message]);
    head(f)
}

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

fn oid_at(f: &Fixture, commit: &str, path: &str) -> Oid {
    Oid::from_hex(f.git(&["rev-parse", &format!("{commit}:{path}")]).trim()).unwrap()
}

#[test]
fn commit_tree_diff_returns_entries_uncut() {
    let f = fixture();
    f.write("keep.rs", "same\n");
    f.write("edit.rs", "one\n");
    f.write("gone.rs", "bye\n");
    f.write("dir/inner.rs", "x\n");
    let a = commit(&f, "a");
    f.write("edit.rs", "two\n");
    f.write("new/deep/file.rs", "fresh\n");
    f.write("tool.sh", "#!/bin/sh\n");
    f.git(&["update-index", "--add", "--chmod=+x", "tool.sh"]);
    f.git(&["rm", "-q", "gone.rs"]);
    let b = commit(&f, "b");

    let mut got = reader(&f)
        .commit_tree_diff(&a, &b, soon())
        .expect("TMC011-TREE-DIFF: commit_tree_diff reads the difference of two commits");
    got.sort_by(|x, y| x.path.cmp(&y.path));

    let paths: Vec<String> = got
        .iter()
        .map(|c| String::from_utf8(c.path.clone()).unwrap())
        .collect();
    assert_eq!(
        paths,
        ["edit.rs", "gone.rs", "new/deep/file.rs", "tool.sh"],
        "TMC011-TREE-DIFF: exactly the paths that differ, whole paths, unchanged ones absent"
    );
    let by = |p: &str| got.iter().find(|c| c.path == p.as_bytes()).unwrap();

    let edit = by("edit.rs");
    assert_eq!(
        edit.old,
        Some((Kind::File, oid_at(&f, &a, "edit.rs"))),
        "TMC011-TREE-DIFF: the before side of a modified file is its entry in old"
    );
    assert_eq!(edit.new, Some((Kind::File, oid_at(&f, &b, "edit.rs"))));
    let gone = by("gone.rs");
    assert_eq!(gone.old, Some((Kind::File, oid_at(&f, &a, "gone.rs"))));
    assert_eq!(
        gone.new, None,
        "TMC011-TREE-DIFF: a deleted path has no after side"
    );
    let fresh = by("new/deep/file.rs");
    assert_eq!(
        fresh.old, None,
        "TMC011-TREE-DIFF: an added path has no before side"
    );
    assert_eq!(
        fresh.new,
        Some((Kind::File, oid_at(&f, &b, "new/deep/file.rs")))
    );
    let tool = by("tool.sh");
    assert_eq!(
        tool.new,
        Some((Kind::Executable, oid_at(&f, &b, "tool.sh"))),
        "TMC011-TREE-DIFF: the kind of the entry travels with it"
    );
}

#[test]
fn commit_tree_diff_returns_entries_uncut_beyond_the_display_limits() {
    let f = fixture();
    let a = head(&f);
    // More paths than any display cap, and one longer than MAX_PATH_BYTES, written with
    // plumbing so the path never has to exist on the file system.
    f.write("blob.tmp", "payload\n");
    let blob = f.git(&["hash-object", "-w", "blob.tmp"]).trim().to_owned();
    std::fs::remove_file(f.repo.join("blob.tmp")).unwrap();
    let component = "d".repeat(200);
    let long =
        format!("{component}/{component}/{component}/{component}/{component}/{component}/end.rs");
    assert!(
        long.len() > MAX_PATH_BYTES,
        "the fixture path is longer than the display cut"
    );
    let mut expected = vec![long.clone()];
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{blob},{long}"),
    ]);
    for n in 0..300 {
        let path = format!("many/f{n:03}.rs");
        f.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},{path}"),
        ]);
        expected.push(path);
    }
    let tree = f.git(&["write-tree"]).trim().to_owned();
    let b = f
        .git(&["commit-tree", &tree, "-p", &a, "-m", "many"])
        .trim()
        .to_owned();

    let got = reader(&f)
        .commit_tree_diff(&a, &b, soon())
        .expect("TMC011-TREE-DIFF: commit_tree_diff reads the difference");

    assert_eq!(
        got.len(),
        expected.len(),
        "TMC011-TREE-DIFF: no cap on the number of paths"
    );
    let mut paths: Vec<String> = got
        .iter()
        .map(|c| String::from_utf8(c.path.clone()).unwrap())
        .collect();
    paths.sort();
    expected.sort();
    assert_eq!(
        paths, expected,
        "TMC011-TREE-DIFF: no path is cut (the long one keeps all its bytes, no cut mark)"
    );
    assert!(
        got.iter()
            .all(|c| !String::from_utf8_lossy(&c.path).ends_with(PATH_CUT_MARK)),
        "TMC011-TREE-DIFF: a path never ends with the cut mark"
    );
}
