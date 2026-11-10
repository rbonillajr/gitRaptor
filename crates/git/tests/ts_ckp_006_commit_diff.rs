//! TS-CKP-006 T002 (CKP006-003, CKP006-005): the diff of a commit and between two commits over
//! Git, and the reachability rule: everything that is not a reachable commit is the same
//! `Lookup::Missing`. Temporary repositories only (NFR-01).

mod common;

use std::time::{Duration, Instant};

use common::Fixture;
use gitraptor_git::diff::{
    DiffLimits, GitBody, GitChange, GitSection, Lookup, is_full_oid, relative_components,
};
use gitraptor_git::{ReadError, ReaderOptions, RepoReader};

fn limits() -> DiffLimits {
    DiffLimits {
        max_files: 1_000,
        max_lines_per_file: 2_000,
        max_blob_bytes: 8 * 1024 * 1024,
        max_bytes: 768 * 1024,
        deadline: Instant::now() + Duration::from_secs(60),
    }
}

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn rev(f: &Fixture, spec: &str) -> String {
    f.git(&["rev-parse", spec]).trim().to_owned()
}

fn commit_all(f: &Fixture, msg: &str) -> String {
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", msg]);
    rev(f, "HEAD")
}

fn commit(f: &Fixture, oid: &str) -> (usize, GitSection) {
    match reader(f)
        .commit_diff(oid, &limits(), &|| false)
        .expect("commit_diff must not fail")
    {
        Lookup::Found(d) => (d.parents, d.section),
        Lookup::Missing => panic!("commit_diff answered Missing for the reachable commit {oid}"),
    }
}

fn between(f: &Fixture, old: &str, new: &str, path: Option<&str>) -> GitSection {
    match reader(f)
        .between_diff(old, new, path, &limits(), &|| false)
        .expect("between_diff must not fail")
    {
        Lookup::Found(s) => s,
        Lookup::Missing => panic!("between_diff answered Missing for reachable commits"),
    }
}

fn changes(s: &GitSection) -> Vec<(String, GitChange)> {
    s.files.iter().map(|f| (f.path.clone(), f.change)).collect()
}

#[test]
fn ckp006_003_root_commit_diffs_against_the_empty_tree() {
    let f = Fixture::with_commit();
    let (parents, section) = commit(&f, &rev(&f, "HEAD"));
    assert_eq!(parents, 0, "a root commit has no parents");
    assert_eq!(
        changes(&section),
        vec![
            ("a.txt".to_owned(), GitChange::Added),
            ("b.txt".to_owned(), GitChange::Added)
        ],
        "the root commit must diff against the empty tree: every file added"
    );
    assert!(matches!(section.files[0].body, GitBody::Text { .. }));
}

#[test]
fn ckp006_003_merge_diffs_against_the_first_parent() {
    let f = Fixture::with_commit();
    f.git(&["checkout", "-q", "-b", "feat"]);
    f.write("f.txt", "feature\n");
    commit_all(&f, "feature");
    f.git(&["checkout", "-q", "main"]);
    f.write("m.txt", "main\n");
    commit_all(&f, "main work");
    f.git(&["merge", "--no-ff", "-q", "-m", "merge feat", "feat"]);
    let (parents, section) = commit(&f, &rev(&f, "HEAD"));
    assert_eq!(parents, 2, "a merge reports parents = 2");
    assert_eq!(
        changes(&section),
        vec![("f.txt".to_owned(), GitChange::Added)],
        "a merge diffs against its first parent only"
    );
}

#[test]
fn ckp006_003_git_mv_is_a_delete_and_an_add() {
    let f = Fixture::with_commit();
    f.git(&["mv", "a.txt", "c.txt"]);
    let oid = commit_all(&f, "rename");
    let (_, section) = commit(&f, &oid);
    assert_eq!(
        changes(&section),
        vec![
            ("a.txt".to_owned(), GitChange::Deleted),
            ("c.txt".to_owned(), GitChange::Added)
        ],
        "no rename detection: a git mv is a deletion and an addition"
    );
}

#[test]
fn ckp006_003_between_with_a_path_brings_only_that_file() {
    let f = Fixture::with_commit();
    let first = rev(&f, "HEAD");
    f.write("a.txt", "alpha\nmore\n");
    f.write("b.txt", "beta\nmore\n");
    let second = commit_all(&f, "both");
    let section = between(&f, &first, &second, Some("b.txt"));
    assert_eq!(
        changes(&section),
        vec![("b.txt".to_owned(), GitChange::Modified)],
        "diff.between with a path reads only that file"
    );
    let absent = between(&f, &first, &second, Some("nope.txt"));
    assert!(
        absent.files.is_empty() && absent.files_total == 0,
        "a path absent on both sides gives an empty section: {absent:?}"
    );
}

#[test]
fn ckp006_003_unrelated_histories_diff_tree_to_tree() {
    let f = Fixture::with_commit();
    let main = rev(&f, "HEAD");
    f.git(&["checkout", "-q", "--orphan", "other"]);
    f.git(&["rm", "-rf", "-q", "."]);
    f.write("z.txt", "zeta\n");
    let other = commit_all(&f, "unrelated root");
    let section = between(&f, &main, &other, None);
    assert_eq!(
        changes(&section),
        vec![
            ("a.txt".to_owned(), GitChange::Deleted),
            ("b.txt".to_owned(), GitChange::Deleted),
            ("z.txt".to_owned(), GitChange::Added)
        ],
        "two histories without a common ancestor give the tree to tree diff"
    );
}

#[test]
fn ckp006_003_invalid_paths_are_rejected_as_invalid_input() {
    let long = "a".repeat(4_097);
    let bad = [
        "", "..", "../x", "a/../b", "a\0b", "a\u{7}b", "a\nb", "/abs", "a//b", &long,
    ];
    for path in bad {
        assert!(
            matches!(relative_components(path), Err(ReadError::InvalidInput(_))),
            "relative_components must reject {path:?}"
        );
    }
    assert_eq!(
        relative_components("dir/file.txt").unwrap(),
        vec!["dir", "file.txt"]
    );
    let f = Fixture::with_commit();
    let head = rev(&f, "HEAD");
    let err = reader(&f)
        .between_diff(&head, &head, Some("../x"), &limits(), &|| false)
        .err();
    assert!(
        matches!(err, Some(ReadError::InvalidInput(_))),
        "between_diff must apply the path validation again: {err:?}"
    );
}

#[test]
fn ckp006_003_is_full_oid_takes_only_lowercase_40_or_64_hex() {
    assert!(is_full_oid(&"a".repeat(40)));
    assert!(is_full_oid(&"0".repeat(64)));
    for bad in [
        "a".repeat(39),
        "a".repeat(41),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(40),
        "HEAD".to_owned(),
        "-".repeat(40),
    ] {
        assert!(!is_full_oid(&bad), "{bad} is not a full oid");
    }
}

// ------------------------------------------------------------------------------ reachability

fn is_missing(f: &Fixture, oid: &str) -> bool {
    matches!(
        reader(f).commit_diff(oid, &limits(), &|| false),
        Ok(Lookup::Missing)
    )
}

#[test]
fn ckp006_005_missing_and_unreachable_take_the_same_path() {
    let f = Fixture::with_commit();
    let head = rev(&f, "HEAD");
    // Control: a reachable commit is found.
    assert!(!is_missing(&f, &head), "the tip of main must be reachable");
    // A loose blob.
    let blob = {
        f.write("loose.txt", "loose\n");
        f.git(&["hash-object", "-w", "loose.txt"]).trim().to_owned()
    };
    // An orphan commit no ref points at.
    let tree = rev(&f, "HEAD^{tree}");
    let orphan = f
        .git(&["commit-tree", &tree, "-m", "orphan"])
        .trim()
        .to_owned();
    // A commit of another repository.
    let other = Fixture::with_commit();
    other.write("c.txt", "other\n");
    let foreign = commit_all(&other, "other repo");
    // Nothing at all, and an oid of another hash type.
    let ghost = "1".repeat(40);
    let sha256 = "a".repeat(64);
    for (name, oid) in [
        ("loose blob", blob),
        ("orphan commit", orphan),
        ("commit of another repo", foreign),
        ("nonexistent oid", ghost),
        ("oid of another hash type", sha256),
    ] {
        assert!(
            is_missing(&f, &oid),
            "{name} must be Lookup::Missing, like a nonexistent oid"
        );
    }
    // A blob and an orphan are Missing for between_diff too.
    let r = reader(&f);
    assert!(
        matches!(
            r.between_diff(&head, &"1".repeat(40), None, &limits(), &|| false),
            Ok(Lookup::Missing)
        ),
        "between_diff with a nonexistent side must be Missing"
    );
}
